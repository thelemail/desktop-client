use std::sync::Mutex;

use tauri::menu::MenuItem;
use tauri::{AppHandle, Manager, Wry};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Text {
    ShowApp,
    Quit,
    NewMessage,
    NoSubject,
}

const STRINGS: &[(&str, Text, &str)] = &[
    ("en", Text::ShowApp, "Show Thelemail"),
    ("en", Text::Quit, "Quit"),
    ("en", Text::NewMessage, "New message"),
    ("en", Text::NoSubject, "(no subject)"),
];

const LOCALES: &[&str] = &["en", "de", "fr", "pt"];

pub fn text(locale: &str, key: Text) -> &'static str {
    let find = |loc: &str| {
        STRINGS
            .iter()
            .find(|(l, k, _)| *l == loc && *k == key)
            .map(|(_, _, s)| *s)
    };
    find(locale).or_else(|| find("en")).unwrap_or_default()
}

pub fn normalize(locale: &str) -> &'static str {
    LOCALES
        .iter()
        .copied()
        .find(|l| *l == locale)
        .unwrap_or("en")
}

pub struct UiLocale {
    current: Mutex<&'static str>,
    tray: Mutex<Option<(MenuItem<Wry>, MenuItem<Wry>)>>,
}

impl Default for UiLocale {
    fn default() -> Self {
        Self {
            current: Mutex::new("en"),
            tray: Mutex::new(None),
        }
    }
}

impl UiLocale {
    pub fn current(&self) -> &'static str {
        *self.current.lock().unwrap()
    }

    pub fn attach_tray(&self, show: MenuItem<Wry>, quit: MenuItem<Wry>) {
        *self.tray.lock().unwrap() = Some((show, quit));
    }

    fn set(&self, locale: &'static str) {
        *self.current.lock().unwrap() = locale;
        if let Some((show, quit)) = self.tray.lock().unwrap().as_ref() {
            let _ = show.set_text(text(locale, Text::ShowApp));
            let _ = quit.set_text(text(locale, Text::Quit));
        }
    }
}

pub fn current(app: &AppHandle) -> &'static str {
    app.state::<UiLocale>().current()
}

#[tauri::command]
pub fn system_locales() -> Vec<String> {
    sys_locale::get_locales().collect()
}

#[tauri::command]
pub fn set_ui_locale(app: AppHandle, locale: String) {
    app.state::<UiLocale>().set(normalize(&locale));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_english_for_missing_entries() {
        assert_eq!(text("de", Text::Quit), text("en", Text::Quit));
        assert_eq!(text("xx", Text::NoSubject), "(no subject)");
    }

    #[test]
    fn every_key_has_english() {
        for key in [Text::ShowApp, Text::Quit, Text::NewMessage, Text::NoSubject] {
            assert!(STRINGS.iter().any(|(l, k, _)| *l == "en" && *k == key));
        }
    }

    #[test]
    fn normalizes_unknown_locales_to_english() {
        assert_eq!(normalize("pt"), "pt");
        assert_eq!(normalize("pt-BR"), "en");
        assert_eq!(normalize(""), "en");
    }
}
