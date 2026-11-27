mod anchor;
mod dialog;
mod focus;
pub mod layer;
mod menu;
mod popover;
mod tooltip;

use gpui_kit::{App, KeyBinding, actions};

pub use dialog::{Dialog, DialogSize, NARROW, Presentation, Sheet, Tone};
pub use layer::{Layer, Z_TRANSITION, covered, native_surface};
pub use menu::{Check, ContextMenu, DropdownMenu, MenuEntry, MenuItem};
pub use popover::Popover;
pub(crate) use tooltip::attach as tooltip_attach;
pub use tooltip::{TOOLTIP_DELAY, TOOLTIP_SKIP, Tip, Tips, Tooltipped};

actions!(
    thelemail_overlay,
    [
        Dismiss,
        MenuNext,
        MenuPrev,
        MenuFirst,
        MenuLast,
        MenuRight,
        MenuLeft,
        MenuActivate,
        MenuBack,
        MenuExit,
        OpenContextMenu
    ]
);

pub(crate) const MODAL: &str = "ThelemailModal";
pub(crate) const MENU_CONTEXT: &str = "ThelemailMenu";
pub(crate) const CONTEXT_TARGET: &str = "ThelemailContextTarget";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, Some(MODAL)),
        KeyBinding::new("down", MenuNext, Some(MENU_CONTEXT)),
        KeyBinding::new("up", MenuPrev, Some(MENU_CONTEXT)),
        KeyBinding::new("home", MenuFirst, Some(MENU_CONTEXT)),
        KeyBinding::new("end", MenuLast, Some(MENU_CONTEXT)),
        KeyBinding::new("pageup", MenuFirst, Some(MENU_CONTEXT)),
        KeyBinding::new("pagedown", MenuLast, Some(MENU_CONTEXT)),
        KeyBinding::new("right", MenuRight, Some(MENU_CONTEXT)),
        KeyBinding::new("left", MenuLeft, Some(MENU_CONTEXT)),
        KeyBinding::new("enter", MenuActivate, Some(MENU_CONTEXT)),
        KeyBinding::new("space", MenuActivate, Some(MENU_CONTEXT)),
        KeyBinding::new("escape", MenuBack, Some(MENU_CONTEXT)),
        KeyBinding::new("tab", MenuExit, Some(MENU_CONTEXT)),
        KeyBinding::new("shift-tab", MenuExit, Some(MENU_CONTEXT)),
        KeyBinding::new("shift-f10", OpenContextMenu, Some(CONTEXT_TARGET)),
        KeyBinding::new("menu", OpenContextMenu, Some(CONTEXT_TARGET)),
    ]);
    tooltip::register(cx);
}
