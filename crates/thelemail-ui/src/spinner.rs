use std::f32::consts::TAU;
use std::time::Duration;

use gpui_kit::{
    App, ElementId, Hsla, IntoElement, ParentElement, PathBuilder, RenderOnce, Styled, Window,
    canvas, div, point, px,
};

const STROKE: f32 = 2.0;
const SWEEP: f32 = TAU * 0.75;
const RING_SWEEP: f32 = TAU * 0.25;
const REDUCED_SLOWDOWN: u32 = 3;

pub const BUTTON_PERIOD: Duration = Duration::from_millis(800);
pub const STATUS_PERIOD: Duration = Duration::from_millis(700);

#[derive(IntoElement)]
pub struct Spinner {
    id: ElementId,
    size: f32,
    color: Hsla,
    track: Option<Hsla>,
    period: Duration,
}

impl Spinner {
    pub fn new(id: impl Into<ElementId>, size: f32, color: Hsla) -> Self {
        Self {
            id: id.into(),
            size,
            color,
            track: None,
            period: BUTTON_PERIOD,
        }
    }

    pub fn ring(mut self, track: Hsla) -> Self {
        self.track = Some(track);
        self
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
        let started = *window.use_keyed_state(self.id, cx, |_, _| now).read(cx);
        let period = if cx.reduce_motion() {
            self.period * REDUCED_SLOWDOWN
        } else {
            self.period
        };
        let angle = turn(now.saturating_duration_since(started), period);
        window.request_animation_frame();
        let color = self.color;
        let track = self.track;
        let sweep = if track.is_some() { RING_SWEEP } else { SWEEP };
        div().flex_none().size(px(self.size)).child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let radius = (bounds.size.width.min(bounds.size.height) - px(STROKE)) / 2.0;
                    let center = bounds.center();
                    let at =
                        |a: f32| point(center.x + radius * a.cos(), center.y + radius * a.sin());
                    if let Some(track) = track {
                        let mut ring = PathBuilder::stroke(px(STROKE));
                        ring.move_to(at(0.0));
                        ring.arc_to(point(radius, radius), px(0.0), false, true, at(TAU / 2.0));
                        ring.arc_to(point(radius, radius), px(0.0), false, true, at(0.0));
                        if let Ok(ring) = ring.build() {
                            window.paint_path(ring, track);
                        }
                    }
                    let mut path = PathBuilder::stroke(px(STROKE));
                    path.move_to(at(angle));
                    path.arc_to(
                        point(radius, radius),
                        px(0.0),
                        sweep > TAU / 2.0,
                        true,
                        at(angle + sweep),
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
