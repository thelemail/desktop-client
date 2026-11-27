use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::{App, ElementId, Window};

use super::driver::{Motion, Sample, Spec};

fn now(cx: &App) -> Instant {
    cx.background_executor().now()
}

fn drive(motion: &mut Motion, target: f32, spec: Spec, window: &mut Window, cx: &App) -> Sample {
    let now = now(cx);
    motion.retarget(target, spec, cx.reduce_motion(), now);
    let sample = motion.sample(now);
    if !sample.settled {
        window.request_animation_frame();
    }
    sample
}

pub fn animate(
    id: impl Into<ElementId>,
    target: f32,
    spec: Spec,
    window: &mut Window,
    cx: &mut App,
) -> Sample {
    animate_from(id, target, target, spec, window, cx)
}

pub fn animate_from(
    id: impl Into<ElementId>,
    initial: f32,
    target: f32,
    spec: Spec,
    window: &mut Window,
    cx: &mut App,
) -> Sample {
    let id = ElementId::NamedChild(id.into().into(), "motion".into());
    let state = window.use_keyed_state(id, cx, |_, _| Motion::at(initial));
    let mut motion = *state.read(cx);
    let sample = drive(&mut motion, target, spec, window, cx);
    state.update(cx, |state, _| *state = motion);
    sample
}

#[derive(Clone)]
pub struct MotionHandle {
    motion: Rc<RefCell<Motion>>,
}

impl MotionHandle {
    pub fn at(value: f32) -> Self {
        Self {
            motion: Rc::new(RefCell::new(Motion::at(value))),
        }
    }

    pub fn jump(&self, value: f32) {
        self.motion.borrow_mut().jump(value);
    }

    pub fn sample(&self, target: f32, spec: Spec, window: &mut Window, cx: &App) -> Sample {
        drive(&mut self.motion.borrow_mut(), target, spec, window, cx)
    }
}
