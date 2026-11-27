use gpui_kit::base::input::{InputEvent, InputState};
use gpui_kit::component::input::{Input, InputContentType};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, ElementId, Entity, EventEmitter, Focusable, InteractiveElement,
    IntoElement, ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window, div, px,
};

use crate::motion::{EASE, FAST, Spec, ease_color};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_MONO};

pub const CODE_LENGTH: usize = 6;
const CELL_H: f32 = 56.0;
const CELL_RADIUS: f32 = 10.5;
const CELL_GAP: f32 = 8.0;
const CELL_FONT: f32 = 22.0;
const BORDER: f32 = 1.0;
const BORDER_ACTIVE: f32 = 1.5;
const INPUT_FONT: f32 = 16.0;
const TONE: Spec = Spec::new(FAST, EASE);

pub fn accept(raw: &str) -> String {
    raw.chars()
        .filter_map(|c| {
            c.to_digit(10).map(|_| c).or_else(|| {
                let offset = (c as u32).checked_sub('０' as u32)?;
                char::from_digit(offset, 10)
            })
        })
        .take(CODE_LENGTH)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeEvent {
    Edit,
    Enter,
}

pub struct CodeState {
    input: Entity<InputState>,
    _subscription: Subscription,
}

impl EventEmitter<CodeEvent> for CodeState {}

impl CodeState {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        let subscription =
            cx.subscribe_in(
                &input,
                window,
                |this, input, event, window, cx| match event {
                    InputEvent::Change => {
                        let raw = input.read(cx).value();
                        let clean = accept(&raw);
                        if clean != raw.as_ref() {
                            this.input
                                .update(cx, |state, cx| state.set_value(clean, window, cx));
                        }
                        cx.emit(CodeEvent::Edit);
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => cx.emit(CodeEvent::Enter),
                    InputEvent::Focus | InputEvent::Blur => cx.notify(),
                },
            );
        Self {
            input,
            _subscription: subscription,
        }
    }

    pub fn value(&self, cx: &App) -> SharedString {
        self.input.read(cx).value()
    }

    pub fn set_value(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let clean = accept(value);
        self.input
            .update(cx, |state, cx| state.set_value(clean, window, cx));
        cx.notify();
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_value("", window, cx);
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |state, cx| state.focus(window, cx));
    }

    fn focused(&self, window: &Window, cx: &App) -> bool {
        self.input.focus_handle(cx).is_focused(window)
    }
}

#[derive(IntoElement)]
pub struct CodeCells {
    id: ElementId,
    state: Entity<CodeState>,
    label: SharedString,
    invalid: bool,
    disabled: bool,
    theme: Option<Theme>,
}

impl CodeCells {
    pub fn new(
        id: impl Into<ElementId>,
        state: &Entity<CodeState>,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            label: label.into(),
            invalid: false,
            disabled: false,
            theme: None,
        }
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    fn child_id(&self, name: impl Into<SharedString>) -> ElementId {
        ElementId::NamedChild(self.id.clone().into(), name.into())
    }
}

pub(crate) fn active_cell(len: usize) -> usize {
    len.min(CODE_LENGTH - 1)
}

impl RenderOnce for CodeCells {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let state = self.state.read(cx);
        let value: Vec<char> = state.value(cx).chars().collect();
        let focused = state.focused(window, cx) && !self.disabled;
        let input = state.input.clone();
        let active = active_cell(value.len());
        let cells: Vec<_> = (0..CODE_LENGTH)
            .map(|i| {
                let on = focused && i == active;
                let (bg, border) = if self.invalid {
                    (ColorToken::Danger100, ColorToken::Danger500)
                } else if on {
                    (ColorToken::Surface, ColorToken::Primary)
                } else {
                    (ColorToken::Surface, ColorToken::BorderControl)
                };
                let border = ease_color(
                    self.child_id(format!("edge-{i}")),
                    theme.color(border),
                    TONE,
                    window,
                    cx,
                );
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(CELL_H))
                    .rounded(px(CELL_RADIUS))
                    .border(px(if on { BORDER_ACTIVE } else { BORDER }))
                    .border_color(border)
                    .bg(theme.color(bg))
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_family(FONT_MONO.name)
                    .text_size(px(CELL_FONT * FONT_MONO.size_adjust))
                    .text_color(theme.color(ColorToken::FgStrong))
                    .children(value.get(i).map(|c| SharedString::from(c.to_string())))
            })
            .collect();
        let field = Input::new(&input)
            .id(self.child_id("input"))
            .appearance(false)
            .disabled(self.disabled)
            .aria_label(self.label.clone())
            .content_type(InputContentType::OneTimeCode)
            .absolute()
            .inset_0()
            .opacity(0.0)
            .text_size(px(INPUT_FONT));
        let focus = self.state.clone();
        div()
            .id(self.id.clone())
            .relative()
            .w_full()
            .flex()
            .gap(px(CELL_GAP))
            .when(!self.disabled, |el| {
                el.on_click(move |_, window, cx| {
                    let input = focus.read(cx).input.clone();
                    input.update(cx, |state, cx| state.focus(window, cx));
                })
            })
            .children(cells)
            .child(field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_digits_are_kept() {
        assert_eq!(accept("12a3-4"), "1234");
        assert_eq!(accept("123 456"), "123456");
    }

    #[test]
    fn a_long_paste_keeps_the_first_six() {
        assert_eq!(accept("1234567890"), "123456");
    }

    #[test]
    fn full_width_digits_become_ascii() {
        assert_eq!(accept("１２３"), "123");
    }

    #[test]
    fn the_highlight_stays_on_the_last_cell_when_full() {
        assert_eq!(active_cell(0), 0);
        assert_eq!(active_cell(3), 3);
        assert_eq!(active_cell(6), 5);
    }
}
