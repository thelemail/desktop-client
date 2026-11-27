use gpui_kit::{App, IntoElement, RenderOnce, Styled, Window, div, px};

use crate::theme::{ActiveTokens, Theme};
use crate::tokens::ColorToken;

const HAIRLINE: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(IntoElement)]
pub struct Separator {
    axis: Axis,
    theme: Option<Theme>,
}

impl Separator {
    pub fn horizontal() -> Self {
        Self {
            axis: Axis::Horizontal,
            theme: None,
        }
    }

    pub fn vertical() -> Self {
        Self {
            axis: Axis::Vertical,
            theme: None,
        }
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl RenderOnce for Separator {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let line = div().flex_none().bg(theme.color(ColorToken::BorderSoft));
        match self.axis {
            Axis::Horizontal => line.w_full().h(px(HAIRLINE)),
            Axis::Vertical => line.h_full().w(px(HAIRLINE)),
        }
    }
}
