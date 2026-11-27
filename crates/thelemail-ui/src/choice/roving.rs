use std::rc::Rc;

use gpui_kit::base::LayoutDirection;
use gpui_kit::{App, ElementId, FocusHandle, KeyDownEvent, Window};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Next,
    Prev,
    First,
    Last,
}

pub fn nav(key: &str, rtl: bool) -> Option<Nav> {
    Some(match key {
        "right" if rtl => Nav::Prev,
        "left" if rtl => Nav::Next,
        "right" | "down" => Nav::Next,
        "left" | "up" => Nav::Prev,
        "home" => Nav::First,
        "end" => Nav::Last,
        _ => return None,
    })
}

pub fn step(current: Option<usize>, nav: Nav, enabled: &[bool], looped: bool) -> Option<usize> {
    let first = enabled.iter().position(|on| *on)?;
    let last = enabled.iter().rposition(|on| *on)?;
    let next = |from: usize| (from + 1..enabled.len()).find(|&ix| enabled[ix]);
    let prev = |from: usize| (0..from).rev().find(|&ix| enabled[ix]);
    let wrap = |end: usize, here: usize| if looped { end } else { here };
    Some(match (nav, current) {
        (Nav::First, _) | (Nav::Next, None) => first,
        (Nav::Last, _) | (Nav::Prev, None) => last,
        (Nav::Next, Some(here)) => next(here).unwrap_or_else(|| wrap(first, here)),
        (Nav::Prev, Some(here)) => prev(here).unwrap_or_else(|| wrap(last, here)),
    })
}

pub fn stop(selected: Option<usize>, enabled: &[bool]) -> Option<usize> {
    selected
        .filter(|&ix| enabled.get(ix).copied().unwrap_or(false))
        .or_else(|| enabled.iter().position(|on| *on))
}

pub(crate) type Pick = Rc<dyn Fn(usize, &mut Window, &mut App)>;

pub(crate) fn handles(
    id: &ElementId,
    count: usize,
    window: &mut Window,
    cx: &mut App,
) -> Vec<FocusHandle> {
    let state = window.use_keyed_state(
        ElementId::NamedChild(id.clone().into(), "roving".into()),
        cx,
        |_, _| Vec::<FocusHandle>::new(),
    );
    state.update(cx, |list, cx| {
        while list.len() < count {
            list.push(cx.focus_handle());
        }
        list.truncate(count);
        list.clone()
    })
}

pub(crate) fn on_key(
    handles: Vec<FocusHandle>,
    enabled: Vec<bool>,
    looped: bool,
    pick: Pick,
) -> impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static {
    move |event, window, cx| {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let rtl = LayoutDirection::global(cx).is_rtl();
        let Some(nav) = nav(&event.keystroke.key, rtl) else {
            return;
        };
        let Some(current) = handles.iter().position(|h| h.is_focused(window)) else {
            return;
        };
        let Some(target) = step(Some(current), nav, &enabled, looped) else {
            return;
        };
        cx.stop_propagation();
        if target == current {
            return;
        }
        window.focus(&handles[target], cx);
        pick(target, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [bool; 4] = [true, true, true, true];
    const GAPS: [bool; 5] = [false, true, false, true, false];

    #[test]
    fn arrows_on_both_axes_mirror_only_the_horizontal_one() {
        assert_eq!(nav("right", false), Some(Nav::Next));
        assert_eq!(nav("down", false), Some(Nav::Next));
        assert_eq!(nav("left", false), Some(Nav::Prev));
        assert_eq!(nav("up", false), Some(Nav::Prev));
        assert_eq!(nav("right", true), Some(Nav::Prev));
        assert_eq!(nav("left", true), Some(Nav::Next));
        assert_eq!(nav("down", true), Some(Nav::Next));
        assert_eq!(nav("home", true), Some(Nav::First));
        assert_eq!(nav("end", false), Some(Nav::Last));
        assert_eq!(nav("enter", false), None);
        assert_eq!(nav("space", false), None);
    }

    #[test]
    fn a_looped_group_wraps_at_both_ends() {
        assert_eq!(step(Some(3), Nav::Next, &ALL, true), Some(0));
        assert_eq!(step(Some(0), Nav::Prev, &ALL, true), Some(3));
        assert_eq!(step(Some(1), Nav::Next, &ALL, true), Some(2));
    }

    #[test]
    fn an_open_group_stops_at_its_ends() {
        assert_eq!(step(Some(3), Nav::Next, &ALL, false), Some(3));
        assert_eq!(step(Some(0), Nav::Prev, &ALL, false), Some(0));
    }

    #[test]
    fn disabled_items_are_skipped() {
        assert_eq!(step(Some(1), Nav::Next, &GAPS, true), Some(3));
        assert_eq!(step(Some(3), Nav::Next, &GAPS, true), Some(1));
        assert_eq!(step(Some(1), Nav::Prev, &GAPS, true), Some(3));
        assert_eq!(step(None, Nav::First, &GAPS, false), Some(1));
        assert_eq!(step(None, Nav::Last, &GAPS, false), Some(3));
    }

    #[test]
    fn nothing_is_reachable_when_everything_is_disabled() {
        assert_eq!(step(Some(0), Nav::Next, &[false, false], true), None);
        assert_eq!(stop(None, &[false, false]), None);
    }

    #[test]
    fn an_empty_selection_starts_from_the_nearest_end() {
        assert_eq!(step(None, Nav::Next, &ALL, true), Some(0));
        assert_eq!(step(None, Nav::Prev, &ALL, true), Some(3));
    }

    #[test]
    fn the_tab_stop_is_the_selection_or_the_first_enabled_item() {
        assert_eq!(stop(Some(2), &ALL), Some(2));
        assert_eq!(stop(None, &GAPS), Some(1));
        assert_eq!(stop(Some(2), &GAPS), Some(1));
        assert_eq!(stop(Some(9), &ALL), Some(0));
    }
}
