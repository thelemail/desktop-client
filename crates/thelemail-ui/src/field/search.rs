use gpui_kit::component::input::InputState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ElementId, Entity, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window,
    div, px, relative,
};

use super::{FieldSize, TextField};
use crate::icons::{IconName, glyph};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_MONO};

const ICON: f32 = 15.0;
const ICON_ROOM: f32 = 36.0;
const HINT_ROOM: f32 = 40.0;
const HINT_INSET: f32 = 10.0;
const HINT_PAD_X: f32 = 5.0;
const HINT_PAD_Y: f32 = 3.0;
const HINT_RADIUS: f32 = 4.0;
const HINT_FONT: f32 = 11.0;

#[derive(IntoElement)]
pub struct SearchField {
    id: ElementId,
    state: Entity<InputState>,
    size: FieldSize,
    shortcut: Option<SharedString>,
    label: Option<SharedString>,
    disabled: bool,
    theme: Option<Theme>,
}

impl SearchField {
    pub fn new(id: impl Into<ElementId>, state: &Entity<InputState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            size: FieldSize::default(),
            shortcut: None,
            label: None,
            disabled: false,
            theme: None,
        }
    }

    pub fn size(mut self, size: FieldSize) -> Self {
        self.size = size;
        self
    }

    pub fn shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
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
}

impl RenderOnce for SearchField {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        self.state
            .update(cx, |state, _| state.set_clean_on_escape(true));
        let pad_x = self.size.metrics().pad_x;
        let icon = glyph(
            IconName::MagnifyingGlass,
            px(ICON),
            theme.color(ColorToken::FgMuted),
        );
        let hint = self.shortcut.map(|keys| {
            div()
                .mr(px(HINT_INSET - 4.0))
                .px(px(HINT_PAD_X))
                .py(px(HINT_PAD_Y))
                .rounded(px(HINT_RADIUS))
                .border_1()
                .border_color(theme.color(ColorToken::BorderSoft))
                .bg(theme.color(ColorToken::Surface))
                .text_color(theme.color(ColorToken::FgMuted))
                .font_family(FONT_MONO.name)
                .text_size(px(HINT_FONT * FONT_MONO.size_adjust))
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .line_height(relative(1.0))
                .whitespace_nowrap()
                .child(keys)
        });
        TextField::new(self.id, &self.state)
            .size(self.size)
            .disabled(self.disabled)
            .search()
            .theme(theme)
            .when_some(self.label, |field, label| field.label(label))
            .leading(icon, ICON_ROOM - pad_x)
            .when_some(hint, |field, hint| field.trailing(hint, HINT_ROOM - pad_x))
    }
}
