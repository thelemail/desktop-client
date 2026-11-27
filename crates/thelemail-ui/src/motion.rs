mod curve;
mod driver;

pub use curve::{BASE, Bezier, CUBIC_OUT, Curve, EASE, EASE_OUT, FAST, LINEAR, SLOW};
pub use driver::{Motion, Reduced, Sample, Spec};
