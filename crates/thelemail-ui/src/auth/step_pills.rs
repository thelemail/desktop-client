use std::time::Duration;

use gpui_kit::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, Role,
    StatefulInteractiveElement, Styled, Window, div, px, relative,
};
use thelemail_i18n::m;

use crate::motion::{EASE_OUT, Spec, animate, once};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::ColorToken;

const PILL_W: f32 = 28.0;
const PILL_H: f32 = 4.0;
const PILL_RADIUS: f32 = 2.0;
const GAP: f32 = 5.0;
const FILL: Spec = Spec::new(Duration::from_millis(520), EASE_OUT);
const ARRIVE_DELAY: Duration = Duration::from_millis(120);
const ARRIVE: Duration = Duration::from_millis(620);

pub fn fills(total: usize, current: usize, fraction: f32) -> Vec<f32> {
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    (0..total)
        .map(|i| match i.cmp(&current) {
            std::cmp::Ordering::Less => 1.0,
            std::cmp::Ordering::Equal => fraction,
            std::cmp::Ordering::Greater => 0.0,
        })
        .collect()
}

#[derive(IntoElement)]
pub struct StepPills {
    id: ElementId,
    total: usize,
    current: usize,
    fraction: f32,
    theme: Option<Theme>,
}

impl StepPills {
    pub fn new(id: impl Into<ElementId>, total: usize, current: usize) -> Self {
        Self {
            id: id.into(),
            total,
            current,
            fraction: 1.0,
            theme: None,
        }
    }

    pub fn fraction(mut self, fraction: f32) -> Self {
        self.fraction = fraction;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    fn child_id(&self, name: String) -> ElementId {
        ElementId::NamedChild(self.id.clone().into(), name.into())
    }
}

impl RenderOnce for StepPills {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let targets = fills(self.total, self.current, self.fraction);
        let pills: Vec<_> = targets
            .iter()
            .enumerate()
            .map(|(i, target)| {
                let eased = animate(
                    self.child_id(format!("fill-{i}")),
                    *target,
                    FILL,
                    window,
                    cx,
                )
                .value
                .clamp(0.0, 1.0);
                let shown = if i == self.current {
                    let id = self.child_id(format!("arrive-{}", self.current));
                    eased * once(id, ARRIVE_DELAY, ARRIVE, EASE_OUT, window, cx)
                } else {
                    eased
                };
                div()
                    .flex_none()
                    .w(px(PILL_W))
                    .h(px(PILL_H))
                    .rounded(px(PILL_RADIUS))
                    .overflow_hidden()
                    .bg(theme.color(ColorToken::BorderSoft))
                    .child(
                        div()
                            .h_full()
                            .w(relative(shown))
                            .bg(theme.color(ColorToken::Primary)),
                    )
            })
            .collect();
        let step = (self.current + 1).min(self.total);
        div()
            .id(self.id.clone())
            .role(Role::Group)
            .aria_label(m::auth_step_of(step, self.total))
            .flex()
            .justify_center()
            .gap(px(GAP))
            .children(pills)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_steps_are_full_and_later_ones_empty() {
        assert_eq!(fills(4, 2, 1.0), vec![1.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn a_work_step_fills_with_its_real_progress() {
        assert_eq!(fills(3, 1, 0.4), vec![1.0, 0.4, 0.0]);
    }

    #[test]
    fn the_fraction_is_clamped() {
        assert_eq!(fills(2, 0, 1.7), vec![1.0, 0.0]);
        assert_eq!(fills(2, 0, -0.2), vec![0.0, 0.0]);
        assert_eq!(fills(2, 0, f32::NAN), vec![0.0, 0.0]);
    }

    #[test]
    fn past_the_last_step_every_pill_is_full() {
        assert_eq!(fills(3, 3, 0.0), vec![1.0, 1.0, 1.0]);
    }
}
