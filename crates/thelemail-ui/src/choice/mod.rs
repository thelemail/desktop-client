mod check;
mod chip;
pub mod look;
pub mod roving;
mod segmented;
mod switch;
mod tiles;

use std::rc::Rc;

use gpui_kit::{
    App, ClickEvent, Div, ElementId, Entity, FocusHandle, Hsla, KeyboardButton, Styled, Window,
    div, px,
};

pub use check::{Checkbox, Radio, RadioGroup, RadioItem};
pub use chip::{Chip, ChipRow, ChipSize};
pub use look::Mark;
pub use segmented::{SegmentOption, Segmented, SegmentedSize};
pub use switch::{Switch, SwitchSize};
pub use tiles::{OptionTiles, Tile, columns};

pub(crate) const RING_WIDTH: f32 = 2.0;
pub(crate) const RING_OFFSET: f32 = 2.0;

pub type Handler<T> = Rc<dyn Fn(T, &mut Window, &mut App)>;

pub(crate) fn child(id: &ElementId, name: &'static str) -> ElementId {
    ElementId::NamedChild(id.clone().into(), name.into())
}

pub(crate) fn ring(border: f32, radius: f32, color: Hsla) -> Div {
    let reach = border + RING_OFFSET + RING_WIDTH;
    div()
        .absolute()
        .top(px(-reach))
        .left(px(-reach))
        .right(px(-reach))
        .bottom(px(-reach))
        .rounded(px(radius + RING_OFFSET + RING_WIDTH))
        .border(px(RING_WIDTH))
        .border_color(color)
}

pub(crate) fn hovered(id: &ElementId, window: &mut Window, cx: &mut App) -> Entity<bool> {
    window.use_keyed_state(child(id, "hover"), cx, |_, _| false)
}

pub(crate) fn focus(id: &ElementId, window: &mut Window, cx: &mut App) -> FocusHandle {
    window
        .use_keyed_state(child(id, "focus"), cx, |_, cx| cx.focus_handle())
        .read(cx)
        .clone()
}

pub(crate) fn is_enter(event: &ClickEvent) -> bool {
    matches!(event, ClickEvent::Keyboard(key) if key.button == KeyboardButton::Enter)
}

pub(crate) fn set_hover(state: Entity<bool>) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
    move |over, window, cx| {
        state.update(cx, |hovered, _| *hovered = *over);
        window.refresh();
    }
}
