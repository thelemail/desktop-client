mod label;
mod password;
mod select;

use gpui_kit::component::input::{Input, InputContentType, InputState, Textarea, TextareaState};
use gpui_kit::component::{Sizable as _, Size as KitSize};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Corners, Div, ElementId, Entity, Focusable as _, Hsla, InteractiveElement,
    IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, px, relative,
};

use crate::motion::{BASE, EASE, Spec, ease_color};
use crate::shape::CornerRadii as _;
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{
    CONTROL_LG, CONTROL_MD, CONTROL_SM, ColorToken, ControlHeight, FONT_MONO, FONT_SANS,
};

pub use label::FieldLabel;
pub use password::PasswordField;
pub use select::{Select, SelectChanged, SelectOption};

const BORDER: f32 = 1.0;
const RING_WIDTH: f32 = 2.0;
const RING_OFFSET: f32 = 1.0;
const LINE_HEIGHT: f32 = 1.3;
const TEXTAREA_LINE_HEIGHT: f32 = 1.5;
const TEXTAREA_MIN: f32 = 1.6;
const SUFFIX_PAD: f32 = 14.0;
const SUFFIX_FONT: f32 = 14.0;
const SELECT_PAD_END: f32 = 34.0;
const COLOR: Spec = Spec::new(BASE, EASE);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FieldSize {
    Sm,
    #[default]
    Md,
    Lg,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldMetrics {
    pub control: ControlHeight,
    pub pad_x: f32,
    pub font: f32,
}

impl FieldSize {
    pub const ALL: [Self; 3] = [Self::Sm, Self::Md, Self::Lg];

    pub const fn metrics(self) -> FieldMetrics {
        match self {
            Self::Sm => FieldMetrics {
                control: CONTROL_SM,
                pad_x: 10.0,
                font: 13.0,
            },
            Self::Md => FieldMetrics {
                control: CONTROL_MD,
                pad_x: 12.0,
                font: 14.0,
            },
            Self::Lg => FieldMetrics {
                control: CONTROL_LG,
                pad_x: 14.0,
                font: 15.0,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldFlags {
    pub focused: bool,
    pub invalid: bool,
    pub disabled: bool,
    pub read_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLook {
    pub border: ColorToken,
    pub bg: ColorToken,
    pub fg: ColorToken,
    pub ring: Option<ColorToken>,
}

pub fn frame_look(flags: FieldFlags) -> FrameLook {
    if flags.disabled {
        return FrameLook {
            border: ColorToken::BorderControl,
            bg: ColorToken::BgSunken,
            fg: ColorToken::FgFaint,
            ring: None,
        };
    }
    let bg = if flags.read_only {
        ColorToken::BgRaised
    } else {
        ColorToken::Surface
    };
    let (border, ring) = match (flags.invalid, flags.focused) {
        (true, true) => (ColorToken::Danger500, Some(ColorToken::Danger500)),
        (true, false) => (ColorToken::Danger500, None),
        (false, true) => (ColorToken::Primary, Some(ColorToken::FocusRing)),
        (false, false) => (ColorToken::BorderControl, None),
    };
    FrameLook {
        border,
        bg,
        fg: ColorToken::FgStrong,
        ring,
    }
}

pub(crate) struct Frame {
    pub id: ElementId,
    pub radius: f32,
    pub height: Option<f32>,
    pub look: FrameLook,
    pub theme: Theme,
}

impl Frame {
    fn ink(&self, name: &'static str, color: Hsla, window: &mut Window, cx: &mut App) -> Hsla {
        ease_color(
            ElementId::NamedChild(self.id.clone().into(), name.into()),
            color,
            COLOR,
            window,
            cx,
        )
    }

    pub fn render(self, window: &mut Window, cx: &mut App) -> Div {
        let border = self.ink("border", self.theme.color(self.look.border), window, cx);
        let bg = self.ink("bg", self.theme.color(self.look.bg), window, cx);
        let ring_target = match self.look.ring {
            Some(token) => self.theme.color(token),
            None => self.theme.color(self.look.border).alpha(0.0),
        };
        let ring = self.ink("ring", ring_target, window, cx);
        let radius = px(self.radius);
        let reach = BORDER + RING_OFFSET + RING_WIDTH;
        div()
            .relative()
            .flex()
            .items_center()
            .w_full()
            .when_some(self.height, |el, height| el.h(px(height)))
            .rounded(radius)
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_color(self.theme.color(self.look.fg))
            .child(
                div()
                    .absolute()
                    .top(px(-reach))
                    .left(px(-reach))
                    .right(px(-reach))
                    .bottom(px(-reach))
                    .rounded(radius + px(RING_OFFSET + RING_WIDTH))
                    .border(px(RING_WIDTH))
                    .border_color(ring),
            )
    }
}

#[derive(IntoElement)]
pub struct TextField {
    id: ElementId,
    state: Entity<InputState>,
    size: FieldSize,
    invalid: bool,
    disabled: bool,
    read_only: bool,
    mono: bool,
    content_type: Option<InputContentType>,
    label: Option<SharedString>,
    suffix: Option<SharedString>,
    trailing: Option<AnyElement>,
    trailing_room: f32,
    theme: Option<Theme>,
}

impl TextField {
    pub fn new(id: impl Into<ElementId>, state: &Entity<InputState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            size: FieldSize::default(),
            invalid: false,
            disabled: false,
            read_only: false,
            mono: false,
            content_type: None,
            label: None,
            suffix: None,
            trailing: None,
            trailing_room: 0.0,
            theme: None,
        }
    }

    pub fn size(mut self, size: FieldSize) -> Self {
        self.size = size;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    pub fn content_type(mut self, content_type: InputContentType) -> Self {
        self.content_type = Some(content_type);
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn suffix(mut self, suffix: impl Into<SharedString>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    pub(crate) fn trailing(mut self, element: impl IntoElement, room: f32) -> Self {
        self.trailing = Some(element.into_any_element());
        self.trailing_room = room;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl RenderOnce for TextField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let metrics = self.size.metrics();
        let focused = self.state.read(cx).focus_handle(cx).is_focused(window);
        let look = frame_look(FieldFlags {
            focused: focused && !self.disabled,
            invalid: self.invalid,
            disabled: self.disabled,
            read_only: self.read_only,
        });
        let family = if self.mono { &FONT_MONO } else { &FONT_SANS };
        let radius = metrics.control.radius;
        let inner_radius = (radius - BORDER).max(0.0);
        let input = Input::new(&self.state)
            .id(ElementId::NamedChild(
                self.id.clone().into(),
                "input".into(),
            ))
            .appearance(false)
            .disabled(self.disabled)
            .readonly(self.read_only)
            .when_some(self.label.clone(), |input, label| input.aria_label(label))
            .when_some(self.content_type, |input, kind| input.content_type(kind))
            .flex_1()
            .min_w_0()
            .h_full()
            .pl(px(metrics.pad_x))
            .pr(px(metrics.pad_x + self.trailing_room))
            .py_0()
            .font_family(family.name)
            .text_size(px(metrics.font * family.size_adjust))
            .line_height(relative(LINE_HEIGHT));
        Frame {
            id: self.id.clone(),
            radius,
            height: Some(metrics.control.height),
            look,
            theme,
        }
        .render(window, cx)
        .id(self.id)
        .child(input)
        .when_some(self.trailing, |frame, trailing| {
            frame.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(4.0))
                    .flex()
                    .items_center()
                    .child(trailing),
            )
        })
        .when_some(self.suffix, |frame, suffix| {
            frame.child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h_full()
                    .px(px(SUFFIX_PAD))
                    .border_l_1()
                    .border_color(theme.color(ColorToken::BorderSoft))
                    .bg(theme.color(ColorToken::BgSunken))
                    .corners(Corners {
                        top_left: px(0.0),
                        bottom_left: px(0.0),
                        top_right: px(inner_radius),
                        bottom_right: px(inner_radius),
                    })
                    .font_family(FONT_MONO.name)
                    .text_size(px(SUFFIX_FONT * FONT_MONO.size_adjust))
                    .text_color(theme.color(ColorToken::FgMuted))
                    .whitespace_nowrap()
                    .child(suffix),
            )
        })
    }
}

#[derive(IntoElement)]
pub struct TextArea {
    id: ElementId,
    state: Entity<TextareaState>,
    size: FieldSize,
    invalid: bool,
    disabled: bool,
    read_only: bool,
    label: Option<SharedString>,
    theme: Option<Theme>,
}

impl TextArea {
    pub fn new(id: impl Into<ElementId>, state: &Entity<TextareaState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            size: FieldSize::default(),
            invalid: false,
            disabled: false,
            read_only: false,
            label: None,
            theme: None,
        }
    }

    pub fn size(mut self, size: FieldSize) -> Self {
        self.size = size;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
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
}

impl RenderOnce for TextArea {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let metrics = self.size.metrics();
        let focused = self.state.read(cx).focus_handle(cx).is_focused(window);
        let look = frame_look(FieldFlags {
            focused: focused && !self.disabled,
            invalid: self.invalid,
            disabled: self.disabled,
            read_only: self.read_only,
        });
        let kit_size = match self.size {
            FieldSize::Sm => KitSize::Medium,
            FieldSize::Md | FieldSize::Lg => KitSize::Large,
        };
        let area = Textarea::new(&self.state)
            .appearance(false)
            .with_size(kit_size)
            .disabled(self.disabled)
            .readonly(self.read_only)
            .when_some(self.label.clone(), |area, label| area.aria_label(label))
            .w_full()
            .font_family(FONT_SANS.name)
            .text_size(px(metrics.font * FONT_SANS.size_adjust))
            .line_height(relative(TEXTAREA_LINE_HEIGHT));
        Frame {
            id: self.id.clone(),
            radius: metrics.control.radius,
            height: None,
            look,
            theme,
        }
        .render(window, cx)
        .id(self.id)
        .items_start()
        .min_h(px(metrics.control.height * TEXTAREA_MIN))
        .child(area)
    }
}

pub(crate) fn select_padding_end() -> f32 {
    SELECT_PAD_END
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_wins_and_never_shows_a_ring() {
        let look = frame_look(FieldFlags {
            focused: true,
            invalid: true,
            disabled: true,
            read_only: true,
        });
        assert_eq!(look.bg, ColorToken::BgSunken);
        assert_eq!(look.fg, ColorToken::FgFaint);
        assert_eq!(look.ring, None);
    }

    #[test]
    fn invalid_keeps_its_colour_through_focus() {
        let rest = frame_look(FieldFlags {
            invalid: true,
            ..FieldFlags::default()
        });
        let focused = frame_look(FieldFlags {
            invalid: true,
            focused: true,
            ..FieldFlags::default()
        });
        assert_eq!(rest.border, ColorToken::Danger500);
        assert_eq!(focused.border, ColorToken::Danger500);
        assert_eq!(focused.ring, Some(ColorToken::Danger500));
    }

    #[test]
    fn focus_draws_the_pine_ring_on_a_primary_border() {
        let look = frame_look(FieldFlags {
            focused: true,
            ..FieldFlags::default()
        });
        assert_eq!(look.border, ColorToken::Primary);
        assert_eq!(look.ring, Some(ColorToken::FocusRing));
    }

    #[test]
    fn read_only_only_changes_the_fill() {
        let look = frame_look(FieldFlags {
            read_only: true,
            ..FieldFlags::default()
        });
        assert_eq!(look.bg, ColorToken::BgRaised);
        assert_eq!(look.border, ColorToken::BorderControl);
    }

    #[test]
    fn radius_follows_height() {
        for size in FieldSize::ALL {
            let control = size.metrics().control;
            assert!(
                (control.radius - control.height * 0.22).abs() < 0.25,
                "{size:?}"
            );
        }
    }
}
