use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled,
    Window, div, px, relative,
};

use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_SANS};

const GAP: f32 = 7.0;
const ROW_GAP: f32 = 8.0;
const FONT: f32 = 14.0;

#[derive(IntoElement)]
pub struct FieldLabel {
    text: SharedString,
    aux: Option<AnyElement>,
    control: Option<AnyElement>,
    theme: Option<Theme>,
}

impl FieldLabel {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            aux: None,
            control: None,
            theme: None,
        }
    }

    pub fn aux(mut self, aux: impl IntoElement) -> Self {
        self.aux = Some(aux.into_any_element());
        self
    }

    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl RenderOnce for FieldLabel {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        div()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(ROW_GAP))
                    .font_family(FONT_SANS.name)
                    .text_size(px(FONT * FONT_SANS.size_adjust))
                    .line_height(relative(1.3))
                    .child(
                        div()
                            .whitespace_nowrap()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.color(ColorToken::Fg))
                            .child(self.text),
                    )
                    .when_some(self.aux, |row, aux| row.child(div().ml_auto().child(aux))),
            )
            .children(self.control)
    }
}
