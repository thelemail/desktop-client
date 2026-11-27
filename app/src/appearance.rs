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

pub fn init(cx: &mut App) {
    let saved = cx.global::<Services>().core().settings().theme();
    Theme::apply(choice(saved), cx);
}
