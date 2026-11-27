mod dialog;
mod focus;
pub mod layer;

use gpui_kit::{App, KeyBinding, actions};

pub use dialog::{Dialog, DialogSize, NARROW, Presentation, Sheet, Tone};
pub use layer::{Layer, Z_TRANSITION, covered, native_surface};

actions!(thelemail_overlay, [Dismiss]);

pub(crate) const MODAL: &str = "ThelemailModal";

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Dismiss, Some(MODAL))]);
}
