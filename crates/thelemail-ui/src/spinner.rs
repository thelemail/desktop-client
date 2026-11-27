use std::f32::consts::TAU;
use std::time::Duration;

use gpui_kit::{
    App, ElementId, Hsla, IntoElement, ParentElement, PathBuilder, RenderOnce, Styled, Window,
    canvas, div, point, px,
};

const STROKE: f32 = 2.0;
const SWEEP: f32 = TAU * 0.75;
const REDUCED_SLOWDOWN: u32 = 3;

pub const BUTTON_PERIOD: Duration = Duration::from_millis(800);
pub const STATUS_PERIOD: Duration = Duration::from_millis(700);

#[derive(IntoElement)]
pub struct Spinner {
    id: ElementId,
    size: f32,
    color: Hsla,
    period: Duration,
}

impl Spinner {
    pub fn new(id: impl Into<ElementId>, size: f32, color: Hsla) -> Self {
        Self {
            id: id.into(),
            size,
            color,
            period: BUTTON_PERIOD,
        }
    }

    pub fn period(mut self, period: Duration) -> Self {
        self.period = period;
        self
    }
}

pub(crate) fn turn(elapsed: Duration, period: Duration) -> f32 {
    let period = period.as_secs_f64();
    if period <= 0.0 {
        return 0.0;
    }
    ((elapsed.as_secs_f64() % period) / period) as f32 * TAU
}

impl RenderOnce for Spinner {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let now = cx.background_executor().now();
        let started = *window
            .use_keyed_state(self.id, cx, |_, _| now)
            .read(cx);
        let period = if cx.reduce_motion() {
            self.period * REDUCED_SLOWDOWN
        } else {
            self.period
        };
        let angle = turn(now.saturating_duration_since(started), period);
        window.request_animation_frame();
        let color = self.color;
        div().flex_none().size(px(self.size)).child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let radius = (bounds.size.width.min(bounds.size.height) - px(STROKE)) / 2.0;
                    let center = bounds.center();
                    let at = |a: f32| point(center.x + radius * a.cos(), center.y + radius * a.sin());
                    let mut path = PathBuilder::stroke(px(STROKE));
                    path.move_to(at(angle));
                    path.arc_to(
                        point(radius, radius),
                        px(0.0),
                        true,
                        true,
                        at(angle + SWEEP),
                    );
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                },
            )
            .size_full(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_period_is_one_turn() {
        assert!(turn(Duration::ZERO, BUTTON_PERIOD).abs() < 1e-6);
        let half = turn(Duration::from_millis(400), BUTTON_PERIOD);
        assert!((half - TAU / 2.0).abs() < 1e-4);
        assert!(turn(BUTTON_PERIOD, BUTTON_PERIOD).abs() < 1e-4);
    }
}
