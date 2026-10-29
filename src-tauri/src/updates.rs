use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};

use crate::mirror::Mirror;
use crate::sse::Streams;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
mod windows;
#[cfg(windows)]
use windows as platform;

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);
const TICK: Duration = Duration::from_secs(10 * 60);
const CHECK_EVERY_SECS: i64 = 6 * 60 * 60;
const SNOOZE_SECS: i64 = 3 * 24 * 60 * 60;
const CHECK_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
const NOTES_LIMIT: usize = 4000;

const RELEASES_URL: &str = "https://github.com/thelemail/desktop-client/releases";
const STATE_FILE: &str = "updates.json";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateError {
    pub code: &'static str,
    pub detail: String,
}

impl UpdateError {
    fn new(code: &'static str, detail: impl ToString) -> Self {
        Self {
            code,
            detail: detail.to_string(),
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Persisted {
    #[serde(default)]
    snoozed_version: Option<String>,
    #[serde(default)]
    snoozed_until: Option<i64>,
    #[serde(default)]
    last_check: Option<i64>,
    #[serde(default)]
    last_failure: Option<String>,
}

impl Persisted {
    fn hides(&self, version: &str, now: i64) -> bool {
        self.snoozed_version.as_deref() == Some(version)
            && self.snoozed_until.is_some_and(|until| now < until)
    }

    fn due(&self, now: i64) -> bool {
        self.last_check
            .is_none_or(|last| now < last || now - last >= CHECK_EVERY_SECS)
    }

    fn snooze(&mut self, version: &str, now: i64) {
        self.snoozed_version = Some(version.to_owned());
        self.snoozed_until = Some(now + SNOOZE_SECS);
    }
}

fn load(path: &Path) -> Persisted {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(path: &Path, persisted: &Persisted) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(persisted).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, bytes)?;
    std::fs::File::open(&tmp)?.sync_all()?;
    std::fs::rename(&tmp, path)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Available {
    pub version: String,
    pub notes: Option<String>,
    pub published_at: Option<String>,
    pub release_url: String,
}

fn describe(update: &Update) -> Available {
    Available {
        version: update.version.clone(),
        notes: update
            .body
            .as_deref()
            .map(|notes| notes.chars().take(NOTES_LIMIT).collect()),
        published_at: update
            .raw_json
            .get("pub_date")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        release_url: release_url(&update.version),
    }
}

fn release_url(version: &str) -> String {
    format!("{RELEASES_URL}/tag/v{version}")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub current_version: String,
    pub available: Option<Available>,
    pub snoozed: bool,
    pub last_check: Option<i64>,
    pub last_failure: Option<String>,
    pub installing: bool,
    pub blocked: Option<&'static str>,
    pub releases_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
    phase: &'static str,
}

pub struct Updates {
    path: PathBuf,
    persisted: Mutex<Persisted>,
    offered: Mutex<Option<Update>>,
    announced: Mutex<Option<String>>,
    installing: AtomicBool,
}

impl Updates {
    pub fn new(dir: &Path) -> Self {
        let path = dir.join(STATE_FILE);
        Self {
            persisted: Mutex::new(load(&path)),
            path,
            offered: Mutex::default(),
            announced: Mutex::default(),
            installing: AtomicBool::new(false),
        }
    }

    fn persisted(&self) -> Persisted {
        self.persisted.lock().expect("updates state").clone()
    }

    fn change(&self, f: impl FnOnce(&mut Persisted)) {
        let mut persisted = self.persisted.lock().expect("updates state");
        f(&mut persisted);
        if let Err(err) = save(&self.path, &persisted) {
            eprintln!("updates: saving state failed: {err}");
        }
    }

    fn offered(&self) -> Option<Update> {
        self.offered.lock().expect("updates offer").clone()
    }
}

#[cfg(windows)]
pub fn installed() -> Option<PathBuf> {
    platform::installed()
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

fn updater(app: &AppHandle, timeout: Duration) -> Result<Updater, tauri_plugin_updater::Error> {
    #[allow(unused_mut)]
    let mut builder = app.updater_builder().timeout(timeout);
    #[cfg(debug_assertions)]
    {
        if let Some(endpoint) = std::env::var("THELEMAIL_UPDATE_ENDPOINT")
            .ok()
            .and_then(|raw| raw.parse::<tauri::Url>().ok())
        {
            builder = builder.endpoints(vec![endpoint])?;
        }
        if let Ok(pubkey) = std::env::var("THELEMAIL_UPDATE_PUBKEY") {
            builder = builder.pubkey(pubkey);
        }
    }
    builder.build()
}

async fn check(app: &AppHandle) -> Result<Option<Available>, UpdateError> {
    let found = updater(app, CHECK_TIMEOUT)
        .map_err(|e| UpdateError::new("check", e))?
        .check()
        .await
        .map_err(|e| UpdateError::new("check", e))?;

    let updates = app.state::<Updates>();
    updates.change(|p| {
        p.last_check = Some(now_unix());
        if found.is_none() {
            p.last_failure = None;
        }
    });
    let available = found.as_ref().map(describe);
    if !updates.installing.load(Ordering::SeqCst) {
        *updates.offered.lock().expect("updates offer") = found;
    }
    Ok(available)
}

fn announce(app: &AppHandle) {
    let updates = app.state::<Updates>();
    let Some(update) = updates.offered() else {
        return;
    };
    if updates.persisted().hides(&update.version, now_unix()) {
        return;
    }
    let mut announced = updates.announced.lock().expect("updates announced");
    if announced.as_deref() == Some(update.version.as_str()) {
        return;
    }
    *announced = Some(update.version.clone());
    let _ = app.emit("updates://available", describe(&update));
}

pub fn spawn(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(installed) = platform::installed() {
            let _ = tauri::async_runtime::spawn_blocking(move || platform::sweep(&installed)).await;
        }
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            let updates = app.state::<Updates>();
            if !updates.installing.load(Ordering::SeqCst)
                && updates.persisted().due(now_unix())
                && let Err(err) = check(&app).await
            {
                eprintln!("updates: check failed: {}", err.detail);
            }
            announce(&app);
            tokio::time::sleep(TICK).await;
        }
    });
}

#[tauri::command]
pub fn updates_status(updates: State<'_, Updates>) -> Status {
    let persisted = updates.persisted();
    let offered = updates.offered();
    Status {
        current_version: env!("CARGO_PKG_VERSION").to_owned(),
        snoozed: offered
            .as_ref()
            .is_some_and(|u| persisted.hides(&u.version, now_unix())),
        available: offered.as_ref().map(describe),
        last_check: persisted.last_check,
        last_failure: persisted.last_failure,
        installing: updates.installing.load(Ordering::SeqCst),
        blocked: match platform::installed() {
            Some(installed) => platform::location_problem(&installed),
            None => Some("unbundled"),
        },
        releases_url: RELEASES_URL.to_owned(),
    }
}

#[tauri::command]
pub async fn updates_check(app: AppHandle) -> Result<Option<Available>, UpdateError> {
    if app.state::<Updates>().installing.load(Ordering::SeqCst) {
        return Err(UpdateError::new("busy", "an update is already installing"));
    }
    check(&app).await
}

#[tauri::command]
pub fn updates_snooze(updates: State<'_, Updates>, version: String) {
    updates.change(|p| p.snooze(&version, now_unix()));
    *updates.announced.lock().expect("updates announced") = None;
}

#[tauri::command]
pub async fn updates_install(app: AppHandle, version: String) -> Result<(), UpdateError> {
    let updates = app.state::<Updates>();
    if updates.installing.swap(true, Ordering::SeqCst) {
        return Err(UpdateError::new("busy", "an update is already installing"));
    }
    updates.change(|p| p.last_failure = None);

    let result = install(&app, &version).await;
    if let Err(err) = &result {
        eprintln!(
            "updates: install of {version} failed: {} {}",
            err.code, err.detail
        );
        updates.change(|p| p.last_failure = Some(err.code.to_owned()));
        updates.installing.store(false, Ordering::SeqCst);
    }
    result
}

async fn install(app: &AppHandle, version: &str) -> Result<(), UpdateError> {
    let update = app
        .state::<Updates>()
        .offered()
        .filter(|u| u.version == version)
        .ok_or_else(|| UpdateError::new("stale", "this version is no longer offered"))?;
    platform::preflight()?;
    let installed = platform::installed().ok_or_else(|| UpdateError::new("unbundled", ""))?;
    if let Some(problem) = platform::location_problem(&installed) {
        return Err(UpdateError::new(problem, installed.display()));
    }

    let mut update = update;
    update.timeout = Some(DOWNLOAD_TIMEOUT);

    let mut downloaded = 0u64;
    let mut last_emit = Instant::now() - PROGRESS_INTERVAL;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                if last_emit.elapsed() >= PROGRESS_INTERVAL {
                    last_emit = Instant::now();
                    let _ = app.emit(
                        "updates://progress",
                        Progress {
                            downloaded,
                            total,
                            phase: "download",
                        },
                    );
                }
            },
            || {},
        )
        .await
        .map_err(|e| match e {
            tauri_plugin_updater::Error::Minisign(_)
            | tauri_plugin_updater::Error::Base64(_)
            | tauri_plugin_updater::Error::SignatureUtf8(_) => UpdateError::new("verify", e),
            other => UpdateError::new("download", other),
        })?;

    let _ = app.emit(
        "updates://progress",
        Progress {
            downloaded,
            total: Some(downloaded),
            phase: "verify",
        },
    );

    let current = env!("CARGO_PKG_VERSION");
    let expected = update.version.clone();
    let target = installed.clone();
    let staged = tauri::async_runtime::spawn_blocking(move || {
        platform::stage(&target, &bytes, &expected, current)
    })
    .await
    .map_err(|e| UpdateError::new("install", e))??;

    let _ = app.emit(
        "updates://progress",
        Progress {
            downloaded,
            total: Some(downloaded),
            phase: "restart",
        },
    );

    app.state::<Streams>().close_all();
    app.state::<Mirror>().shutdown();

    platform::finish(app, update, staged, &installed, version);
    app.restart()
}

fn check_version(version: Option<&str>, expected: &str, current: &str) -> Result<(), UpdateError> {
    let Some(version) = version else {
        return Err(UpdateError::new("verify", "the bundle has no version"));
    };
    if version != expected {
        return Err(UpdateError::new(
            "verify",
            format!("the bundle is {version}, the release claims {expected}"),
        ));
    }
    let parse = |v: &str| semver::Version::parse(v).map_err(|e| UpdateError::new("verify", e));
    if parse(version)? <= parse(current)? {
        return Err(UpdateError::new(
            "verify",
            format!("{version} is not newer than {current}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snooze_hides_only_that_version_and_only_until_it_expires() {
        let mut p = Persisted::default();
        let now = 1_000_000;
        p.snooze("0.13.0", now);
        assert!(p.hides("0.13.0", now));
        assert!(p.hides("0.13.0", now + SNOOZE_SECS - 1));
        assert!(
            !p.hides("0.13.0", now + SNOOZE_SECS),
            "the snooze must lapse"
        );
        assert!(
            !p.hides("0.13.1", now),
            "a newer release must surface through an older snooze"
        );
    }

    #[test]
    fn checks_are_due_every_six_hours_and_after_the_clock_moves_backwards() {
        let mut p = Persisted::default();
        assert!(p.due(0), "a first launch checks");
        p.last_check = Some(10_000);
        assert!(!p.due(10_000 + CHECK_EVERY_SECS - 1));
        assert!(p.due(10_000 + CHECK_EVERY_SECS));
        assert!(p.due(9_000));
    }

    #[test]
    fn state_survives_a_reload_and_a_corrupt_file_resets_to_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let updates = Updates::new(dir.path());
        updates.change(|p| {
            p.snooze("0.13.0", 42);
            p.last_check = Some(7);
        });
        assert!(!dir.path().join("updates.json.tmp").exists());
        let reloaded = Updates::new(dir.path()).persisted();
        assert_eq!(reloaded.snoozed_version.as_deref(), Some("0.13.0"));
        assert_eq!(reloaded.last_check, Some(7));

        std::fs::write(dir.path().join(STATE_FILE), b"{not json").expect("corrupt");
        assert_eq!(Updates::new(dir.path()).persisted(), Persisted::default());
    }

    #[test]
    fn an_installer_must_match_the_release_and_be_newer() {
        assert!(check_version(Some("0.13.0"), "0.13.0", "0.12.0").is_ok());
        assert!(
            check_version(Some("0.11.0"), "0.13.0", "0.12.0").is_err(),
            "an old signed build relabelled as new must be refused"
        );
        assert!(
            check_version(Some("0.12.0"), "0.12.0", "0.12.0").is_err(),
            "reinstalling the running version is not an update"
        );
        assert!(
            check_version(Some("0.11.0"), "0.11.0", "0.12.0").is_err(),
            "a downgrade must be refused even when the manifest agrees"
        );
        assert!(check_version(None, "0.13.0", "0.12.0").is_err());
        assert!(check_version(Some("not-a-version"), "not-a-version", "0.12.0").is_err());
    }
}
