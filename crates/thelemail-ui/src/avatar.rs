mod identity;

use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ElementId, FontWeight, Image, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled, StyledImage, Window, div,
    img, px, white,
};

pub use identity::{Choice, PALETTE, Sender, choice, initials, palette_slot, sender};

use crate::motion::{EASE_OUT, FAST, Spec, animate_from};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{FONT_SANS, RADIUS_AVATAR_FRACTION};

pub const SIZE: f32 = 30.0;
const FONT_SHARE: f32 = 0.4;
const LOGO_PAD: f32 = 1.0;
const FADE: Spec = Spec::new(FAST, EASE_OUT);

#[derive(Clone)]
pub enum Picture {
    Photo(Arc<Image>),
    Logo(Arc<Image>),
}

impl Picture {
    fn image(&self) -> &Arc<Image> {
        match self {
            Self::Photo(image) | Self::Logo(image) => image,
        }
    }
}

#[derive(IntoElement)]
pub struct Avatar {
    id: ElementId,
    initials: SharedString,
    slot: usize,
    size: f32,
    font: Option<f32>,
    picture: Option<Picture>,
    label: Option<SharedString>,
    theme: Option<Theme>,
}

impl Avatar {
    pub fn new(id: impl Into<ElementId>, initials: impl Into<SharedString>, slot: usize) -> Self {
        Self {
            id: id.into(),
            initials: initials.into(),
            slot: slot % PALETTE.len(),
            size: SIZE,
            font: None,
            picture: None,
            label: None,
            theme: None,
        }
    }

    pub fn of(id: impl Into<ElementId>, sender: &Sender) -> Self {
        Self::new(id, sender.initials.clone(), sender.slot)
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn font(mut self, font: f32) -> Self {
        self.font = Some(font);
        self
    }

    pub fn picture(mut self, picture: Option<Picture>) -> Self {
        self.picture = picture;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
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

impl RenderOnce for Avatar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let (bg, fg) = PALETTE[self.slot];
        let radius = px(self.size * RADIUS_AVATAR_FRACTION);
        let font = self.font.unwrap_or((self.size * FONT_SHARE).round());
        let layer = self.picture.clone().map(|picture| {
            let shown = animate_from(
                self.child_id(format!("fade-{}", picture.image().id())),
                0.0,
                1.0,
                FADE,
                window,
                cx,
            )
            .value;
            let (fit, backing, pad) = match &picture {
                Picture::Photo(_) => (ObjectFit::Cover, None, 0.0),
                Picture::Logo(_) => (ObjectFit::Contain, Some(white()), LOGO_PAD),
            };
            div()
                .absolute()
                .inset_0()
                .rounded(radius)
                .overflow_hidden()
                .opacity(shown)
                .when_some(backing, |el, bg| el.bg(bg))
                .p(px(pad))
                .child(
                    img(picture.image().clone())
                        .size_full()
                        .rounded(radius)
                        .object_fit(fit)
                        .with_loading(|| div().into_any_element())
                        .with_fallback(|| div().into_any_element()),
                )
        });
        div()
            .id(self.id.clone())
            .when_some(self.label.clone(), |el, label| {
                el.role(Role::Image).aria_label(label)
            })
            .flex_none()
            .relative()
            .size(px(self.size))
            .rounded(radius)
            .overflow_hidden()
            .bg(theme.color(bg))
            .flex()
            .items_center()
            .justify_center()
            .font_family(FONT_SANS.name)
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(font * FONT_SANS.size_adjust))
            .line_height(px(self.size))
            .text_color(theme.color(fg))
            .child(self.initials.clone())
            .children(layer)
    }
}
