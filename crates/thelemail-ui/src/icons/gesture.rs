use std::time::Duration;

use gpui_kit::{Transformation, point, px, radians, size};

use crate::motion::{Curve, EASE_OUT};

pub const GESTURE_DURATION: Duration = Duration::from_millis(520);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gesture {
    Drop,
    Fly,
    FlyLevel,
    Spin,
    Ring,
    Shake,
    Pulse,
    Tilt,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub tx: f32,
    pub ty: f32,
    pub rotate: f32,
    pub sx: f32,
    pub sy: f32,
    pub alpha: f32,
}

impl Pose {
    pub const REST: Self = Self {
        tx: 0.0,
        ty: 0.0,
        rotate: 0.0,
        sx: 1.0,
        sy: 1.0,
        alpha: 1.0,
    };

    const fn moved(tx: f32, ty: f32) -> Self {
        Self {
            tx,
            ty,
            ..Self::REST
        }
    }

    const fn turned(rotate: f32) -> Self {
        Self {
            rotate,
            ..Self::REST
        }
    }

    const fn scaled(s: f32) -> Self {
        Self {
            sx: s,
            sy: s,
            ..Self::REST
        }
    }

    const fn then_y(mut self, ty: f32) -> Self {
        self.ty = ty;
        self
    }

    const fn squash(mut self, sy: f32) -> Self {
        self.sy = sy;
        self
    }

    const fn shrunk(mut self, s: f32) -> Self {
        self.sx = s;
        self.sy = s;
        self
    }

    const fn faded(mut self, alpha: f32) -> Self {
        self.alpha = alpha;
        self
    }

    fn lerp(self, to: Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            tx: mix(self.tx, to.tx),
            ty: mix(self.ty, to.ty),
            rotate: mix(self.rotate, to.rotate),
            sx: mix(self.sx, to.sx),
            sy: mix(self.sy, to.sy),
            alpha: mix(self.alpha, to.alpha),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    TranslateOutside,
    TranslateInside,
}

type Frames = &'static [(f32, Pose)];

const DROP: Frames = &[
    (0.0, Pose::REST),
    (0.35, Pose::moved(0.0, 3.0).squash(0.9)),
    (0.70, Pose::moved(0.0, -1.0)),
    (1.0, Pose::REST),
];

const FLY: Frames = &[
    (0.0, Pose::REST),
    (0.45, Pose::moved(9.0, -9.0).shrunk(0.6).faded(0.0)),
    (0.46, Pose::moved(-9.0, 9.0).shrunk(0.6).faded(0.0)),
    (1.0, Pose::REST),
];

const FLY_LEVEL: Frames = &[
    (0.0, Pose::REST),
    (0.45, Pose::moved(11.0, 0.0).shrunk(0.7).faded(0.0)),
    (0.46, Pose::moved(-11.0, 0.0).shrunk(0.7).faded(0.0)),
    (1.0, Pose::REST),
];

const SPIN: Frames = &[
    (0.0, Pose::REST),
    (0.5, Pose::turned(144.0).shrunk(1.25)),
    (1.0, Pose::turned(216.0)),
];

const RING: Frames = &[
    (0.0, Pose::REST),
    (0.15, Pose::turned(-14.0)),
    (0.30, Pose::turned(12.0)),
    (0.45, Pose::turned(-9.0)),
    (0.60, Pose::turned(6.0)),
    (0.75, Pose::turned(-3.0)),
    (1.0, Pose::REST),
];

const SHAKE: Frames = &[
    (0.0, Pose::REST),
    (0.25, Pose::turned(-9.0).then_y(-1.0)),
    (0.55, Pose::turned(7.0)),
    (0.80, Pose::turned(-3.0)),
    (1.0, Pose::REST),
];

const PULSE: Frames = &[
    (0.0, Pose::REST),
    (0.4, Pose::scaled(1.22)),
    (1.0, Pose::REST),
];

const TILT: Frames = &[
    (0.0, Pose::REST),
    (0.4, Pose::turned(-10.0).then_y(-1.0)),
    (1.0, Pose::REST),
];

const SEGMENT: Curve = EASE_OUT;

impl Gesture {
    pub const ALL: [Self; 8] = [
        Self::Drop,
        Self::Fly,
        Self::FlyLevel,
        Self::Spin,
        Self::Ring,
        Self::Shake,
        Self::Pulse,
        Self::Tilt,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Drop => "drop",
            Self::Fly => "fly",
            Self::FlyLevel => "fly-level",
            Self::Spin => "spin",
            Self::Ring => "ring",
            Self::Shake => "shake",
            Self::Pulse => "pulse",
            Self::Tilt => "tilt",
        }
    }

    const fn frames(self) -> Frames {
        match self {
            Self::Drop => DROP,
            Self::Fly => FLY,
            Self::FlyLevel => FLY_LEVEL,
            Self::Spin => SPIN,
            Self::Ring => RING,
            Self::Shake => SHAKE,
            Self::Pulse => PULSE,
            Self::Tilt => TILT,
        }
    }

    const fn order(self) -> Order {
        match self {
            Self::Shake | Self::Tilt => Order::TranslateInside,
            _ => Order::TranslateOutside,
        }
    }

    pub fn pose(self, progress: f32) -> Pose {
        let frames = self.frames();
        let p = progress.clamp(0.0, 1.0);
        let Some(i) = frames.windows(2).position(|w| p <= w[1].0) else {
            return frames[frames.len() - 1].1;
        };
        let (from_at, from) = frames[i];
        let (to_at, to) = frames[i + 1];
        let span = to_at - from_at;
        if span <= 0.0 {
            return to;
        }
        let local = f64::from((p - from_at) / span);
        from.lerp(to, SEGMENT.value(local) as f32)
    }

    fn offset(self, pose: Pose) -> (f32, f32) {
        match self.order() {
            Order::TranslateOutside => (pose.tx, pose.ty),
            Order::TranslateInside => {
                let (sin, cos) = pose.rotate.to_radians().sin_cos();
                (pose.tx * cos - pose.ty * sin, pose.tx * sin + pose.ty * cos)
            }
        }
    }

    pub fn transformation(self, progress: f32) -> Transformation {
        let pose = self.pose(progress);
        let angle = pose.rotate.to_radians();
        let (tx, ty) = self.offset(pose);
        Transformation::scale(size(pose.sx, pose.sy))
            .with_rotation(radians(angle))
            .with_translation(point(px(tx), px(ty)))
            .with_alpha(pose.alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Pose, b: Pose) -> bool {
        let near = |x: f32, y: f32| (x - y).abs() < 1e-4;
        near(a.tx, b.tx)
            && near(a.ty, b.ty)
            && near(a.rotate, b.rotate)
            && near(a.sx, b.sx)
            && near(a.sy, b.sy)
            && near(a.alpha, b.alpha)
    }

    #[test]
    fn every_gesture_starts_at_rest() {
        for g in Gesture::ALL {
            assert!(close(g.pose(0.0), Pose::REST), "{}", g.name());
        }
    }

    #[test]
    fn every_gesture_ends_where_the_icon_rests() {
        for g in Gesture::ALL {
            let end = g.pose(1.0);
            let turned = Pose { rotate: 0.0, ..end };
            assert!(close(turned, Pose::REST), "{}", g.name());
            assert_eq!(end.rotate % 72.0, 0.0, "{}", g.name());
        }
    }

    #[test]
    fn keyframes_are_hit_exactly() {
        for g in Gesture::ALL {
            for &(at, pose) in g.frames() {
                assert!(close(g.pose(at), pose), "{} at {at}", g.name());
            }
        }
    }

    #[test]
    fn segments_ease_out_from_each_keyframe() {
        let half = Gesture::Pulse.pose(0.2);
        let eased = EASE_OUT.value(0.5) as f32;
        assert!((half.sx - (1.0 + 0.22 * eased)).abs() < 1e-5);
        assert!(half.sx > 1.11);
    }

    #[test]
    fn fly_vanishes_and_comes_back_from_the_other_side() {
        let gone = Gesture::Fly.pose(0.455);
        assert_eq!(gone.alpha, 0.0);
        let back = Gesture::Fly.pose(0.47);
        assert!(back.tx < 0.0 && back.ty > 0.0);
        assert!(Gesture::Fly.pose(0.3).tx > 0.0);
    }

    #[test]
    fn progress_outside_the_run_is_clamped() {
        for g in Gesture::ALL {
            assert!(close(g.pose(-1.0), g.pose(0.0)));
            assert!(close(g.pose(2.0), g.pose(1.0)));
        }
    }

    #[test]
    fn shake_lifts_along_the_rotated_axis() {
        let pose = Pose::turned(-9.0).then_y(-1.0);
        let (tx, ty) = Gesture::Shake.offset(pose);
        let angle = (-9.0f32).to_radians();
        assert!((tx - angle.sin()).abs() < 1e-6);
        assert!((ty + angle.cos()).abs() < 1e-6);
        assert!(tx < 0.0 && ty < 0.0);
        assert_eq!(Gesture::Drop.offset(Pose::moved(0.0, 3.0)), (0.0, 3.0));
    }
}
