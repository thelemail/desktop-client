use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};
use tauri_plugin_updater::Update;

use super::{UpdateError, Updates, check_version};

const PRODUCT_NAME: &str = "Thelemail";
const UNINSTALLER: &str = "uninstall.exe";

pub struct Staged {
    installer: Vec<u8>,
}

pub fn installed() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join(UNINSTALLER).is_file().then(|| dir.to_path_buf())
}

pub fn preflight() -> Result<(), UpdateError> {
    Ok(())
}

pub fn location_problem(_installed: &Path) -> Option<&'static str> {
    None
}

pub fn stage(
    _installed: &Path,
    installer: &[u8],
    expected: &str,
    current: &str,
) -> Result<Staged, UpdateError> {
    verify_installer(installer, expected, current)?;
    Ok(Staged {
        installer: installer.to_vec(),
    })
}

pub fn finish(app: &AppHandle, update: Update, staged: Staged, _installed: &Path, version: &str) {
    if let Err(err) = update.install(&staged.installer) {
        eprintln!("updates: launching the {version} installer failed: {err}");
        app.state::<Updates>()
            .change(|p| p.last_failure = Some("install".to_owned()));
    }
}

fn aligned(bytes: &[u8]) -> Vec<u64> {
    let mut words = vec![0u64; bytes.len().div_ceil(8)];
    for (word, chunk) in words.iter_mut().zip(bytes.chunks(8)) {
        let mut raw = [0u8; 8];
        raw[..chunk.len()].copy_from_slice(chunk);
        *word = u64::from_ne_bytes(raw);
    }
    words
}

fn installer_identity(installer: &[u8]) -> Result<(Option<String>, Option<String>), UpdateError> {
    use dataview::PodMethods;

    let words = aligned(installer);
    let image = &words.as_slice().as_bytes()[..installer.len()];
    let file = pelite::PeFile::from_bytes(image).map_err(|e| UpdateError::new("verify", e))?;
    let info = file
        .resources()
        .map_err(|e| UpdateError::new("verify", e))?
        .version_info()
        .map_err(|e| UpdateError::new("verify", e))?;
    let Some(&lang) = info.translation().first() else {
        return Err(UpdateError::new(
            "verify",
            "the installer carries no version strings",
        ));
    };
    Ok((
        info.value(lang, "ProductName"),
        info.value(lang, "ProductVersion"),
    ))
}

fn verify_installer(installer: &[u8], expected: &str, current: &str) -> Result<(), UpdateError> {
    let (product, version) = installer_identity(installer)?;
    if product.as_deref() != Some(PRODUCT_NAME) {
        return Err(UpdateError::new(
            "verify",
            "the installer is for a different product",
        ));
    }
    check_version(version.as_deref(), expected, current)
}

pub fn sweep(_installed: &Path) {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if is_leftover(name) && entry.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn is_leftover(name: &str) -> bool {
    name.strip_prefix(PRODUCT_NAME)
        .and_then(|rest| rest.strip_prefix('-'))
        .and_then(|rest| rest.split_once("-updater-"))
        .is_some_and(|(version, _)| semver::Version::parse(version).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OURS: &[u8] = include_bytes!("../../../fixtures/installer/thelemail-0.13.0.exe");
    const OTHER: &[u8] = include_bytes!("../../../fixtures/installer/other-0.13.0.exe");

    #[test]
    fn the_installer_version_strings_are_read() {
        let (product, version) = installer_identity(OURS).expect("identity");
        assert_eq!(product.as_deref(), Some("Thelemail"));
        assert_eq!(version.as_deref(), Some("0.13.0"));
    }

    #[test]
    fn only_a_newer_installer_of_this_product_for_the_offered_version_passes() {
        assert!(verify_installer(OURS, "0.13.0", "0.12.0").is_ok());
        assert!(
            verify_installer(OURS, "0.14.0", "0.12.0").is_err(),
            "an old signed installer relabelled as new must be refused"
        );
        assert!(
            verify_installer(OURS, "0.13.0", "0.13.0").is_err(),
            "reinstalling the running version is not an update"
        );
        assert!(
            verify_installer(OTHER, "0.13.0", "0.12.0").is_err(),
            "another product's installer must be refused"
        );
    }

    #[test]
    fn anything_but_a_pe_installer_is_refused() {
        assert!(verify_installer(b"MZ not really", "0.13.0", "0.12.0").is_err());
        assert!(verify_installer(&OURS[..512], "0.13.0", "0.12.0").is_err());
        assert!(verify_installer(&[], "0.13.0", "0.12.0").is_err());
    }

    #[test]
    fn only_the_updater_leftovers_are_swept() {
        assert!(is_leftover("Thelemail-0.22.0-updater-a1B2c3"));
        assert!(!is_leftover("Thelemail-notes"));
        assert!(!is_leftover("Thelemail-x-updater-a1B2c3"));
        assert!(!is_leftover("Other-0.22.0-updater-a1B2c3"));
        assert!(!is_leftover("Thelemail"));
    }
}
