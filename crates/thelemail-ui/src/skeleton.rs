use gpui_kit::{
    App, ElementId, Hsla, InteractiveElement, IntoElement, Length, ParentElement, RenderOnce, Role,
    SharedString, StatefulInteractiveElement, Styled, Window, div, linear_color_stop,
    linear_gradient, px, relative,
};
use thelemail_i18n::m;

use crate::a11y::A11yStateExt as _;
use crate::icons::{IconName, glyph};
use crate::motion::{mix, pulse, shimmer};
use crate::spinner::Spinner;
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_MONO, RADIUS_XS};

const BAR_HEIGHT: f32 = 10.0;
const BAR_RADIUS: f32 = 5.0;
const SWEEP_LIGHT: f32 = 0.45;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Animation {
    #[default]
    Shimmer,
    Pulse,
    Still,
}

#[derive(IntoElement)]
pub struct Skeleton {
    id: ElementId,
    width: Length,
    height: f32,
    radius: f32,
    animation: Animation,
    fill: Option<Hsla>,
    opacity: f32,
    theme: Option<Theme>,
}

impl Skeleton {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            width: relative(1.0).into(),
            height: BAR_HEIGHT,
            radius: BAR_RADIUS,
            animation: Animation::default(),
            fill: None,
            opacity: 1.0,
            theme: None,
        }
    }

    pub fn w(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn h(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    pub fn animation(mut self, animation: Animation) -> Self {
        self.animation = animation;
        self
    }

    pub fn fill(mut self, fill: Hsla) -> Self {
        self.fill = Some(fill);
        self
    }

    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
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

impl RenderOnce for Skeleton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let base = self
            .fill
            .unwrap_or_else(|| theme.color(ColorToken::Skeleton));
        let moving = !cx.reduce_motion();
        let opacity = match self.animation {
            Animation::Pulse if moving => self.opacity * pulse(self.child_id("pulse"), window, cx),
            _ => self.opacity,
        };
        let sweep = (moving && self.animation == Animation::Shimmer).then(|| {
            let phase = shimmer(self.child_id("shimmer"), window, cx);
            let light = mix(base, theme.color(ColorToken::Surface), SWEEP_LIGHT);
            let clear = light.alpha(0.0);
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(phase * 2.0 - 1.0))
                .w(relative(1.0))
                .flex()
                .child(div().flex_1().bg(linear_gradient(
                    90.0,
                    linear_color_stop(clear, 0.0),
                    linear_color_stop(light, 1.0),
                )))
                .child(div().flex_1().bg(linear_gradient(
                    90.0,
                    linear_color_stop(light, 0.0),
                    linear_color_stop(clear, 1.0),
                )))
        });
        div()
            .id(self.id)
            .flex_none()
            .relative()
            .overflow_hidden()
            .w(self.width)
            .h(px(self.height))
            .rounded(px(self.radius))
            .bg(base)
            .opacity(opacity)
            .children(sweep)
    }
}

const LIST_ROWS: usize = 9;
const LIST_WIDTHS: [f32; 9] = [0.62, 0.78, 0.54, 0.70, 0.84, 0.58, 0.74, 0.66, 0.80];
const LIST_SEAL: f32 = 22.0;
const LIST_GAP: f32 = 10.0;
const LIST_LINE: f32 = 9.0;
const LIST_PREVIEW: f32 = 8.0;
const LIST_STACK_GAP: f32 = 7.0;
const LIST_TOP_GAP: f32 = 12.0;
const LIST_DATE: f32 = 44.0;
const LIST_SENDER: f32 = 0.34;
const LIST_PREVIEW_WIDTH: f32 = 0.88;
const LIST_PREVIEW_OPACITY: f32 = 0.6;
const LIST_SEAL_FILL: f32 = 0.08;
const LIST_NARROW: f32 = 1000.0;

struct Pad {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

const LIST_PAD: Pad = Pad {
    top: 9.0,
    right: 14.0,
    bottom: 10.0,
    left: 13.0,
};

const LIST_PAD_NARROW: Pad = Pad {
    top: 12.0,
    right: 12.0,
    bottom: 12.0,
    left: 10.0,
};

pub fn list_width(row: usize) -> f32 {
    LIST_WIDTHS[row % LIST_WIDTHS.len()]
}

#[derive(IntoElement)]
pub struct ListSkeleton {
    id: ElementId,
    rows: usize,
    label: Option<SharedString>,
    theme: Option<Theme>,
}

impl ListSkeleton {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            rows: LIST_ROWS,
            label: None,
            theme: None,
        }
    }

    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows;
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

    fn row_id(&self, row: usize, part: &'static str) -> ElementId {
        ElementId::NamedChild(
            ElementId::NamedChild(self.id.clone().into(), format!("row-{row}").into()).into(),
            part.into(),
        )
    }
}

impl RenderOnce for ListSkeleton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let pad = if window.viewport_size().width <= px(LIST_NARROW) {
            &LIST_PAD_NARROW
        } else {
            &LIST_PAD
        };
        let seal = theme.color(ColorToken::FgStrong);
        let seal = seal.alpha(seal.a * LIST_SEAL_FILL);
        let bar = |id: ElementId| {
            Skeleton::new(id)
                .h(LIST_LINE)
                .radius(RADIUS_XS)
                .theme(theme)
        };
        let label: SharedString = self
            .label
            .clone()
            .unwrap_or_else(|| m::mail_list_loading().into());
        let rows = (0..self.rows).map(|row| {
            div()
                .flex()
                .gap(px(LIST_GAP))
                .pt(px(pad.top))
                .pr(px(pad.right))
                .pb(px(pad.bottom))
                .pl(px(pad.left))
                .border_b_1()
                .border_color(theme.color(ColorToken::BorderSoft))
                .child(
                    Skeleton::new(self.row_id(row, "seal"))
                        .w(px(LIST_SEAL))
                        .h(LIST_SEAL)
                        .radius(LIST_SEAL / 2.0)
                        .fill(seal)
                        .animation(Animation::Still)
                        .theme(theme),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(LIST_STACK_GAP))
                        .pt(px(3.0))
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .gap(px(LIST_TOP_GAP))
                                .child(bar(self.row_id(row, "from")).w(relative(LIST_SENDER)))
                                .child(bar(self.row_id(row, "date")).w(px(LIST_DATE))),
                        )
                        .child(bar(self.row_id(row, "subject")).w(relative(list_width(row))))
                        .child(
                            bar(self.row_id(row, "preview"))
                                .w(relative(LIST_PREVIEW_WIDTH))
                                .h(LIST_PREVIEW)
                                .opacity(LIST_PREVIEW_OPACITY),
                        ),
                )
        });
        div()
            .id(self.id.clone())
            .w_full()
            .flex()
            .flex_col()
            .role(Role::Status)
            .aria_label(label)
            .children(rows)
            .a11y_state(true, false)
    }
}

const BODY_LINES: [Option<f32>; 9] = [
    Some(0.96),
    Some(1.0),
    Some(0.89),
    Some(0.71),
    None,
    Some(0.93),
    Some(1.0),
    Some(0.82),
    Some(0.58),
];
const BODY_LINE: f32 = 13.0;
const BODY_GAP: f32 = 13.0;
const BODY_BREAK: f32 = 14.0;
const BODY_SPINNER: f32 = 14.0;
const BODY_LOCK: f32 = 13.0;
const BODY_STATUS_FONT: f32 = 11.5;
const BODY_STATUS_GAP: f32 = 8.0;
const BODY_STATUS_BELOW: f32 = 20.0;

#[derive(IntoElement)]
pub struct BodyPending {
    id: ElementId,
    theme: Option<Theme>,
}

impl BodyPending {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            theme: None,
        }
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    fn child_id(&self, name: impl Into<SharedString>) -> ElementId {
        ElementId::NamedChild(self.id.clone().into(), name.into())
    }
}

impl RenderOnce for BodyPending {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let text: SharedString = m::mail_skeleton_decrypting().into();
        let status = div()
            .id(self.child_id("status"))
            .flex()
            .items_center()
            .gap(px(BODY_STATUS_GAP))
            .mb(px(BODY_STATUS_BELOW))
            .font_family(FONT_MONO.name)
            .text_size(px(BODY_STATUS_FONT * FONT_MONO.size_adjust))
            .text_color(theme.color(ColorToken::FgMuted))
            .role(Role::Status)
            .aria_label(text.clone())
            .child(
                Spinner::new(
                    self.child_id("spinner"),
                    BODY_SPINNER,
                    theme.color(ColorToken::Pine600),
                )
                .ring(theme.color(ColorToken::Pine100)),
            )
            .child(glyph(
                IconName::Lock,
                px(BODY_LOCK),
                theme.color(ColorToken::Brass600),
            ))
            .child(text);
        let lines = BODY_LINES.iter().enumerate().map(|(i, width)| match width {
            Some(width) => Skeleton::new(self.child_id(format!("line-{i}")))
                .w(relative(*width))
                .h(BODY_LINE)
                .animation(Animation::Pulse)
                .theme(theme)
                .into_any_element(),
            None => div().h(px(BODY_BREAK - BODY_GAP)).into_any_element(),
        });
        div()
            .id(self.id.clone())
            .w_full()
            .pt(px(4.0))
            .pb(px(8.0))
            .child(status)
            .child(div().flex().flex_col().gap(px(BODY_GAP)).children(lines))
            .a11y_state(true, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_rows_cycle_through_the_web_widths() {
        assert_eq!(list_width(0), 0.62);
        assert_eq!(list_width(8), 0.80);
        assert_eq!(list_width(9), 0.62);
    }

    #[test]
    fn the_body_keeps_one_paragraph_break() {
        assert_eq!(BODY_LINES.iter().filter(|w| w.is_none()).count(), 1);
        assert_eq!(BODY_LINES.len(), 9);
    }
}
