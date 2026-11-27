use gpui_kit::{
    App, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement, RenderOnce, Role,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px, svg,
};

use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_SANS};

const LOGO_MARK: &str = "brand/logo-mark.svg";
const LOGO_TYPE: &str = "brand/logo-type.svg";
const MARK: &str = "brand/mark.svg";

const FILES: [(&str, &[u8]); 3] = [
    (LOGO_MARK, include_bytes!("../assets/brand/logo-mark.svg")),
    (LOGO_TYPE, include_bytes!("../assets/brand/logo-type.svg")),
    (MARK, include_bytes!("../assets/brand/mark.svg")),
];

const LOGO_ASPECT: f32 = 869.2 / 100.0;
const LOGO_HEIGHT: f32 = 16.0;
const MARK_ASPECT: f32 = 176.0 / 118.0;
const MARK_SIZE: f32 = 32.0;
const NAME: &str = "Thelemail";
const WORDMARK_SIZE: f32 = 13.5;
const WORDMARK_LARGE: f32 = 18.0;
const WORDMARK_WEIGHT: f32 = 680.0;
const WORDMARK_TRACKING: f32 = 0.22;

pub(crate) fn asset(path: &str) -> Option<&'static [u8]> {
    FILES
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}

pub(crate) fn paths() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|(name, _)| *name)
}

#[derive(IntoElement)]
pub struct Logo {
    id: ElementId,
    height: f32,
    theme: Option<Theme>,
}

impl Logo {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            height: LOGO_HEIGHT,
            theme: None,
        }
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl RenderOnce for Logo {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let w = px(self.height * LOGO_ASPECT);
        let h = px(self.height);
        let layer = |path: &'static str, token: ColorToken| {
            svg()
                .path(path)
                .absolute()
                .top_0()
                .left_0()
                .w(w)
                .h(h)
                .text_color(theme.color(token))
        };
        div()
            .id(self.id)
            .role(Role::Image)
            .aria_label(NAME)
            .flex_none()
            .relative()
            .w(w)
            .h(h)
            .child(layer(LOGO_MARK, ColorToken::LogoMark))
            .child(layer(LOGO_TYPE, ColorToken::LogoType))
    }
}

#[derive(IntoElement)]
pub struct Mark {
    id: ElementId,
    size: f32,
    color: ColorToken,
    theme: Option<Theme>,
}

impl Mark {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            size: MARK_SIZE,
            color: ColorToken::FgStrong,
            theme: None,
        }
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: ColorToken) -> Self {
        self.color = color;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl RenderOnce for Mark {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        svg()
            .path(MARK)
            .id(self.id)
            .role(Role::Image)
            .aria_label(NAME)
            .flex_none()
            .w(px(self.size * MARK_ASPECT))
            .h(px(self.size))
            .text_color(theme.color(self.color))
    }
}

#[derive(IntoElement)]
pub struct Wordmark {
    id: ElementId,
    large: bool,
    color: ColorToken,
    theme: Option<Theme>,
}

impl Wordmark {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            large: false,
            color: ColorToken::FgStrong,
            theme: None,
        }
    }

    pub fn large(mut self) -> Self {
        self.large = true;
        self
    }

    pub fn color(mut self, color: ColorToken) -> Self {
        self.color = color;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

pub fn glyphs(text: &str) -> Vec<SharedString> {
    text.to_uppercase()
        .chars()
        .map(|c| SharedString::from(c.to_string()))
        .collect()
}

impl RenderOnce for Wordmark {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let size = if self.large {
            WORDMARK_LARGE
        } else {
            WORDMARK_SIZE
        };
        let track = px(size * WORDMARK_TRACKING);
        div()
            .id(self.id)
            .role(Role::Label)
            .aria_label(NAME)
            .flex_none()
            .flex()
            .gap(track)
            .pr(track)
            .font_family(FONT_SANS.name)
            .text_size(px(size * FONT_SANS.size_adjust))
            .font_weight(FontWeight(WORDMARK_WEIGHT))
            .line_height(px(size))
            .whitespace_nowrap()
            .text_color(theme.color(self.color))
            .children(glyphs(NAME))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_brand_file_is_served() {
        for path in paths() {
            let bytes = asset(path).expect(path);
            assert!(bytes.starts_with(b"<svg"), "{path}");
        }
        assert!(asset("brand/missing.svg").is_none());
    }

    #[test]
    fn the_wordmark_is_set_in_capitals_one_glyph_each() {
        let g = glyphs(NAME);
        assert_eq!(g.len(), 9);
        assert_eq!(g.concat(), "THELEMAIL");
    }
}
