use gpui_kit::{App, Global};
use thelemail_i18n::{Locale, set_locale};

use crate::services::Services;
use crate::{menus, tray};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Language {
    pub choice: Option<Locale>,
    pub active: Locale,
}

impl Global for Language {}

pub fn resolve(choice: Option<Locale>, system: &[String]) -> Locale {
    choice
        .or_else(|| Locale::match_tags(system))
        .unwrap_or(Locale::En)
}

pub fn init(cx: &mut App) {
    let choice = cx.global::<Services>().core().settings().locale();
    apply(choice, cx);
}

#[cfg(debug_assertions)]
pub fn set(choice: Option<Locale>, cx: &mut App) {
    apply(choice, cx);
    let services = cx.global::<Services>();
    let core = services.core().clone();
    if core.settings().set_locale(choice) {
        services.runtime().spawn_blocking(move || {
            if let Err(err) = core.settings().save() {
                tracing::warn!("language: saving the language failed: {err}");
            }
        });
    }
}

fn apply(choice: Option<Locale>, cx: &mut App) {
    let system: Vec<String> = sys_locale::get_locales().collect();
    let next = Language {
        choice,
        active: resolve(choice, &system),
    };
    let previous = cx.try_global::<Language>().copied();
    set_locale(next.active);
    cx.set_global(next);
    if previous.is_some_and(|p| p.active != next.active) {
        menus::refresh(cx);
        tray::relabel(cx);
        cx.refresh_windows();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_choice_then_system_then_english() {
        let system = ["pt-BR".to_string(), "fr-FR".to_string()];
        assert_eq!(resolve(Some(Locale::De), &system), Locale::De);
        assert_eq!(resolve(None, &system), Locale::Fr);
        assert_eq!(resolve(None, &["es-ES".to_string()]), Locale::En);
        assert_eq!(resolve(None, &[]), Locale::En);
    }
}
