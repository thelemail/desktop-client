mod gesture;
mod state;

use std::borrow::Cow;

use gpui_kit::{
    App, AssetSource, ElementId, Hsla, InteractiveElement, IntoElement, Pixels, RenderOnce, Result,
    Role, SharedString, StatefulInteractiveElement, Styled, Svg, Transformation, Window, px, svg,
};

use crate::theme::ActiveTokens;
use crate::tokens::ColorToken;

pub use gesture::{GESTURE_DURATION, Gesture, Pose};
pub use state::StateIcon;

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconStyle {
    Regular,
    Solid,
    Brands,
}

pub const SIZE_MIN: f32 = 12.0;
pub const SIZE_MAX: f32 = 24.0;
pub const SIZE_DEFAULT: f32 = 24.0;

#[derive(Debug, Clone, Copy, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(IconName::from_path(path)
            .map(|icon| icon.svg())
            .or_else(|| crate::brand::asset(path))
            .map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(IconName::ALL
            .iter()
            .map(|icon| icon.path())
            .chain(crate::brand::paths())
            .filter(|p| p.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tint {
    Token(ColorToken),
    Exact(Hsla),
}

impl Tint {
    pub fn resolve(self, cx: &App) -> Hsla {
        match self {
            Self::Token(token) => cx.tokens().color(token),
            Self::Exact(color) => color,
        }
    }
}

impl From<ColorToken> for Tint {
    fn from(token: ColorToken) -> Self {
        Self::Token(token)
    }
}

impl From<Hsla> for Tint {
    fn from(color: Hsla) -> Self {
        Self::Exact(color)
    }
}

pub(crate) fn checked_size(size: f32) -> Pixels {
    debug_assert!(
        (SIZE_MIN..=SIZE_MAX).contains(&size),
        "icon size {size} is outside {SIZE_MIN}–{SIZE_MAX}"
    );
    px(size.clamp(SIZE_MIN, SIZE_MAX))
}

pub(crate) fn glyph(icon: IconName, size: Pixels, color: Hsla) -> Svg {
    svg()
        .path(icon.path())
        .flex_none()
        .size(size)
        .text_color(color)
}

#[derive(IntoElement)]
pub struct Icon {
    icon: IconName,
    label: Option<(ElementId, SharedString)>,
    size: f32,
    tint: Tint,
    pose: Option<Transformation>,
}

impl Icon {
    pub fn hidden(icon: IconName) -> Self {
        Self {
            icon,
            label: None,
            size: SIZE_DEFAULT,
            tint: Tint::Token(ColorToken::Fg),
            pose: None,
        }
    }

    pub fn labelled(
        id: impl Into<ElementId>,
        icon: IconName,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            label: Some((id.into(), label.into())),
            ..Self::hidden(icon)
        }
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, tint: impl Into<Tint>) -> Self {
        self.tint = tint.into();
        self
    }

    pub fn pose(mut self, pose: Transformation) -> Self {
        self.pose = Some(pose);
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut el = glyph(self.icon, checked_size(self.size), self.tint.resolve(cx));
        if let Some(pose) = self.pose {
            el = el.with_transformation(pose);
        }
        match self.label {
            Some((id, label)) => el
                .id(id)
                .role(Role::Image)
                .aria_label(label)
                .into_any_element(),
            None => el.into_any_element(),
        }
    }
}
