use std::time::Duration;

use gpui_kit::base::{Keyframe, Keyframes, Timing, animate_keyframes};
use gpui_kit::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, Role, SharedString,
    StatefulInteractiveElement, Styled, Transformation, Window, accesskit, div, px, relative, size,
};

use crate::a11y::A11yStateExt as _;
use crate::icons::{IconName, glyph};
use crate::motion::{BASE, EASE_OUT, Spec, animate, ease_color};
use crate::spinner::{STATUS_PERIOD, Spinner};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_SANS, RADIUS_PILL};

const BAR_H: f32 = 6.0;
const BAR_RADIUS: f32 = 3.0;
const BAR_BELOW: f32 = 20.0;
const LIST_GAP: f32 = 16.0;
const ROW_GAP: f32 = 12.0;
const FONT: f32 = 14.5;
const SLOT: f32 = 24.0;
const SLOT_BORDER: f32 = 1.5;
const SPINNER: f32 = 15.0;
const TICK: f32 = 14.0;
const TICK_FROM: f32 = 0.4;
const TICK_POP: Duration = Duration::from_millis(320);
const BAR: Spec = Spec::new(Duration::from_millis(600), EASE_OUT);
const INK: Spec = Spec::new(BASE, EASE_OUT);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Pending,
    Current,
    Done,
    Failed,
}

pub fn row_state(index: usize, at: usize, failed: bool) -> Row {
    match index.cmp(&at) {
        std::cmp::Ordering::Less => Row::Done,
        std::cmp::Ordering::Equal if failed => Row::Failed,
        std::cmp::Ordering::Equal => Row::Current,
        std::cmp::Ordering::Greater => Row::Pending,
    }
}

pub fn progress(at: usize, len: usize) -> f32 {
    if len == 0 {
        return 0.0;
    }
    ((at.min(len) as f32 / len as f32) * 100.0).round() / 100.0
}

#[derive(IntoElement)]
pub struct WorkSteps {
    id: ElementId,
    lines: Vec<SharedString>,
    at: usize,
    failed: bool,
    theme: Option<Theme>,
}

impl WorkSteps {
    pub fn new(id: impl Into<ElementId>, lines: Vec<SharedString>, at: usize) -> Self {
        Self {
            id: id.into(),
            lines,
            at,
            failed: false,
            theme: None,
        }
    }

    pub fn failed(mut self, failed: bool) -> Self {
        self.failed = failed;
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

fn pop(id: ElementId, window: &mut Window, cx: &mut App) -> f32 {
    let keyframes = Keyframes::try_new([
        Keyframe::new(0.0, 0.0_f32).ease(EASE_OUT.easing()),
        Keyframe::new(1.0, 1.0),
    ])
    .expect("two endpoint keyframes");
    animate_keyframes(id, &keyframes, Timing::new(TICK_POP), window, cx).value
}

impl RenderOnce for WorkSteps {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let fill = animate(
            self.child_id("bar"),
            progress(self.at, self.lines.len()),
            BAR,
            window,
            cx,
        )
        .value
        .clamp(0.0, 1.0);
        let rows: Vec<_> = self
            .lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let state = row_state(i, self.at, self.failed);
                let ink = match state {
                    Row::Pending | Row::Failed => ColorToken::FgFaint,
                    Row::Current => ColorToken::FgStrong,
                    Row::Done => ColorToken::Fg,
                };
                let ink = ease_color(
                    self.child_id(format!("ink-{i}")),
                    theme.color(ink),
                    INK,
                    window,
                    cx,
                );
                let slot = div()
                    .flex_none()
                    .size(px(SLOT))
                    .rounded(px(RADIUS_PILL))
                    .flex()
                    .items_center()
                    .justify_center();
                let slot = match state {
                    Row::Pending => slot
                        .border(px(SLOT_BORDER))
                        .border_color(theme.color(ColorToken::BorderStrong)),
                    Row::Failed => slot
                        .border(px(SLOT_BORDER))
                        .border_color(theme.color(ColorToken::Danger500)),
                    Row::Current => slot.child(
                        Spinner::new(
                            self.child_id(format!("spin-{i}")),
                            SPINNER,
                            theme.color(ColorToken::Primary).opacity(0.85),
                        )
                        .period(STATUS_PERIOD),
                    ),
                    Row::Done => {
                        let shown = pop(self.child_id(format!("tick-{i}")), window, cx);
                        let scale = TICK_FROM + (1.0 - TICK_FROM) * shown;
                        slot.bg(theme.color(ColorToken::Success100)).child(
                            div().opacity(shown).child(
                                glyph(
                                    IconName::Check,
                                    px(TICK),
                                    theme.color(ColorToken::Success700),
                                )
                                .with_transformation(Transformation::scale(size(scale, scale))),
                            ),
                        )
                    }
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(ROW_GAP))
                    .text_color(ink)
                    .child(slot)
                    .child(div().flex_1().min_w_0().child(line.clone()))
            })
            .collect();
        div()
            .id(self.id.clone())
            .w_full()
            .flex()
            .flex_col()
            .font_family(FONT_SANS.name)
            .text_size(px(FONT * FONT_SANS.size_adjust))
            .child(
                div()
                    .w_full()
                    .h(px(BAR_H))
                    .mb(px(BAR_BELOW))
                    .rounded(px(BAR_RADIUS))
                    .overflow_hidden()
                    .bg(theme.color(ColorToken::BgSunken))
                    .child(
                        div()
                            .h_full()
                            .w(relative(fill))
                            .rounded(px(BAR_RADIUS))
                            .bg(theme.color(ColorToken::Primary)),
                    ),
            )
            .child(
                div()
                    .id(self.child_id("list"))
                    .role(Role::List)
                    .flex()
                    .flex_col()
                    .gap(px(LIST_GAP))
                    .children(rows)
                    .a11y_state(!self.failed && self.at < self.lines.len(), false)
                    .live(accesskit::Live::Polite),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_before_the_cursor_are_done() {
        assert_eq!(row_state(0, 2, false), Row::Done);
        assert_eq!(row_state(2, 2, false), Row::Current);
        assert_eq!(row_state(3, 2, false), Row::Pending);
    }

    #[test]
    fn a_failure_marks_only_the_current_row() {
        assert_eq!(row_state(1, 2, true), Row::Done);
        assert_eq!(row_state(2, 2, true), Row::Failed);
        assert_eq!(row_state(3, 2, true), Row::Pending);
    }

    #[test]
    fn progress_rounds_like_the_web_percentage() {
        assert_eq!(progress(1, 3), 0.33);
        assert_eq!(progress(2, 3), 0.67);
        assert_eq!(progress(5, 3), 1.0);
        assert_eq!(progress(0, 0), 0.0);
    }
}
