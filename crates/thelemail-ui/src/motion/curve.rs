use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::Easing;

pub const FAST: Duration = Duration::from_millis(120);
pub const BASE: Duration = Duration::from_millis(200);
pub const SLOW: Duration = Duration::from_millis(320);

const SOLVE_EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bezier {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl Bezier {
    pub const fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        Self { x1, y1, x2, y2 }
    }

    fn coefficients(a: f64, b: f64) -> (f64, f64, f64) {
        let c = 3.0 * a;
        let b = 3.0 * (b - a) - c;
        (1.0 - c - b, b, c)
    }

    fn x(&self, s: f64) -> f64 {
        let (a, b, c) = Self::coefficients(self.x1, self.x2);
        ((a * s + b) * s + c) * s
    }

    fn y(&self, s: f64) -> f64 {
        let (a, b, c) = Self::coefficients(self.y1, self.y2);
        ((a * s + b) * s + c) * s
    }

    fn dx(&self, s: f64) -> f64 {
        let (a, b, c) = Self::coefficients(self.x1, self.x2);
        (3.0 * a * s + 2.0 * b) * s + c
    }

    fn dy(&self, s: f64) -> f64 {
        let (a, b, c) = Self::coefficients(self.y1, self.y2);
        (3.0 * a * s + 2.0 * b) * s + c
    }

    fn solve(&self, x: f64) -> f64 {
        let mut s = x;
        for _ in 0..8 {
            let error = self.x(s) - x;
            if error.abs() < SOLVE_EPSILON {
                return s;
            }
            let slope = self.dx(s);
            if slope.abs() < 1e-7 {
                break;
            }
            s -= error / slope;
        }
        let (mut low, mut high) = (0.0, 1.0);
        s = x;
        for _ in 0..64 {
            let error = self.x(s) - x;
            if error.abs() < SOLVE_EPSILON {
                break;
            }
            if error < 0.0 {
                low = s;
            } else {
                high = s;
            }
            s = (low + high) / 2.0;
        }
        s
    }

    pub fn value(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return 0.0;
        }
        if t >= 1.0 {
            return 1.0;
        }
        self.y(self.solve(t))
    }

    pub fn slope(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        let s = self.solve(t);
        let dx = self.dx(s);
        if dx.abs() < 1e-12 {
            return if self.dy(s).abs() < 1e-12 {
                0.0
            } else {
                f64::INFINITY
            };
        }
        self.dy(s) / dx
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Curve {
    Linear,
    CubicOut,
    Bezier(Bezier),
}

pub const EASE: Curve = Curve::Bezier(Bezier::new(0.2, 0.6, 0.2, 1.0));
pub const EASE_OUT: Curve = Curve::Bezier(Bezier::new(0.16, 1.0, 0.3, 1.0));
pub const CUBIC_OUT: Curve = Curve::CubicOut;
pub const LINEAR: Curve = Curve::Linear;

impl Curve {
    pub fn value(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::CubicOut => 1.0 - (1.0 - t).powi(3),
            Self::Bezier(b) => b.value(t),
        }
    }

    pub fn slope(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => 1.0,
            Self::CubicOut => 3.0 * (1.0 - t).powi(2),
            Self::Bezier(b) => b.slope(t),
        }
    }

    pub fn easing(self) -> Easing {
        match self {
            Self::Linear => Easing::Linear,
            curve => Easing::Custom(Rc::new(move |t| curve.value(f64::from(t)) as f32)),
        }
    }

    pub fn stays_in_range(&self) -> bool {
        match self {
            Self::Linear | Self::CubicOut => true,
            Self::Bezier(b) => [b.x1, b.y1, b.x2, b.y2]
                .iter()
                .all(|v| (0.0..=1.0).contains(v)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSS_EASE: Curve = Curve::Bezier(Bezier::new(0.25, 0.1, 0.25, 1.0));
    const POINTS: [f64; 5] = [0.1, 0.25, 0.5, 0.75, 0.9];

    fn assert_points(curve: Curve, expected: [f64; 5]) {
        for (t, want) in POINTS.into_iter().zip(expected) {
            let got = curve.value(t);
            assert!(
                (got - want).abs() < 1e-7,
                "value({t}) = {got}, expected {want}"
            );
        }
    }

    #[test]
    fn ease_matches_reference_points() {
        assert_points(
            EASE,
            [
                0.3215519812,
                0.6874624915,
                0.9130781798,
                0.9838278866,
                0.9977429842,
            ],
        );
    }

    #[test]
    fn ease_out_matches_reference_points() {
        assert_points(
            EASE_OUT,
            [
                0.4943906171,
                0.8256223012,
                0.9717791053,
                0.9976770671,
                0.9998783478,
            ],
        );
    }

    #[test]
    fn css_ease_matches_webkit() {
        assert_points(
            CSS_EASE,
            [
                0.0947963057,
                0.4085105914,
                0.8024033876,
                0.9604589783,
                0.9943164775,
            ],
        );
    }

    #[test]
    fn chromium_reference_curve_holds() {
        let curve = Curve::Bezier(Bezier::new(0.25, 0.0, 0.75, 1.0));
        for (t, want) in [
            (0.05, 0.01136),
            (0.1, 0.03978),
            (0.2, 0.12803),
            (0.3, 0.24115),
            (0.4, 0.36761),
            (0.5, 0.5),
            (0.6, 0.63238),
            (0.7, 0.75884),
            (0.8, 0.87196),
            (0.9, 0.96021),
            (0.95, 0.98863),
        ] {
            assert!((curve.value(t) - want).abs() < 0.00015, "value({t})");
        }
    }

    #[test]
    fn cubic_out_matches_svelte() {
        for t in POINTS {
            let f = t - 1.0;
            assert!((CUBIC_OUT.value(t) - (f * f * f + 1.0)).abs() < 1e-12);
        }
    }

    #[test]
    fn every_curve_runs_from_zero_to_one() {
        for curve in [EASE, EASE_OUT, CUBIC_OUT, LINEAR, CSS_EASE] {
            assert_eq!(curve.value(0.0), 0.0);
            assert_eq!(curve.value(1.0), 1.0);
            assert_eq!(curve.value(-0.5), 0.0);
            assert_eq!(curve.value(1.5), 1.0);
        }
    }

    #[test]
    fn every_curve_is_monotonic() {
        for curve in [EASE, EASE_OUT, CUBIC_OUT, LINEAR] {
            let mut last = 0.0;
            for i in 0..=10_000 {
                let v = curve.value(f64::from(i) / 10_000.0);
                assert!(v >= last - 1e-12, "{curve:?} dips at {i}");
                assert!(v <= 1.0);
                last = v;
            }
        }
    }

    #[test]
    fn slope_agrees_with_finite_differences() {
        let h = 1e-6;
        for curve in [EASE, EASE_OUT, CUBIC_OUT, LINEAR, CSS_EASE] {
            for i in 1..100 {
                let t = f64::from(i) / 100.0;
                let numeric = (curve.value(t + h) - curve.value(t - h)) / (2.0 * h);
                let analytic = curve.slope(t);
                assert!(
                    (numeric - analytic).abs() < 1e-4 * analytic.max(1.0),
                    "{curve:?} slope({t}) = {analytic}, numeric {numeric}"
                );
            }
        }
    }

    #[test]
    fn kit_easing_samples_the_same_curve() {
        for curve in [EASE, EASE_OUT, CUBIC_OUT, LINEAR] {
            let easing = curve.easing();
            for t in POINTS {
                assert!((f64::from(easing.sample(t as f32)) - curve.value(t)).abs() < 1e-6);
            }
        }
    }
}
