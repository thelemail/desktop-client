use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos as platform;
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
pub mod windows;
#[cfg(windows)]
pub use windows as platform;

const CHECK_EVERY_SECS: i64 = 6 * 60 * 60;
const SNOOZE_SECS: i64 = 3 * 24 * 60 * 60;
pub const NOTES_LIMIT: usize = 4000;

const RELEASES_URL: &str = "https://github.com/thelemail/desktop-client/releases";
const STATE_FILE: &str = "updates.json";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateError {
    pub code: &'static str,
    pub detail: String,
}

impl UpdateError {
    pub fn new(code: &'static str, detail: impl ToString) -> Self {
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
    let mut file = std::fs::File::create(&tmp)?;
    std::io::Write::write_all(&mut file, &bytes)?;
    file.sync_all()?;
    drop(file);
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

pub fn release_url(version: &str) -> String {
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

pub struct Updates {
    path: PathBuf,
    persisted: Mutex<Persisted>,
    offered: Mutex<Option<Available>>,
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

    fn offered(&self) -> Option<Available> {
        self.offered.lock().expect("updates offer").clone()
    }

    pub fn due(&self) -> bool {
        !self.installing() && self.persisted().due(now_unix())
    }

    pub fn installing(&self) -> bool {
        self.installing.load(Ordering::SeqCst)
    }

    pub fn checked(&self, found: Option<Available>) {
        let none = found.is_none();
        self.change(|p| {
            p.last_check = Some(now_unix());
            if none {
                p.last_failure = None;
            }
        });
        if !self.installing() {
            *self.offered.lock().expect("updates offer") = found;
        }
    }

    pub fn announcement(&self) -> Option<Available> {
        let update = self.offered()?;
        if self.persisted().hides(&update.version, now_unix()) {
            return None;
        }
        let mut announced = self.announced.lock().expect("updates announced");
        if announced.as_deref() == Some(update.version.as_str()) {
            return None;
        }
        *announced = Some(update.version.clone());
        Some(update)
    }

    pub fn snooze(&self, version: &str) {
        self.change(|p| p.snooze(version, now_unix()));
        *self.announced.lock().expect("updates announced") = None;
    }

    pub fn begin_install(&self, version: &str) -> Result<Available, UpdateError> {
        if self.installing.swap(true, Ordering::SeqCst) {
            return Err(UpdateError::new("busy", "an update is already installing"));
        }
        self.change(|p| p.last_failure = None);
        match self.offered().filter(|u| u.version == version) {
            Some(update) => Ok(update),
            None => {
                self.installing.store(false, Ordering::SeqCst);
                Err(UpdateError::new(
                    "stale",
                    "this version is no longer offered",
                ))
            }
        }
    }

    pub fn install_failed(&self, err: &UpdateError) {
        self.change(|p| p.last_failure = Some(err.code.to_owned()));
        self.installing.store(false, Ordering::SeqCst);
    }

    pub fn status(&self) -> Status {
        let persisted = self.persisted();
        let offered = self.offered();
        Status {
            current_version: env!("CARGO_PKG_VERSION").to_owned(),
            snoozed: offered
                .as_ref()
                .is_some_and(|u| persisted.hides(&u.version, now_unix())),
            available: offered,
            last_check: persisted.last_check,
            last_failure: persisted.last_failure,
            installing: self.installing(),
            blocked: blocked(),
            releases_url: RELEASES_URL.to_owned(),
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
fn blocked() -> Option<&'static str> {
    match platform::installed() {
        Some(installed) => platform::location_problem(&installed),
        None => Some("unbundled"),
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn blocked() -> Option<&'static str> {
    Some("unbundled")
}

#[cfg(any(target_os = "macos", windows))]
pub fn installed() -> Option<PathBuf> {
    platform::installed()
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn installed() -> Option<PathBuf> {
    None
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
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

    fn offer(version: &str) -> Available {
        Available {
            version: version.to_owned(),
            notes: None,
            published_at: None,
            release_url: release_url(version),
        }
    }

    #[test]
    fn an_offer_is_announced_once_until_snoozed_or_replaced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let updates = Updates::new(dir.path());
        updates.checked(Some(offer("0.25.0")));
        assert_eq!(
            updates.announcement().map(|a| a.version),
            Some("0.25.0".to_owned())
        );
        assert!(updates.announcement().is_none());
        updates.snooze("0.25.0");
        assert!(updates.announcement().is_none());
        assert!(updates.status().snoozed);
        updates.checked(Some(offer("0.25.1")));
        assert_eq!(
            updates.announcement().map(|a| a.version),
            Some("0.25.1".to_owned())
        );
    }

    #[test]
    fn only_the_offered_version_installs_and_only_once_at_a_time() {
        let dir = tempfile::tempdir().expect("tempdir");
        let updates = Updates::new(dir.path());
        updates.checked(Some(offer("0.25.0")));
        assert_eq!(updates.begin_install("0.24.9").unwrap_err().code, "stale");
        assert!(!updates.installing());
        assert!(updates.begin_install("0.25.0").is_ok());
        assert_eq!(updates.begin_install("0.25.0").unwrap_err().code, "busy");
        updates.install_failed(&UpdateError::new("download", "offline"));
        assert!(!updates.installing());
        assert_eq!(updates.status().last_failure.as_deref(), Some("download"));
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
