use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Div, ElementId, FocusHandle, Hsla, InteractiveElement, IntoElement, ParentElement,
    PathBuilder, RenderOnce, Role, SharedString, Stateful, StatefulInteractiveElement, Styled,
    Toggled, Window, accesskit, canvas, div, point, px, relative,
};

use super::look::{self, DISABLED_OPACITY, Mark};
use super::roving::{self, Pick};
use super::{Handler, child, focus, is_enter, ring};
use crate::a11y::A11yStateExt as _;
use crate::motion::{EASE, EASE_OUT, FAST, Spec, animate, ease_color};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_SANS, RADIUS_CHECK};

const BOX: f32 = 18.0;
const BORDER: f32 = 1.5;
const TICK: f32 = 10.0;
const BAR_WIDTH: f32 = 8.4;
const BAR_HEIGHT: f32 = 2.0;
const DOT: f32 = 8.0;
const REACH: f32 = 4.0;
const LABEL_GAP: f32 = 8.0;
const LABEL_FONT: f32 = 14.0;
const GROUP_GAP: f32 = 12.0;
const COLOR: Spec = Spec::new(FAST, EASE);
const GROW: Spec = Spec::new(FAST, EASE_OUT);
const TICK_SHAPE: [(f32, f32); 6] = [
    (0.14, 0.52),
    (0.0, 0.66),
    (0.38, 1.0),
    (1.0, 0.22),
    (0.86, 0.08),
    (0.38, 0.72),
];

pub fn next_mark(mark: Mark) -> Mark {
    match mark {
        Mark::On => Mark::Off,
        Mark::Off | Mark::Mixed => Mark::On,
    }
}

fn tick(scale: f32, color: Hsla) -> impl IntoElement {
    div().flex_none().size(px(TICK)).child(
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                if scale <= f32::EPSILON {
                    return;
                }
                let center = bounds.center();
                let side = bounds.size.width;
                let at = |(x, y): (f32, f32)| {
                    point(
                        center.x + side * ((x - 0.5) * scale),
                        center.y + side * ((y - 0.5) * scale),
                    )
                };
                let mut path = PathBuilder::fill();
                path.move_to(at(TICK_SHAPE[0]));
                for corner in &TICK_SHAPE[1..] {
                    path.line_to(at(*corner));
                }
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            },
        )
        .size_full(),
    )
}

struct Chrome {
    id: ElementId,
    label: Option<SharedString>,
    disabled: bool,
    theme: Theme,
}

impl Chrome {
    fn wrap(
        self,
        control: Div,
        handle: &FocusHandle,
        tab_stop: bool,
        window: &Window,
    ) -> Stateful<Div> {
        let focused = handle.is_focused(window);
        let base = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .when(!self.disabled, |el| el.cursor_pointer())
            .when(self.disabled, |el| {
                el.cursor_not_allowed().opacity(DISABLED_OPACITY)
            })
            .when(!self.disabled || focused, |el| {
                el.track_focus(
                    &handle
                        .clone()
                        .tab_index(0)
                        .tab_stop(tab_stop && !self.disabled),
                )
            });
        match self.label {
            Some(label) => base
                .gap(px(LABEL_GAP))
                .aria_label(label.clone())
                .child(control)
                .child(
                    div()
                        .font_family(FONT_SANS.name)
                        .text_size(px(LABEL_FONT * FONT_SANS.size_adjust))
                        .line_height(relative(1.3))
                        .text_color(self.theme.color(ColorToken::Fg))
                        .child(label),
                ),
            None => base.m(px(-REACH)).p(px(REACH)).child(control),
        }
    }
}

#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    mark: Mark,
    label: Option<SharedString>,
    accessibility_label: Option<SharedString>,
    disabled: bool,
    invalid: bool,
    theme: Option<Theme>,
    on_change: Option<Handler<Mark>>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            mark: Mark::Off,
            label: None,
            accessibility_label: None,
            disabled: false,
            invalid: false,
            theme: None,
            on_change: None,
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.mark = if checked { Mark::On } else { Mark::Off };
        self
    }

    pub fn mark(mut self, mark: Mark) -> Self {
        self.mark = mark;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn on_change(mut self, handler: impl Fn(Mark, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let paint = look::checkbox(self.mark, self.invalid);
        let bg = ease_color(
            child(&self.id, "bg"),
            paint.bg.resolve(&theme),
            COLOR,
            window,
            cx,
        );
        let border = ease_color(
            child(&self.id, "border"),
            paint.border.resolve(&theme),
            COLOR,
            window,
            cx,
        );
        let ink = paint.fg.resolve(&theme);
        let shown = self.mark != Mark::Off;
        let scale = animate(
            child(&self.id, "mark"),
            if shown { 1.0 } else { 0.0 },
            GROW,
            window,
            cx,
        )
        .value
        .max(0.0);
        let mixed = window.use_keyed_state(child(&self.id, "mixed"), cx, |_, _| false);
        if shown {
            mixed.update(cx, |was, _| *was = self.mark == Mark::Mixed);
        }
        let as_bar = *mixed.read(cx);
        let handle = focus(&self.id, window, cx);
        let keyboard = handle.is_focused(window) && window.last_input_was_keyboard();
        let ring_color = theme.color(ColorToken::FocusRing);

        let control = div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(BOX))
            .rounded(px(RADIUS_CHECK))
            .border(px(BORDER))
            .border_color(border)
            .bg(bg)
            .map(|el| {
                if as_bar {
                    el.child(
                        div()
                            .flex_none()
                            .w(px(BAR_WIDTH * scale))
                            .h(px(BAR_HEIGHT * scale))
                            .bg(ink),
                    )
                } else {
                    el.child(tick(scale, ink))
                }
            })
            .when(keyboard, |el| {
                el.child(ring(BORDER, RADIUS_CHECK, ring_color))
            });

        let name = self.accessibility_label.clone();
        let mark = self.mark;
        let disabled = self.disabled;
        let on_change = self.on_change.clone();
        Chrome {
            id: self.id,
            label: self.label,
            disabled,
            theme,
        }
        .wrap(control, &handle, true, window)
        .role(Role::CheckBox)
        .when_some(name, |el, name| el.aria_label(name))
        .aria_toggled(match mark {
            Mark::On => Toggled::True,
            Mark::Off => Toggled::False,
            Mark::Mixed => Toggled::Mixed,
        })
        .on_click(move |event, window, cx| {
            if disabled || is_enter(event) {
                return;
            }
            if let Some(on_change) = &on_change {
                on_change(next_mark(mark), window, cx);
            }
        })
        .a11y_state(false, disabled)
        .invalid(self.invalid)
    }
}

#[derive(IntoElement)]
pub struct Radio {
    id: ElementId,
    checked: bool,
    label: Option<SharedString>,
    accessibility_label: Option<SharedString>,
    disabled: bool,
    invalid: bool,
    theme: Option<Theme>,
    on_select: Option<Handler<()>>,
    focus: Option<(FocusHandle, bool)>,
    position: Option<(usize, usize)>,
}

impl Radio {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            checked: false,
            label: None,
            accessibility_label: None,
            disabled: false,
            invalid: false,
            theme: None,
            on_select: None,
            focus: None,
            position: None,
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn on_select(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(move |(), window, cx| handler(window, cx)));
        self
    }

    fn roving(mut self, handle: FocusHandle, tab_stop: bool, position: (usize, usize)) -> Self {
        self.focus = Some((handle, tab_stop));
        self.position = Some(position);
        self
    }
}

impl RenderOnce for Radio {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let paint = look::radio(self.checked, self.invalid);
        let border = ease_color(
            child(&self.id, "border"),
            paint.border.resolve(&theme),
            COLOR,
            window,
            cx,
        );
        let scale = animate(
            child(&self.id, "dot"),
            if self.checked { 1.0 } else { 0.0 },
            GROW,
            window,
            cx,
        )
        .value
        .max(0.0);
        let (handle, tab_stop) = match self.focus.clone() {
            Some(pair) => pair,
            None => (focus(&self.id, window, cx), true),
        };
        let keyboard = handle.is_focused(window) && window.last_input_was_keyboard();
        let ring_color = theme.color(ColorToken::FocusRing);

        let control = div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(BOX))
            .rounded_full()
            .border(px(BORDER))
            .border_color(border)
            .bg(paint.bg.resolve(&theme))
            .child(
                div()
                    .flex_none()
                    .size(px(DOT * scale))
                    .rounded_full()
                    .bg(paint.fg.resolve(&theme)),
            )
            .when(keyboard, |el| el.child(ring(BORDER, BOX / 2.0, ring_color)));

        let name = self.accessibility_label.clone();
        let checked = self.checked;
        let disabled = self.disabled;
        let on_select = self.on_select.clone();
        let focus_on_click = handle.clone();
        Chrome {
            id: self.id,
            label: self.label,
            disabled,
            theme,
        }
        .wrap(control, &handle, tab_stop, window)
        .role(Role::RadioButton)
        .when_some(name, |el, name| el.aria_label(name))
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .aria_selected(checked)
        .when_some(self.position, |el, (ix, count)| {
            el.aria_position_in_set(ix + 1).aria_size_of_set(count)
        })
        .on_click(move |event, window, cx| {
            if disabled || is_enter(event) {
                return;
            }
            window.focus(&focus_on_click, cx);
            if !checked && let Some(on_select) = &on_select {
                on_select((), window, cx);
            }
        })
        .a11y_state(false, disabled)
        .invalid(self.invalid)
    }
}

#[derive(Debug, Clone)]
pub struct RadioItem {
    value: SharedString,
    label: SharedString,
    disabled: bool,
}

impl RadioItem {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            disabled: false,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(IntoElement)]
pub struct RadioGroup {
    id: ElementId,
    items: Vec<RadioItem>,
    value: Option<SharedString>,
    label: Option<SharedString>,
    disabled: bool,
    invalid: bool,
    theme: Option<Theme>,
    on_change: Option<Handler<SharedString>>,
}

impl RadioGroup {
    pub fn new(id: impl Into<ElementId>, items: Vec<RadioItem>) -> Self {
        Self {
            id: id.into(),
            items,
            value: None,
            label: None,
            disabled: false,
            invalid: false,
            theme: None,
            on_change: None,
        }
    }

    pub fn value(mut self, value: Option<impl Into<SharedString>>) -> Self {
        self.value = value.map(Into::into);
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn on_change(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for RadioGroup {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let count = self.items.len();
        let selected = self
            .items
            .iter()
            .position(|item| self.value.as_ref() == Some(&item.value));
        let enabled: Vec<bool> = self
            .items
            .iter()
            .map(|item| !item.disabled && !self.disabled)
            .collect();
        let handles = roving::handles(&self.id, count, window, cx);
        let stop = roving::stop(selected, &enabled);
        let values: Vec<SharedString> = self.items.iter().map(|i| i.value.clone()).collect();
        let on_change = self.on_change.clone();
        let pick: Pick = Rc::new(move |ix, window, cx| {
            if let (Some(on_change), Some(value)) = (&on_change, values.get(ix)) {
                on_change(value.clone(), window, cx);
            }
        });
        let radios = self
            .items
            .into_iter()
            .enumerate()
            .map(|(ix, item)| {
                let pick = pick.clone();
                Radio::new(ElementId::NamedChild(
                    self.id.clone().into(),
                    item.value.clone(),
                ))
                .checked(selected == Some(ix))
                .label(item.label)
                .disabled(!enabled[ix])
                .invalid(self.invalid)
                .theme(theme)
                .roving(handles[ix].clone(), stop == Some(ix), (ix, count))
                .on_select(move |window, cx| pick(ix, window, cx))
            })
            .collect::<Vec<_>>();
        div()
            .id(self.id.clone())
            .flex()
            .flex_col()
            .items_start()
            .gap(px(GROUP_GAP))
            .role(Role::RadioGroup)
            .aria_orientation(accesskit::Orientation::Vertical)
            .when_some(self.label, |el, label| el.aria_label(label))
            .on_key_down(roving::on_key(handles, enabled, false, pick))
            .children(radios)
            .a11y_state(false, self.disabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_on_a_mixed_box_checks_it() {
        assert_eq!(next_mark(Mark::Mixed), Mark::On);
        assert_eq!(next_mark(Mark::On), Mark::Off);
        assert_eq!(next_mark(Mark::Off), Mark::On);
    }
}
