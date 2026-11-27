use gpui_kit::{FontWeight, SharedString, Styled, px};

use crate::theme::Theme;
use crate::tokens::{Family, TextStyleToken};

pub trait TextTokens: Styled + Sized {
    fn family(self, family: &Family) -> Self {
        self.font_family(family.name)
    }

    fn text_token(self, style: &TextStyleToken, theme: &Theme) -> Self {
        self.family(&style.family)
            .text_size(px(style.size * style.family.size_adjust))
            .line_height(px(style.line_height))
            .font_weight(FontWeight(style.weight))
            .text_color(theme.color(style.color))
    }
}

impl<E: Styled> TextTokens for E {}

pub fn cased(style: &TextStyleToken, text: &str) -> SharedString {
    if style.uppercase {
        text.to_uppercase().into()
    } else {
        SharedString::from(text.to_string())
    }
}
