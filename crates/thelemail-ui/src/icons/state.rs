use std::time::Instant;

use gpui_kit::{
    App, ElementId, IntoElement, ParentElement, RenderOnce, Styled, Transformation, Window, div,
};

use super::gesture::{GESTURE_DURATION, Gesture};
use super::{IconName, SIZE_DEFAULT, Tint, checked_size, glyph};
use crate::motion::{BASE, EASE, Spec, animate};
use crate::tokens::ColorToken;

const CROSSFADE: Spec = Spec::new(BASE, EASE);

#[derive(Debug, Clone, Copy, Default)]
struct Run {
    was: Option<bool>,
    started: Option<Instant>,
}

impl Run {
    fn step(&mut self, active: bool, gesture: bool, reduced: bool, now: Instant) -> Option<f32> {
        if gesture && !reduced && active && self.was == Some(false) {
            self.started = Some(now);
        }
        self.was = Some(active);
        if reduced {
            self.started = None;
        }
        let started = self.started?;
        let progress =
            now.saturating_duration_since(started).as_secs_f32() / GESTURE_DURATION.as_secs_f32();
        if progress >= 1.0 {
            self.started = None;
            return None;
        }
        Some(progress)
    }
}

#[derive(IntoElement)]
pub struct StateIcon {
    id: ElementId,
    icon: IconName,
    active_icon: Option<IconName>,
    active: bool,
    gesture: Option<Gesture>,
    size: f32,
    tint: Tint,
}

impl StateIcon {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            active_icon: None,
            active: false,
            gesture: None,
            size: SIZE_DEFAULT,
            tint: Tint::Token(ColorToken::Fg),
        }
    }

    pub fn active_icon(mut self, icon: IconName) -> Self {
        self.active_icon = Some(icon);
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn gesture(mut self, gesture: Gesture) -> Self {
        self.gesture = Some(gesture);
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, tint: impl Into<Tint>) -> Self {
        self.tint = tint.into();
        self
    }
}

impl RenderOnce for StateIcon {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let size = checked_size(self.size);
        let color = self.tint.resolve(cx);
        let on = animate(
            ElementId::NamedChild(self.id.clone().into(), "on".into()),
            if self.active { 1.0 } else { 0.0 },
            CROSSFADE,
            window,
            cx,
        )
        .value
        .clamp(0.0, 1.0);

        let reduced = cx.reduce_motion();
        let now = cx.background_executor().now();
        let run = window.use_keyed_state(
            ElementId::NamedChild(self.id.into(), "gesture".into()),
            cx,
            |_, _| Run::default(),
        );
        let progress = run.update(cx, |run, _| {
            run.step(self.active, self.gesture.is_some(), reduced, now)
        });
        let pose = match (self.gesture, progress) {
            (Some(gesture), Some(progress)) => {
                window.request_animation_frame();
                gesture.transformation(progress)
            }
            _ => Transformation::default(),
        };

        let rest = glyph(self.icon, size, color.opacity(1.0 - on));
        let lit = glyph(
            self.active_icon.unwrap_or(self.icon),
            size,
            color.opacity(on),
        );
        div()
            .flex_none()
            .relative()
            .size(size)
            .child(rest.absolute().top_0().left_0().with_transformation(pose))
            .child(lit.absolute().top_0().left_0().with_transformation(pose))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn turning_on_plays_the_gesture_once() {
        let t0 = Instant::now();
        let mut run = Run::default();
        assert_eq!(run.step(false, true, false, t0), None);
        assert_eq!(run.step(true, true, false, at(t0, 10)), Some(0.0));
        let mid = run.step(true, true, false, at(t0, 270)).unwrap();
        assert!((mid - 0.5).abs() < 1e-3);
        assert_eq!(run.step(true, true, false, at(t0, 530)), None);
        assert_eq!(run.step(true, true, false, at(t0, 600)), None);
    }

    #[test]
    fn the_first_frame_never_plays() {
        let t0 = Instant::now();
        let mut run = Run::default();
        assert_eq!(run.step(true, true, false, t0), None);
    }

    #[test]
    fn turning_off_does_not_play() {
        let t0 = Instant::now();
        let mut run = Run::default();
        run.step(true, true, false, t0);
        assert_eq!(run.step(false, true, false, at(t0, 10)), None);
    }

    #[test]
    fn turning_on_again_restarts() {
        let t0 = Instant::now();
        let mut run = Run::default();
        run.step(false, true, false, t0);
        run.step(true, true, false, at(t0, 10));
        run.step(false, true, false, at(t0, 200));
        assert_eq!(run.step(true, true, false, at(t0, 300)), Some(0.0));
    }

    #[test]
    fn reduced_motion_skips_the_gesture() {
        let t0 = Instant::now();
        let mut run = Run::default();
        run.step(false, true, true, t0);
        assert_eq!(run.step(true, true, true, at(t0, 10)), None);
        run.step(false, true, false, at(t0, 20));
        run.step(true, true, false, at(t0, 30));
        assert_eq!(run.step(true, true, true, at(t0, 40)), None);
    }

    #[test]
    fn no_gesture_means_no_run() {
        let t0 = Instant::now();
        let mut run = Run::default();
        run.step(false, false, false, t0);
        assert_eq!(run.step(true, false, false, at(t0, 10)), None);
    }
}
