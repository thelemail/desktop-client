use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::{App, ElementId, FocusHandle, Window};

#[derive(Clone)]
pub(crate) struct Return {
    open: Rc<Cell<bool>>,
    previous: Rc<RefCell<Option<FocusHandle>>>,
}

pub(crate) fn handle(
    id: &ElementId,
    name: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> FocusHandle {
    window
        .use_keyed_state(
            ElementId::NamedChild(id.clone().into(), name.into()),
            cx,
            |_, cx| cx.focus_handle(),
        )
        .read(cx)
        .clone()
}

pub(crate) fn follow(
    id: &ElementId,
    open: bool,
    container: &FocusHandle,
    initial: Option<FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) {
    follow_to(id, open, container, initial, None, window, cx);
}

pub(crate) fn follow_to(
    id: &ElementId,
    open: bool,
    container: &FocusHandle,
    initial: Option<FocusHandle>,
    return_to: Option<FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) {
    let state = window
        .use_keyed_state(
            ElementId::NamedChild(id.clone().into(), "focus-return".into()),
            cx,
            |_, _| Return {
                open: Rc::new(Cell::new(false)),
                previous: Rc::new(RefCell::new(None)),
            },
        )
        .read(cx)
        .clone();
    if state.open.get() == open {
        return;
    }
    state.open.set(open);
    if open {
        let previous = return_to.or_else(|| {
            window
                .focused(cx)
                .filter(|focused| !container.contains(focused, window))
        });
        *state.previous.borrow_mut() = previous;
        let target = initial.unwrap_or_else(|| container.clone());
        window.defer(cx, move |window, cx| target.focus(window, cx));
    } else {
        let previous = state.previous.borrow_mut().take();
        let container = container.clone();
        window.defer(cx, move |window, cx| {
            let lost = window
                .focused(cx)
                .is_none_or(|focused| container.contains(&focused, window));
            if lost && let Some(previous) = previous {
                previous.focus(window, cx);
            }
        });
    }
}
