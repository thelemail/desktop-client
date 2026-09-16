use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};

use crate::mirror::Mirror;
use crate::sse::Streams;

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);
const TICK: Duration = Duration::from_secs(10 * 60);
const CHECK_EVERY_SECS: i64 = 6 * 60 * 60;
const SNOOZE_SECS: i64 = 3 * 24 * 60 * 60;
const CHECK_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
const NOTES_LIMIT: usize = 4000;

const BUNDLE_ID: &str = "com.thelemail.desktop";
const RELEASES_URL: &str = "https://github.com/thelemail/desktop-client/releases";
const STATE_FILE: &str = "updates.json";
const STAGING_PREFIX: &str = ".thelemail-update-";

#[cfg(not(debug_assertions))]
const TEAM_ID: Option<&str> = Some(env!("APPLE_TEAM_ID"));
#[cfg(debug_assertions)]
const TEAM_ID: Option<&str> = option_env!("APPLE_TEAM_ID");

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
        if let Some(bundle) = running_bundle() {
            let _ = tauri::async_runtime::spawn_blocking(move || sweep_staging(&bundle)).await;
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
        blocked: match running_bundle() {
            Some(bundle) => location_problem(&bundle),
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
    let team = TEAM_ID.ok_or_else(|| UpdateError::new("verify", "built without a team id"))?;
    let bundle = running_bundle().ok_or_else(|| UpdateError::new("unbundled", ""))?;
    if let Some(problem) = location_problem(&bundle) {
        return Err(UpdateError::new(problem, bundle.display()));
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
    let target = bundle.clone();
    let staged = tauri::async_runtime::spawn_blocking(move || {
        stage(&target, &bytes, &expected, current, team)
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

    let swapped = {
        let staged_app = staged.app.clone();
        let target = bundle.clone();
        tauri::async_runtime::spawn_blocking(move || swap(&staged_app, &target))
            .await
            .map_err(std::io::Error::other)
            .and_then(|r| r)
    };
    match swapped {
        Ok(()) => {
            let _ = Command::new("/usr/bin/touch").arg(&bundle).status();
        }
        Err(err) => {
            eprintln!("updates: swapping in {version} failed: {err}");
            let _ = std::fs::remove_dir_all(&staged.dir);
            app.state::<Updates>()
                .change(|p| p.last_failure = Some("install".to_owned()));
        }
    }
    app.restart()
}

fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let named = |p: &Path, name: &str| p.file_name().is_some_and(|n| n == name);
    let is_app = bundle.extension().is_some_and(|ext| ext == "app");
    (named(macos, "MacOS") && named(contents, "Contents") && is_app).then(|| bundle.to_path_buf())
}

fn running_bundle() -> Option<PathBuf> {
    std::env::current_exe().ok().as_deref().and_then(bundle_of)
}

fn writable(path: &Path) -> bool {
    let Ok(raw) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::access(raw.as_ptr(), libc::W_OK) == 0 }
}

fn location_problem(bundle: &Path) -> Option<&'static str> {
    if bundle
        .components()
        .any(|c| c.as_os_str() == "AppTranslocation")
    {
        return Some("translocated");
    }
    match bundle.parent() {
        Some(parent) if writable(parent) && writable(bundle) => None,
        _ => Some("read-only"),
    }
}

struct Staged {
    dir: PathBuf,
    app: PathBuf,
}

fn stage(
    bundle: &Path,
    archive: &[u8],
    expected: &str,
    current: &str,
    team: &str,
) -> Result<Staged, UpdateError> {
    let parent = bundle
        .parent()
        .ok_or_else(|| UpdateError::new("read-only", bundle.display()))?;
    let dir = parent.join(format!("{STAGING_PREFIX}{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).map_err(|e| UpdateError::new("install", e))?;

    let result = unpack(archive, &dir).and_then(|app| {
        verify_identity(&app, expected, current)?;
        verify_signature(&app, team)?;
        #[cfg(not(debug_assertions))]
        verify_gatekeeper(&app)?;
        Ok(app)
    });
    match result {
        Ok(app) => Ok(Staged { dir, app }),
        Err(err) => {
            let _ = std::fs::remove_dir_all(&dir);
            Err(err)
        }
    }
}

fn unpack(archive: &[u8], dir: &Path) -> Result<PathBuf, UpdateError> {
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    tar.set_overwrite(false);
    tar.unpack(dir).map_err(|e| UpdateError::new("verify", e))?;

    let entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| UpdateError::new("install", e))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    let [app] = entries.as_slice() else {
        return Err(UpdateError::new(
            "verify",
            "the archive must hold exactly one app bundle",
        ));
    };
    let is_dir = std::fs::symlink_metadata(app).is_ok_and(|m| m.file_type().is_dir());
    if !is_dir || app.extension().is_none_or(|ext| ext != "app") {
        return Err(UpdateError::new(
            "verify",
            "the archive does not hold an app bundle",
        ));
    }
    Ok(app.clone())
}

fn verify_identity(app: &Path, expected: &str, current: &str) -> Result<(), UpdateError> {
    let info: plist::Dictionary = plist::from_file(app.join("Contents").join("Info.plist"))
        .map_err(|e| UpdateError::new("verify", e))?;
    let field = |key: &str| info.get(key).and_then(plist::Value::as_string);
    check_identity(
        field("CFBundleIdentifier"),
        field("CFBundleShortVersionString"),
        expected,
        current,
    )
}

fn check_identity(
    identifier: Option<&str>,
    version: Option<&str>,
    expected: &str,
    current: &str,
) -> Result<(), UpdateError> {
    if identifier != Some(BUNDLE_ID) {
        return Err(UpdateError::new(
            "verify",
            "the bundle identifier does not match",
        ));
    }
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

fn requirement(team: &str) -> Result<String, UpdateError> {
    let well_formed = team.len() == 10
        && team
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if !well_formed {
        return Err(UpdateError::new(
            "verify",
            "the pinned team id is malformed",
        ));
    }
    Ok(format!(
        "=anchor apple generic and identifier \"{BUNDLE_ID}\" \
         and certificate 1[field.1.2.840.113635.100.6.2.6] exists \
         and certificate leaf[field.1.2.840.113635.100.6.1.13] exists \
         and certificate leaf[subject.OU] = \"{team}\""
    ))
}

fn verify_signature(app: &Path, team: &str) -> Result<(), UpdateError> {
    let output = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict", "-R"])
        .arg(requirement(team)?)
        .arg(app)
        .output()
        .map_err(|e| UpdateError::new("verify", e))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(UpdateError::new(
            "verify",
            String::from_utf8_lossy(&output.stderr).trim(),
        ))
    }
}

#[cfg(not(debug_assertions))]
fn verify_gatekeeper(app: &Path) -> Result<(), UpdateError> {
    let output = Command::new("/usr/sbin/spctl")
        .args(["--assess", "--type", "execute"])
        .arg(app)
        .output()
        .map_err(|e| UpdateError::new("verify", e))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(UpdateError::new(
            "verify",
            String::from_utf8_lossy(&output.stderr).trim(),
        ))
    }
}

fn swap(staged: &Path, bundle: &Path) -> std::io::Result<()> {
    let from = CString::new(staged.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
    let to = CString::new(bundle.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
    if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_SWAP) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn sweep_staging(bundle: &Path) {
    let Some(parent) = bundle.parent() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(suffix) = name.to_str().and_then(|n| n.strip_prefix(STAGING_PREFIX)) else {
            continue;
        };
        if uuid::Uuid::parse_str(suffix).is_err() {
            continue;
        }
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
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
    fn the_bundle_is_found_only_from_a_real_app_executable() {
        assert_eq!(
            bundle_of(Path::new(
                "/Applications/Thelemail.app/Contents/MacOS/thelemail-desktop"
            )),
            Some(PathBuf::from("/Applications/Thelemail.app"))
        );
        assert_eq!(
            bundle_of(Path::new(
                "/Users/me/Apps/Mail 2.app/Contents/MacOS/thelemail-desktop"
            )),
            Some(PathBuf::from("/Users/me/Apps/Mail 2.app"))
        );
        assert_eq!(
            bundle_of(Path::new("/repo/target/debug/thelemail-desktop")),
            None
        );
        assert_eq!(bundle_of(Path::new("/tmp/Evil/Contents/MacOS/bin")), None);
    }

    #[test]
    fn translocated_and_read_only_locations_refuse_to_update() {
        assert_eq!(
            location_problem(Path::new(
                "/private/var/folders/xy/T/AppTranslocation/ABCD/d/Thelemail.app"
            )),
            Some("translocated")
        );

        let dir = tempfile::tempdir().expect("tempdir");
        let bundle = dir.path().join("Thelemail.app");
        std::fs::create_dir(&bundle).expect("bundle");
        assert_eq!(location_problem(&bundle), None);

        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555))
            .expect("chmod");
        assert_eq!(location_problem(&bundle), Some("read-only"));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
    }

    #[test]
    fn the_unpacked_bundle_must_match_the_release_and_be_newer() {
        assert!(check_identity(Some(BUNDLE_ID), Some("0.13.0"), "0.13.0", "0.12.0").is_ok());
        assert!(
            check_identity(Some(BUNDLE_ID), Some("0.11.0"), "0.13.0", "0.12.0").is_err(),
            "an old signed build relabelled as new must be refused"
        );
        assert!(
            check_identity(Some(BUNDLE_ID), Some("0.12.0"), "0.12.0", "0.12.0").is_err(),
            "reinstalling the running version is not an update"
        );
        assert!(
            check_identity(Some(BUNDLE_ID), Some("0.11.0"), "0.11.0", "0.12.0").is_err(),
            "a downgrade must be refused even when the manifest agrees"
        );
        assert!(
            check_identity(
                Some("com.example.other"),
                Some("0.13.0"),
                "0.13.0",
                "0.12.0"
            )
            .is_err()
        );
        assert!(check_identity(Some(BUNDLE_ID), None, "0.13.0", "0.12.0").is_err());
    }

    #[test]
    fn only_a_well_formed_team_id_reaches_the_code_requirement() {
        let req = requirement("ABCDE12345").expect("requirement");
        assert!(req.contains("certificate leaf[subject.OU] = \"ABCDE12345\""));
        assert!(req.contains(&format!("identifier \"{BUNDLE_ID}\"")));
        for hostile in [
            "",
            "abcde12345",
            "ABCDE1234",
            "ABC\" or true",
            "ABCDE123456",
        ] {
            assert!(requirement(hostile).is_err(), "accepted {hostile:?}");
        }
    }

    fn archive_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (path, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, path, *data)
                .expect("append");
        }
        builder.into_inner().expect("tar").finish().expect("gzip")
    }

    #[test]
    fn unpacking_yields_the_single_app_bundle() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = archive_with(&[("Thelemail.app/Contents/MacOS/thelemail-desktop", b"bin")]);
        let app = unpack(&archive, dir.path()).expect("unpack");
        assert_eq!(app, dir.path().join("Thelemail.app"));
    }

    #[test]
    fn an_archive_without_exactly_one_bundle_is_refused() {
        let two = archive_with(&[
            ("Thelemail.app/Contents/Info.plist", b"x"),
            ("Other.app/Contents/Info.plist", b"x"),
        ]);
        assert!(unpack(&two, tempfile::tempdir().expect("tempdir").path()).is_err());

        let loose = archive_with(&[("payload", b"x")]);
        assert!(unpack(&loose, tempfile::tempdir().expect("tempdir").path()).is_err());
    }

    #[test]
    fn a_failed_stage_leaves_nothing_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bundle = dir.path().join("Thelemail.app");
        std::fs::create_dir(&bundle).expect("bundle");
        let archive = archive_with(&[("Thelemail.app/Contents/Info.plist", b"not a plist")]);
        assert!(stage(&bundle, &archive, "0.13.0", "0.12.0", "ABCDE12345").is_err());
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read")
            .flatten()
            .collect();
        assert_eq!(left.len(), 1, "only the running bundle may remain");
    }

    #[test]
    fn the_swap_exchanges_both_bundles_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let current = dir.path().join("Thelemail.app");
        let staged = dir
            .path()
            .join(format!("{STAGING_PREFIX}{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&current).expect("current");
        std::fs::create_dir(&staged).expect("staged");
        std::fs::write(current.join("marker"), "old").expect("old");
        std::fs::write(staged.join("marker"), "new").expect("new");

        swap(&staged, &current).expect("swap");
        assert_eq!(
            std::fs::read_to_string(current.join("marker")).expect("read"),
            "new"
        );
        assert_eq!(
            std::fs::read_to_string(staged.join("marker")).expect("read"),
            "old"
        );

        assert!(
            swap(&dir.path().join("missing"), &current).is_err(),
            "a failed swap must report instead of half-moving"
        );
        assert_eq!(
            std::fs::read_to_string(current.join("marker")).expect("read"),
            "new"
        );
    }

    #[test]
    fn leftover_staging_directories_are_swept_and_nothing_else_is() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bundle = dir.path().join("Thelemail.app");
        std::fs::create_dir(&bundle).expect("bundle");
        let leftover = dir
            .path()
            .join(format!("{STAGING_PREFIX}{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&leftover).expect("leftover");
        let lookalike = dir.path().join(format!("{STAGING_PREFIX}not-a-uuid"));
        std::fs::create_dir(&lookalike).expect("lookalike");

        sweep_staging(&bundle);
        assert!(!leftover.exists());
        assert!(lookalike.exists());
        assert!(bundle.exists());
    }
}
