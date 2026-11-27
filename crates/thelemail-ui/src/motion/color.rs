use gpui_kit::base::animation::Lerp;
use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::{App, ElementId, Hsla, Rgba, Window};

use super::driver::Spec;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Paint {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl Paint {
    fn of(color: Hsla) -> Self {
        let rgba = Rgba::from(color);
        Self {
            r: rgba.r * rgba.a,
            g: rgba.g * rgba.a,
            b: rgba.b * rgba.a,
            a: rgba.a,
        }
    }

    fn color(self) -> Hsla {
        if self.a <= f32::EPSILON {
            return Hsla::transparent_black();
        }
        Rgba {
            r: self.r / self.a,
            g: self.g / self.a,
            b: self.b / self.a,
            a: self.a,
        }
        .into()
    }
}

impl Lerp for Paint {
    fn lerp(&self, target: &Self, t: f32) -> Self {
        Self {
            r: self.r + (target.r - self.r) * t,
            g: self.g + (target.g - self.g) * t,
            b: self.b + (target.b - self.b) * t,
            a: self.a + (target.a - self.a) * t,
        }
    }
}

pub fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    Paint::of(from)
        .lerp(&Paint::of(to), t.clamp(0.0, 1.0))
        .color()
}

pub fn ease_color(
    id: impl Into<ElementId>,
    target: Hsla,
    spec: Spec,
    window: &mut Window,
    cx: &mut App,
) -> Hsla {
    let policy = Transition::new(spec.duration).easing(spec.curve.easing());
    transition(id.into(), Paint::of(target), policy, window, cx).color()
}

#[cfg(test)]
mod tests {
    use gpui_kit::hsla;

    use super::*;

    #[test]
    fn mixing_with_transparent_keeps_the_hue() {
        let green = hsla(0.4, 0.8, 0.3, 1.0);
        let half = Rgba::from(mix(Hsla::transparent_black(), green, 0.5));
        let full = Rgba::from(green);
        assert!((half.a - 0.5).abs() < 1e-4);
        assert!((half.r - full.r).abs() < 1e-4);
        assert!((half.g - full.g).abs() < 1e-4);
        assert!((half.b - full.b).abs() < 1e-4);
    }

    #[test]
    fn the_ends_are_exact() {
        let a = hsla(0.1, 0.5, 0.5, 1.0);
        let b = hsla(0.7, 0.5, 0.2, 0.6);
        assert_eq!(Rgba::from(mix(a, b, 0.0)), Rgba::from(a));
        let end = Rgba::from(mix(a, b, 1.0));
        let want = Rgba::from(b);
        assert!((end.r - want.r).abs() < 1e-4 && (end.a - want.a).abs() < 1e-4);
    }
}
