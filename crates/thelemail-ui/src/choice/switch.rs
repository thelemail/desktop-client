use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, Role, SharedString,
    StatefulInteractiveElement, Styled, Toggled, Window, div, px, relative,
};

use super::look::{self, DISABLED_OPACITY};
use super::{Handler, child, focus, ring};
use crate::a11y::A11yStateExt as _;
use crate::motion::{BASE, EASE, EASE_OUT, Spec, animate, ease_color};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_SANS};

const BORDER: f32 = 1.5;
const INSET: f32 = 3.0;
const GROWN: f32 = 1.2;
const REACH_Y: f32 = 10.0;
const REACH_X: f32 = 2.0;
const LABEL_GAP: f32 = 8.0;
const LABEL_FONT: f32 = 14.0;
const COLOR: Spec = Spec::new(BASE, EASE);
const SLIDE: Spec = Spec::new(BASE, EASE_OUT);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SwitchSize {
    Sm,
    #[default]
    Md,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwitchMetrics {
    pub width: f32,
    pub height: f32,
    pub thumb: f32,
}

impl SwitchSize {
    pub const fn metrics(self) -> SwitchMetrics {
        match self {
            Self::Sm => SwitchMetrics {
                width: 32.0,
                height: 20.0,
                thumb: 12.0,
            },
            Self::Md => SwitchMetrics {
                width: 40.0,
                height: 24.0,
                thumb: 15.0,
            },
        }
    }
}

impl SwitchMetrics {
    pub fn travel(self) -> f32 {
        self.width - self.thumb - 3.0 * INSET
    }

    pub fn thumb_at(self, on: f32) -> (f32, f32, f32) {
        let size = self.thumb * (1.0 + (GROWN - 1.0) * on);
        let inner = self.height - 2.0 * BORDER;
        let left = INSET + self.travel() * on - (size - self.thumb) / 2.0;
        let top = (inner - size) / 2.0;
        (left, top, size)
    }
}

#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    checked: bool,
    size: SwitchSize,
    label: Option<SharedString>,
    accessibility_label: Option<SharedString>,
    disabled: bool,
    theme: Option<Theme>,
    on_change: Option<Handler<bool>>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            checked: false,
            size: SwitchSize::default(),
            label: None,
            accessibility_label: None,
            disabled: false,
            theme: None,
            on_change: None,
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn size(mut self, size: SwitchSize) -> Self {
        self.size = size;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
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

    pub fn on_change(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let metrics = self.size.metrics();
        let paint = look::switch(self.checked);
        let bg = ease_color(
            child(&self.id, "bg"),
            paint.bg.resolve(&theme),
            COLOR,
            window,
            cx,
        );
        let border = ease_color(
            child(&self.id, "border"),
            paint.border.resolve(&theme),
            COLOR,
            window,
            cx,
        );
        let knob = ease_color(
            child(&self.id, "thumb"),
            paint.fg.resolve(&theme),
            COLOR,
            window,
            cx,
        );
        let on = animate(
            child(&self.id, "slide"),
            if self.checked { 1.0 } else { 0.0 },
            SLIDE,
            window,
            cx,
        )
        .value;
        let (left, top, size) = metrics.thumb_at(on);
        let handle = focus(&self.id, window, cx);
        let focused = handle.is_focused(window);
        let keyboard = focused && window.last_input_was_keyboard();
        let ring_color = theme.color(ColorToken::FocusRing);

        let track = div()
            .relative()
            .flex_none()
            .w(px(metrics.width))
            .h(px(metrics.height))
            .rounded_full()
            .border(px(BORDER))
            .border_color(border)
            .bg(bg)
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .size(px(size))
                    .rounded_full()
                    .bg(knob),
            )
            .when(keyboard, |el| {
                el.child(ring(BORDER, metrics.height / 2.0, ring_color))
            });

        let disabled = self.disabled;
        let checked = self.checked;
        let on_change = self.on_change.clone();
        let name = self.accessibility_label.or_else(|| self.label.clone());
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .map(|el| match self.label.clone() {
                Some(label) => el.gap(px(LABEL_GAP)).child(track).child(
                    div()
                        .font_family(FONT_SANS.name)
                        .text_size(px(LABEL_FONT * FONT_SANS.size_adjust))
                        .line_height(relative(1.3))
                        .text_color(theme.color(ColorToken::Fg))
                        .child(label),
                ),
                None => el
                    .my(px(-REACH_Y))
                    .mx(px(-REACH_X))
                    .py(px(REACH_Y))
                    .px(px(REACH_X))
                    .child(track),
            })
            .when(!disabled, |el| el.cursor_pointer())
            .when(disabled, |el| {
                el.cursor_not_allowed().opacity(DISABLED_OPACITY)
            })
            .role(Role::Switch)
            .when_some(name, |el, name| el.aria_label(name))
            .aria_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            })
            .when(!disabled || focused, |el| {
                el.track_focus(&handle.tab_index(0).tab_stop(!disabled))
            })
            .on_click(move |_, window, cx| {
                if disabled {
                    return;
                }
                if let Some(on_change) = &on_change {
                    on_change(!checked, window, cx);
                }
            })
            .a11y_state(false, disabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumb_travels_like_the_web() {
        assert_eq!(SwitchSize::Md.metrics().travel(), 16.0);
        assert_eq!(SwitchSize::Sm.metrics().travel(), 11.0);
    }

    #[test]
    fn the_grown_thumb_keeps_its_centre_and_stays_inside_the_track() {
        for size in [SwitchSize::Sm, SwitchSize::Md] {
            let m = size.metrics();
            let inner_w = m.width - 2.0 * BORDER;
            let inner_h = m.height - 2.0 * BORDER;
            let (l0, t0, s0) = m.thumb_at(0.0);
            let (l1, t1, s1) = m.thumb_at(1.0);
            assert!((s1 / s0 - GROWN).abs() < 1e-5);
            assert!(((t0 + s0 / 2.0) - (t1 + s1 / 2.0)).abs() < 1e-5);
            assert!(((l1 + s1 / 2.0) - (l0 + s0 / 2.0) - m.travel()).abs() < 1e-5);
            assert!(l0 >= 0.0 && l1 + s1 <= inner_w && t1 >= 0.0 && t1 + s1 <= inner_h);
        }
    }
}
