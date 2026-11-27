use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::{
    IterationCount, Keyframe, Keyframes, MotionReveal, Timing, animate_keyframes,
};
use gpui_kit::{
    AnyElement, App, ElementId, IntoElement, ParentElement, Pixels, Styled, Window, div, px,
};

use super::curve::{BASE, CUBIC_OUT, Curve, EASE, EASE_OUT, FAST, LINEAR, SLOW};
use super::driver::Spec;
use super::live::{MotionHandle, animate_from};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Offset {
    Px(f32),
    Height(f32),
}

impl Offset {
    const fn amount(self) -> f32 {
        match self {
            Self::Px(v) | Self::Height(v) => v,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Toggle {
    pub enter: Spec,
    pub exit: Spec,
    pub rise: Offset,
    pub sink: Offset,
    pub hidden_opacity: f32,
    pub kept_fade: Option<Duration>,
}

impl Toggle {
    const fn new(enter: Spec, exit: Spec, rise: Offset, sink: Offset) -> Self {
        Self {
            enter,
            exit,
            rise,
            sink,
            hidden_opacity: 0.0,
            kept_fade: None,
        }
    }

    const fn entrance(spec: Spec, rise: f32) -> Self {
        Self::new(spec, spec, Offset::Px(rise), Offset::Px(rise))
    }

    const fn hidden_at(mut self, opacity: f32) -> Self {
        self.hidden_opacity = opacity;
        self
    }

    const fn keep_fade(mut self, duration: Duration) -> Self {
        self.kept_fade = Some(duration);
        self
    }

    pub fn curves(&self) -> [Curve; 2] {
        [self.enter.curve, self.exit.curve]
    }

    fn fade_spec(&self, spec: Spec) -> Spec {
        match self.kept_fade {
            Some(duration) => spec.keep_when_reduced(duration),
            None => spec,
        }
    }

    pub fn sample(
        &self,
        id: impl Into<ElementId>,
        open: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Look {
        let id = ElementId::NamedChild(id.into().into(), "toggle".into());
        let state = window
            .use_keyed_state(id, cx, |_, _| ToggleHandle::new(self))
            .read(cx)
            .clone();
        self.sample_handle(&state, open, window, cx)
    }

    pub fn sample_handle(
        &self,
        handle: &ToggleHandle,
        open: bool,
        window: &mut Window,
        cx: &App,
    ) -> Look {
        if !handle.shown.get() {
            if open {
                handle.fade.jump(self.hidden_opacity);
                handle.offset.jump(self.rise.amount());
            } else {
                handle.fade.jump(self.hidden_opacity);
                handle.offset.jump(self.sink.amount());
            }
        }
        let (spec, fade_to, offset_to, unit) = if open {
            (self.enter, 1.0, 0.0, self.rise)
        } else {
            (
                self.exit,
                self.hidden_opacity,
                self.sink.amount(),
                self.sink,
            )
        };
        let fade = handle
            .fade
            .sample(fade_to, self.fade_spec(spec), window, cx);
        let offset = handle.offset.sample(offset_to, spec, window, cx);
        let settled = fade.settled && offset.settled;
        handle.shown.set(open || !settled);
        Look {
            opacity: fade.value,
            offset: match unit {
                Offset::Px(_) => Offset::Px(offset.value),
                Offset::Height(_) => Offset::Height(offset.value),
            },
            present: open || !settled,
            settled,
        }
    }
}

#[derive(Clone)]
pub struct ToggleHandle {
    fade: MotionHandle,
    offset: MotionHandle,
    shown: Rc<Cell<bool>>,
}

impl ToggleHandle {
    pub fn new(toggle: &Toggle) -> Self {
        Self {
            fade: MotionHandle::at(toggle.hidden_opacity),
            offset: MotionHandle::at(toggle.sink.amount()),
            shown: Rc::new(Cell::new(false)),
        }
    }

    pub fn shown() -> Self {
        Self {
            fade: MotionHandle::at(1.0),
            offset: MotionHandle::at(0.0),
            shown: Rc::new(Cell::new(true)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub opacity: f32,
    pub offset: Offset,
    pub present: bool,
    pub settled: bool,
}

impl Look {
    pub fn flash(&self) -> f32 {
        (1.0 - self.opacity).clamp(0.0, 1.0)
    }

    pub fn y(&self, height: Pixels) -> Pixels {
        match self.offset {
            Offset::Px(v) => px(v),
            Offset::Height(f) => height * f,
        }
    }
}

pub trait Moving: Styled + Sized {
    fn look(self, look: &Look) -> Self {
        self.look_within(look, px(0.0))
    }

    fn look_within(self, look: &Look, height: Pixels) -> Self {
        self.relative().top(look.y(height)).opacity(look.opacity)
    }
}

impl<E: Styled> Moving for E {}

pub const CARD_IN: Toggle = Toggle::entrance(Spec::new(SLOW, EASE_OUT), 10.0);

pub const DIALOG: Toggle = Toggle::new(
    Spec::new(SLOW, EASE_OUT),
    Spec::new(FAST, EASE),
    Offset::Px(8.0),
    Offset::Px(4.0),
);

pub const SHEET: Toggle = Toggle::new(
    Spec::new(SLOW, EASE_OUT),
    Spec::new(BASE, EASE),
    Offset::Height(1.0),
    Offset::Height(1.0),
)
.hidden_at(1.0);

pub const MENU: Toggle = Toggle::new(
    Spec::new(FAST, EASE_OUT),
    Spec::new(FAST, EASE),
    Offset::Px(-4.0),
    Offset::Px(0.0),
);

pub const TOOLTIP: Toggle = Toggle::new(
    Spec::new(FAST, EASE_OUT),
    Spec::new(FAST, EASE),
    Offset::Px(3.0),
    Offset::Px(0.0),
);

pub const OVERLAY: Toggle = Toggle::new(
    Spec::new(BASE, EASE),
    Spec::new(FAST, EASE),
    Offset::Px(0.0),
    Offset::Px(0.0),
);

pub const ROW_ARRIVE: Toggle = Toggle::entrance(Spec::new(SLOW, EASE_OUT), -8.0).keep_fade(SLOW);

pub const READER_CONTENT_IN: Toggle =
    Toggle::entrance(Spec::new(BASE, EASE_OUT), 5.0).keep_fade(FAST);

pub const COMPOSE_IN: Toggle = Toggle::entrance(Spec::new(SLOW, EASE_OUT), 22.0).hidden_at(0.4);

pub const CROSSFADE: Toggle = Toggle::entrance(Spec::new(BASE, EASE_OUT), 0.0);

pub const TOAST_IN: Toggle = Toggle::entrance(Spec::new(SLOW, EASE_OUT), 12.0);

pub const UNFOLD: Spec = Spec::new(Duration::from_millis(260), CUBIC_OUT);

pub const SHIMMER_PERIOD: Duration = Duration::from_millis(1400);

pub const NAMED: [(&str, Toggle); 11] = [
    ("card_in", CARD_IN),
    ("dialog", DIALOG),
    ("sheet", SHEET),
    ("menu", MENU),
    ("tooltip", TOOLTIP),
    ("overlay", OVERLAY),
    ("row_arrive", ROW_ARRIVE),
    ("reader_content_in", READER_CONTENT_IN),
    ("compose_in", COMPOSE_IN),
    ("crossfade", CROSSFADE),
    ("toast_in", TOAST_IN),
];

fn reveal(id: ElementId, progress: f32, opacity: f32, child: impl IntoElement) -> AnyElement {
    MotionReveal::new(
        id,
        progress,
        div().opacity(opacity).child(child).into_any_element(),
    )
    .into_any_element()
}

pub fn unfold(
    id: impl Into<ElementId>,
    open: bool,
    child: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let id: ElementId = id.into();
    let target = if open { 1.0 } else { 0.0 };
    let sample = animate_from(id.clone(), 0.0, target, UNFOLD, window, cx);
    if !open && sample.settled {
        return None;
    }
    let progress = sample.value.clamp(0.0, 1.0);
    Some(reveal(id, progress, progress, child))
}

pub fn height_auto(
    id: impl Into<ElementId>,
    child: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let id: ElementId = id.into();
    let sample = animate_from(id.clone(), 0.0, 1.0, UNFOLD, window, cx);
    reveal(id, sample.value.clamp(0.0, 1.0), 1.0, child)
}

pub fn shimmer(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> f32 {
    let keyframes = Keyframes::try_new([Keyframe::new(0.0, 0.0_f32), Keyframe::new(1.0, 1.0)])
        .expect("two endpoint keyframes");
    let timing = Timing::new(SHIMMER_PERIOD)
        .iterations(IterationCount::Infinite)
        .ease(LINEAR.easing());
    animate_keyframes(id.into(), &keyframes, timing, window, cx).value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_named_motion_can_bounce_or_overshoot() {
        for (name, toggle) in NAMED {
            for curve in toggle.curves() {
                assert!(curve.stays_in_range(), "{name} uses {curve:?}");
            }
        }
        assert!(UNFOLD.curve.stays_in_range());
    }

    #[test]
    fn web_timings_are_kept() {
        assert_eq!(CARD_IN.enter.duration, SLOW);
        assert_eq!(DIALOG.enter.duration, SLOW);
        assert_eq!(DIALOG.exit.duration, FAST);
        assert_eq!(SHEET.exit.duration, BASE);
        assert_eq!(MENU.enter.duration, FAST);
        assert_eq!(OVERLAY.enter.duration, BASE);
        assert_eq!(UNFOLD.duration, Duration::from_millis(260));
        assert_eq!(ROW_ARRIVE.kept_fade, Some(SLOW));
        assert_eq!(READER_CONTENT_IN.kept_fade, Some(FAST));
        assert!(NAMED.iter().all(|(name, toggle)| toggle.kept_fade.is_none()
            || matches!(*name, "row_arrive" | "reader_content_in")));
    }
}
