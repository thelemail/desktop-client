use gpui_kit::App;
use thelemail_core::settings::Theme as Stored;
use thelemail_ui::theme::{Theme, ThemeChoice};

use crate::services::Services;

fn choice(stored: Stored) -> ThemeChoice {
    match stored {
        Stored::Light => ThemeChoice::Light,
        Stored::Dark => ThemeChoice::Dark,
        Stored::System => ThemeChoice::System,
    }
}

#[cfg(debug_assertions)]
fn stored(choice: ThemeChoice) -> Stored {
    match choice {
        ThemeChoice::Light => Stored::Light,
        ThemeChoice::Dark => Stored::Dark,
        ThemeChoice::System => Stored::System,
    }
}

pub fn init(cx: &mut App) {
    let saved = cx.global::<Services>().core().settings().theme();
    Theme::apply(choice(saved), cx);
}

#[cfg(debug_assertions)]
pub fn set(next: ThemeChoice, cx: &mut App) {
    Theme::apply(next, cx);
    let services = cx.global::<Services>();
    let core = services.core().clone();
    if core.settings().set_theme(stored(next)) {
        services.runtime().spawn_blocking(move || {
            if let Err(err) = core.settings().save() {
                tracing::warn!("appearance: saving the theme failed: {err}");
            }
        });
    }
}
