mod color;
mod curve;
mod driver;
mod live;
mod named;

pub use color::{ease_color, mix};
pub use curve::{BASE, Bezier, CUBIC_OUT, Curve, EASE, EASE_OUT, FAST, LINEAR, SLOW};
pub use driver::{Motion, Reduced, Sample, Spec};
pub use live::{MotionHandle, animate, animate_from};
pub use named::{
    CARD_IN, COMPOSE_IN, CROSSFADE, DIALOG, Look, MENU, Moving, NAMED, OVERLAY, Offset, PULSE_DIM,
    PULSE_PERIOD, READER_CONTENT_IN, ROW_ARRIVE, SHEET, SHIMMER_PERIOD, TOAST_IN, TOOLTIP, Toggle,
    ToggleHandle, UNFOLD, height_auto, pulse, shimmer, unfold,
};
