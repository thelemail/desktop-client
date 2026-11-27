use std::time::{Duration, Instant};

use gpui_kit::{SpringConfig, SpringState};

use super::curve::Curve;

const SETTLE_DECAY: f64 = 6.64;
const EPSILON_FRACTION: f32 = 1e-3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reduced {
    Finish,
    Keep(Duration),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spec {
    pub duration: Duration,
    pub curve: Curve,
    pub reduced: Reduced,
}

impl Spec {
    pub const fn new(duration: Duration, curve: Curve) -> Self {
        Self {
            duration,
            curve,
            reduced: Reduced::Finish,
        }
    }

    pub const fn keep_when_reduced(mut self, duration: Duration) -> Self {
        self.reduced = Reduced::Keep(duration);
        self
    }

    fn effective(self, reduce_motion: bool) -> Option<Self> {
        if !reduce_motion {
            return (!self.duration.is_zero()).then_some(self);
        }
        match self.reduced {
            Reduced::Finish => None,
            Reduced::Keep(duration) if duration.is_zero() => None,
            Reduced::Keep(duration) => Some(Self { duration, ..self }),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Rest,
    Curve {
        from: f32,
        start: Instant,
        duration: Duration,
        curve: Curve,
    },
    Spring {
        state: SpringState,
        at: Instant,
        config: SpringConfig,
        epsilon: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub value: f32,
    pub velocity: f32,
    pub settled: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Motion {
    target: f32,
    phase: Phase,
}

impl Motion {
    pub const fn at(value: f32) -> Self {
        Self {
            target: value,
            phase: Phase::Rest,
        }
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn peek(&self, now: Instant) -> Sample {
        let rest = Sample {
            value: self.target,
            velocity: 0.0,
            settled: true,
        };
        match self.phase {
            Phase::Rest => rest,
            Phase::Curve {
                from,
                start,
                duration,
                curve,
            } => {
                let seconds = duration.as_secs_f64();
                let t = now.saturating_duration_since(start).as_secs_f64() / seconds;
                if t >= 1.0 {
                    return rest;
                }
                let span = f64::from(self.target - from);
                Sample {
                    value: (f64::from(from) + span * curve.value(t)) as f32,
                    velocity: (span * curve.slope(t) / seconds) as f32,
                    settled: false,
                }
            }
            Phase::Spring {
                state,
                at,
                config,
                epsilon,
            } => {
                let dt = now.saturating_duration_since(at).as_secs_f32();
                let stepped = config.step(state, self.target, dt);
                if config.is_settled(stepped, self.target, epsilon) {
                    return rest;
                }
                Sample {
                    value: stepped.position,
                    velocity: stepped.velocity,
                    settled: false,
                }
            }
        }
    }

    pub fn sample(&mut self, now: Instant) -> Sample {
        let sample = self.peek(now);
        if sample.settled {
            self.phase = Phase::Rest;
        }
        sample
    }

    pub fn jump(&mut self, value: f32) {
        self.target = value;
        self.phase = Phase::Rest;
    }

    pub fn retarget(&mut self, target: f32, spec: Spec, reduce_motion: bool, now: Instant) {
        if target == self.target {
            return;
        }
        let current = self.sample(now);
        let Some(spec) = spec.effective(reduce_motion) else {
            self.jump(target);
            return;
        };
        self.phase = if current.settled {
            Phase::Curve {
                from: current.value,
                start: now,
                duration: spec.duration,
                curve: spec.curve,
            }
        } else {
            spring_from(current, target, spec.duration, now)
        };
        self.target = target;
    }
}

fn spring_from(current: Sample, target: f32, duration: Duration, now: Instant) -> Phase {
    let offset = f64::from(current.value - target);
    let velocity = f64::from(current.velocity);
    let base = SETTLE_DECAY / duration.as_secs_f64();
    let toward = -velocity * offset.signum();
    let omega = if offset.abs() > f64::EPSILON && toward > 0.0 {
        base.max(toward / offset.abs())
    } else {
        base
    };
    let omega = omega as f32;
    Phase::Spring {
        state: SpringState {
            position: current.value,
            velocity: current.velocity,
        },
        at: now,
        config: SpringConfig::new(omega * omega, 2.0 * omega, 1.0),
        epsilon: (offset.abs() as f32).max(1.0) * EPSILON_FRACTION,
    }
}

#[cfg(test)]
mod tests {
    use super::super::curve::{EASE, EASE_OUT};
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    const SPEC: Spec = Spec::new(Duration::from_millis(320), EASE_OUT);

    #[test]
    fn a_new_motion_rests_at_its_value() {
        let m = Motion::at(0.5);
        let s = m.peek(Instant::now());
        assert_eq!(s.value, 0.5);
        assert!(s.settled);
    }

    #[test]
    fn an_uninterrupted_motion_follows_the_curve_exactly() {
        let t0 = Instant::now();
        let mut m = Motion::at(0.0);
        m.retarget(10.0, SPEC, false, t0);
        for i in 0..320 {
            let s = m.sample(t0 + ms(i));
            let want = 10.0 * EASE_OUT.value(i as f64 / 320.0);
            assert!((f64::from(s.value) - want).abs() < 1e-4, "at {i} ms");
        }
        let end = m.sample(t0 + ms(320));
        assert_eq!(end.value, 10.0);
        assert!(end.settled);
    }

    #[test]
    fn a_retarget_keeps_position_and_velocity() {
        let t0 = Instant::now();
        for at in [10, 40, 80, 160, 250, 300] {
            let mut m = Motion::at(0.0);
            m.retarget(1.0, SPEC, false, t0);
            let cut = t0 + ms(at);
            let before = m.peek(cut);
            m.retarget(0.0, SPEC, false, cut);
            let after = m.peek(cut);
            assert!(
                (before.value - after.value).abs() < 1e-6,
                "position at {at} ms"
            );
            assert!(
                (before.velocity - after.velocity).abs() < 1e-3 * before.velocity.abs().max(1.0),
                "velocity at {at} ms: {} vs {}",
                before.velocity,
                after.velocity
            );
        }
    }

    #[test]
    fn nothing_crosses_its_target() {
        let t0 = Instant::now();
        for at in [5, 30, 60, 120, 200, 310] {
            for (first, second) in [(1.0, 0.0), (1.0, 2.0), (1.0, 1.05), (1.0, 0.95), (1.0, 0.2)] {
                let mut m = Motion::at(0.0);
                m.retarget(first, SPEC, false, t0);
                let cut = t0 + ms(at);
                m.retarget(second, Spec::new(ms(200), EASE), false, cut);
                let side = m.peek(cut).value - second;
                for i in 0..2000 {
                    let v = m.sample(cut + ms(i)).value;
                    assert!(
                        (v - second) * side.signum() >= -1e-4,
                        "cut {at} ms toward {second}: {v} at {i} ms"
                    );
                }
                assert_eq!(m.sample(cut + ms(2000)).value, second);
            }
        }
    }

    #[test]
    fn reversing_turns_around_smoothly_and_settles() {
        let t0 = Instant::now();
        let mut m = Motion::at(0.0);
        m.retarget(1.0, SPEC, false, t0);
        m.retarget(0.0, SPEC, false, t0 + ms(100));
        let s = m.sample(t0 + ms(1000));
        assert_eq!(s.value, 0.0);
        assert!(s.settled);
    }

    #[test]
    fn a_settled_motion_starts_a_fresh_curve() {
        let t0 = Instant::now();
        let mut m = Motion::at(0.0);
        m.retarget(1.0, SPEC, false, t0);
        m.retarget(0.0, SPEC, false, t0 + ms(400));
        let mid = t0 + ms(400 + 160);
        let want = 1.0 - EASE_OUT.value(0.5);
        assert!((f64::from(m.sample(mid).value) - want).abs() < 1e-4);
    }

    #[test]
    fn reduced_motion_finishes_at_once() {
        let t0 = Instant::now();
        let mut m = Motion::at(0.0);
        m.retarget(1.0, SPEC, true, t0);
        let s = m.sample(t0);
        assert_eq!(s.value, 1.0);
        assert!(s.settled);
    }

    #[test]
    fn reduced_motion_keeps_a_fade_that_carries_meaning() {
        let t0 = Instant::now();
        let spec = SPEC.keep_when_reduced(ms(120));
        let mut m = Motion::at(0.0);
        m.retarget(1.0, spec, true, t0);
        let half = m.sample(t0 + ms(60));
        assert!(!half.settled);
        assert!((f64::from(half.value) - EASE_OUT.value(0.5)).abs() < 1e-4);
        assert!(m.sample(t0 + ms(120)).settled);
    }

    #[test]
    fn a_zero_duration_snaps() {
        let t0 = Instant::now();
        let mut m = Motion::at(0.0);
        m.retarget(4.0, Spec::new(Duration::ZERO, EASE), false, t0);
        assert_eq!(m.sample(t0).value, 4.0);
    }

    #[test]
    fn the_same_target_does_not_restart() {
        let t0 = Instant::now();
        let mut m = Motion::at(0.0);
        m.retarget(1.0, SPEC, false, t0);
        let before = m.peek(t0 + ms(100));
        m.retarget(1.0, SPEC, false, t0 + ms(100));
        assert_eq!(m.peek(t0 + ms(100)), before);
    }
}
