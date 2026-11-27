use std::time::{Duration, Instant};

use gpui_kit::{App, ElementId, Window};

use super::curve::Curve;

pub fn progress(elapsed: Duration, delay: Duration, run: Duration, curve: Curve) -> f32 {
    let Some(into) = elapsed.checked_sub(delay) else {
        return 0.0;
    };
    if run.is_zero() || into >= run {
        return 1.0;
    }
    curve.value(into.as_secs_f64() / run.as_secs_f64()) as f32
}

pub fn once(
    id: impl Into<ElementId>,
    delay: Duration,
    run: Duration,
    curve: Curve,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    if cx.reduce_motion() {
        return 1.0;
    }
    let now = cx.background_executor().now();
    let id = ElementId::NamedChild(id.into().into(), "once".into());
    let started: Instant = *window.use_keyed_state(id, cx, |_, _| now).read(cx);
    let value = progress(now.saturating_duration_since(started), delay, run, curve);
    if value < 1.0 {
        window.request_animation_frame();
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::{EASE_OUT, LINEAR};

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn nothing_moves_during_the_delay() {
        assert_eq!(progress(ms(119), ms(120), ms(620), LINEAR), 0.0);
    }

    #[test]
    fn it_runs_after_the_delay_and_settles_at_one() {
        assert!((progress(ms(430), ms(120), ms(620), LINEAR) - 0.5).abs() < 1e-3);
        assert_eq!(progress(ms(740), ms(120), ms(620), EASE_OUT), 1.0);
        assert_eq!(progress(ms(5_000), ms(0), ms(320), EASE_OUT), 1.0);
    }
}
