use std::path::PathBuf;

pub fn data_root() -> PathBuf {
    platform_root().join("com.thelemail.desktop")
}

pub fn accounts_root() -> PathBuf {
    data_root().join("accounts")
}

#[cfg(windows)]
fn platform_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(not(windows))]
fn platform_root() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library")
        .join("Application Support")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn the_data_directory_matches_the_tauri_install() {
        let root = data_root();
        assert!(root.ends_with("Library/Application Support/com.thelemail.desktop"));
        assert_eq!(accounts_root(), root.join("accounts"));
    }

    #[cfg(windows)]
    #[test]
    fn the_data_directory_matches_the_tauri_install() {
        let root = data_root();
        assert!(root.ends_with("com.thelemail.desktop"));
        assert_eq!(accounts_root(), root.join("accounts"));
    }
}
