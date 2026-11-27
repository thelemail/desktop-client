use std::path::PathBuf;

const IDENTIFIER: &str = "com.thelemail.desktop";

pub fn data_root() -> PathBuf {
    platform_root().join(IDENTIFIER)
}

pub fn accounts_root() -> PathBuf {
    data_root().join("accounts")
}

#[cfg(windows)]
pub fn logs_root() -> PathBuf {
    data_root().join("logs")
}

#[cfg(not(windows))]
pub fn logs_root() -> PathBuf {
    home().join("Library").join("Logs").join(IDENTIFIER)
}

#[cfg(windows)]
fn platform_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(not(windows))]
fn platform_root() -> PathBuf {
    home().join("Library").join("Application Support")
}

#[cfg(not(windows))]
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
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

    #[cfg(not(windows))]
    #[test]
    fn logs_go_to_the_user_logs_folder() {
        assert!(logs_root().ends_with("Library/Logs/com.thelemail.desktop"));
    }

    #[cfg(windows)]
    #[test]
    fn logs_go_next_to_the_data() {
        assert_eq!(logs_root(), data_root().join("logs"));
    }
}
