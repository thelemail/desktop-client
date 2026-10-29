use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tauri::{AppHandle, Manager};
use tauri_plugin_updater::Update;

use super::{UpdateError, Updates, check_version};

const BUNDLE_ID: &str = "com.thelemail.desktop";
const STAGING_PREFIX: &str = ".thelemail-update-";

#[cfg(not(debug_assertions))]
const TEAM_ID: Option<&str> = Some(env!("APPLE_TEAM_ID"));
#[cfg(debug_assertions)]
const TEAM_ID: Option<&str> = option_env!("APPLE_TEAM_ID");

pub struct Staged {
    dir: PathBuf,
    app: PathBuf,
}

pub fn installed() -> Option<PathBuf> {
    std::env::current_exe().ok().as_deref().and_then(bundle_of)
}

pub fn preflight() -> Result<(), UpdateError> {
    TEAM_ID
        .map(|_| ())
        .ok_or_else(|| UpdateError::new("verify", "built without a team id"))
}

pub fn stage(
    bundle: &Path,
    archive: &[u8],
    expected: &str,
    current: &str,
) -> Result<Staged, UpdateError> {
    let team = TEAM_ID.ok_or_else(|| UpdateError::new("verify", "built without a team id"))?;
    stage_with(bundle, archive, expected, current, team)
}

pub fn finish(app: &AppHandle, _update: Update, staged: Staged, bundle: &Path, version: &str) {
    match swap(&staged.app, bundle) {
        Ok(()) => {
            let _ = Command::new("/usr/bin/touch").arg(bundle).status();
        }
        Err(err) => {
            eprintln!("updates: swapping in {version} failed: {err}");
            let _ = std::fs::remove_dir_all(&staged.dir);
            app.state::<Updates>()
                .change(|p| p.last_failure = Some("install".to_owned()));
        }
    }
}

fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let named = |p: &Path, name: &str| p.file_name().is_some_and(|n| n == name);
    let is_app = bundle.extension().is_some_and(|ext| ext == "app");
    (named(macos, "MacOS") && named(contents, "Contents") && is_app).then(|| bundle.to_path_buf())
}

fn writable(path: &Path) -> bool {
    let Ok(raw) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::access(raw.as_ptr(), libc::W_OK) == 0 }
}

pub fn location_problem(bundle: &Path) -> Option<&'static str> {
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

fn stage_with(
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
    check_version(version, expected, current)
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

pub fn sweep(bundle: &Path) {
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
        assert!(stage_with(&bundle, &archive, "0.13.0", "0.12.0", "ABCDE12345").is_err());
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

        sweep(&bundle);
        assert!(!leftover.exists());
        assert!(lookalike.exists());
        assert!(bundle.exists());
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
}
