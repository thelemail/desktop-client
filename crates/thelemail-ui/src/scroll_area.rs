use gpui_kit::base::{Scrollbar, ScrollbarAxis, ScrollbarMode, ScrollbarMotion, ScrollbarStyles};
use gpui_kit::{
    AnyElement, App, Div, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, ScrollHandle, StatefulInteractiveElement, StyleRefinement, Styled, Window, div, px,
};

use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, RADIUS_PILL};

pub const TRACK: f32 = 11.0;
pub const INSET: f32 = 3.0;
const THUMB: f32 = TRACK - INSET * 2.0;
const THUMB_MIN: f32 = 24.0;

pub fn mode() -> ScrollbarMode {
    if cfg!(target_os = "macos") {
        ScrollbarMode::Scrolling
    } else {
        ScrollbarMode::Hover
    }
}

pub fn motion() -> ScrollbarMotion {
    ScrollbarMotion::default()
}

pub fn styles(theme: &Theme) -> ScrollbarStyles {
    let rest = theme.color(ColorToken::BorderStrong);
    let hover = theme.color(ColorToken::BorderControl);
    let clear = Hsla::transparent_black();
    let thumb = |bg: Hsla| {
        move |style: gpui_kit::base::ScrollbarThumbStyle| {
            style
                .bg(bg)
                .width(px(THUMB))
                .inset(px(INSET))
                .radius(px(RADIUS_PILL))
                .min_length(px(THUMB_MIN))
        }
    };
    ScrollbarStyles::default()
        .track(|style| style.bg(clear).border_color(clear).width(px(TRACK)))
        .track_hover(|style| style.bg(clear).border_color(clear).width(px(TRACK)))
        .track_active(|style| style.bg(clear).border_color(clear).width(px(TRACK)))
        .thumb(thumb(rest))
        .thumb_hover(thumb(hover))
        .thumb_active(thumb(hover))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
    Both,
}

impl From<Axis> for ScrollbarAxis {
    fn from(axis: Axis) -> Self {
        match axis {
            Axis::Vertical => ScrollbarAxis::Vertical,
            Axis::Horizontal => ScrollbarAxis::Horizontal,
            Axis::Both => ScrollbarAxis::Both,
        }
    }
}

#[derive(IntoElement)]
pub struct ScrollArea {
    id: ElementId,
    axis: Axis,
    outer: Div,
    content: Div,
    handle: Option<ScrollHandle>,
    theme: Option<Theme>,
}

impl ScrollArea {
    pub fn new(id: impl Into<ElementId>, axis: Axis) -> Self {
        Self {
            id: id.into(),
            axis,
            outer: div(),
            content: div(),
            handle: None,
            theme: None,
        }
    }

    pub fn vertical(id: impl Into<ElementId>) -> Self {
        Self::new(id, Axis::Vertical)
    }

    pub fn horizontal(id: impl Into<ElementId>) -> Self {
        Self::new(id, Axis::Horizontal)
    }

    pub fn track(mut self, handle: &ScrollHandle) -> Self {
        self.handle = Some(handle.clone());
        self
    }

    pub fn content(mut self, style: impl FnOnce(Div) -> Div) -> Self {
        self.content = style(self.content);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    fn child_id(&self, name: &'static str) -> ElementId {
        ElementId::NamedChild(self.id.clone().into(), name.into())
    }
}

impl Styled for ScrollArea {
    fn style(&mut self) -> &mut StyleRefinement {
        self.outer.style()
    }
}

impl ParentElement for ScrollArea {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.content.extend(elements);
    }
}

impl RenderOnce for ScrollArea {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let handle = match &self.handle {
            Some(handle) => handle.clone(),
            None => window
                .use_keyed_state(self.child_id("handle"), cx, |_, _| ScrollHandle::new())
                .read(cx)
                .clone(),
        };
        let viewport_id = self.child_id("viewport");
        let bar_id = self.child_id("bar");
        let viewport = self
            .content
            .id(viewport_id)
            .flex_shrink(1.0)
            .min_h_0()
            .min_w_0()
            .track_scroll(&handle);
        let viewport = match self.axis {
            Axis::Vertical => viewport.w_full().overflow_y_scroll(),
            Axis::Horizontal => viewport.h_full().overflow_x_scroll(),
            Axis::Both => viewport.overflow_scroll(),
        };
        let outer = match self.axis {
            Axis::Horizontal => self.outer.flex().flex_row(),
            Axis::Vertical | Axis::Both => self.outer.flex().flex_col(),
        };
        let bar = Scrollbar::new(&handle)
            .id(bar_id)
            .axis(self.axis)
            .mode(mode())
            .styles(|_| styles(&theme))
            .viewport_from_layout();
        outer
            .id(self.id)
            .relative()
            .overflow_hidden()
            .child(viewport)
            .child(div().absolute().inset_0().child(bar))
    }
}
