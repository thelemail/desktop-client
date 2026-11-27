use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use thelemail_i18n::Locale;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub theme: Theme,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

pub struct Settings {
    path: PathBuf,
    current: Mutex<AppSettings>,
    writing: Mutex<()>,
}

impl Settings {
    pub fn load(path: PathBuf) -> Self {
        let current = read(&path).unwrap_or_default();
        Self {
            path,
            current: Mutex::new(current),
            writing: Mutex::new(()),
        }
    }

    pub fn current(&self) -> AppSettings {
        self.current.lock().unwrap().clone()
    }

    pub fn theme(&self) -> Theme {
        self.current.lock().unwrap().theme
    }

    pub fn set_theme(&self, theme: Theme) -> bool {
        let mut current = self.current.lock().unwrap();
        let changed = current.theme != theme;
        current.theme = theme;
        changed
    }

    pub fn locale(&self) -> Option<Locale> {
        Locale::parse(self.current.lock().unwrap().locale.as_deref()?)
    }

    pub fn set_locale(&self, locale: Option<Locale>) -> bool {
        let mut current = self.current.lock().unwrap();
        let next = locale.map(|l| l.code().to_string());
        let changed = current.locale != next;
        current.locale = next;
        changed
    }

    pub fn save(&self) -> std::io::Result<()> {
        let _writing = self.writing.lock().unwrap();
        let bytes = serde_json::to_vec_pretty(&self.current()).map_err(std::io::Error::other)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &self.path)
    }
}

fn read(path: &Path) -> Option<AppSettings> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            tracing::warn!("settings: reading failed: {err}");
            return None;
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(settings) => Some(settings),
        Err(err) => {
            tracing::warn!("settings: ignoring an unreadable file: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_follows_the_system() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::load(dir.path().join("settings.json"));
        assert_eq!(settings.theme(), Theme::System);
    }

    #[test]
    fn saved_theme_survives_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("settings.json");
        let settings = Settings::load(path.clone());
        assert!(settings.set_theme(Theme::Dark));
        assert!(!settings.set_theme(Theme::Dark));
        settings.save().unwrap();
        assert_eq!(Settings::load(path.clone()).theme(), Theme::Dark);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\n  \"theme\": \"dark\"\n}"
        );
    }

    #[test]
    fn corrupt_or_unknown_values_fall_back_to_the_system() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(Settings::load(path.clone()).theme(), Theme::System);
        std::fs::write(&path, br#"{"theme":"sepia"}"#).unwrap();
        assert_eq!(Settings::load(path.clone()).theme(), Theme::System);
        std::fs::write(&path, br#"{"later":true}"#).unwrap();
        assert_eq!(Settings::load(path).theme(), Theme::System);
    }

    #[test]
    fn saved_locale_survives_a_reload_and_automatic_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = Settings::load(path.clone());
        assert_eq!(settings.locale(), None);
        assert!(settings.set_locale(Some(Locale::De)));
        assert!(!settings.set_locale(Some(Locale::De)));
        settings.save().unwrap();
        let reloaded = Settings::load(path.clone());
        assert_eq!(reloaded.locale(), Some(Locale::De));
        assert!(reloaded.set_locale(None));
        reloaded.save().unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\n  \"theme\": \"system\"\n}"
        );
    }

    #[test]
    fn unknown_locales_fall_back_to_automatic_and_keep_the_theme() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, br#"{"theme":"dark","locale":"pt-BR"}"#).unwrap();
        let settings = Settings::load(path);
        assert_eq!(settings.locale(), None);
        assert_eq!(settings.theme(), Theme::Dark);
    }
}
