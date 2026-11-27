use gpui_kit::base::Select as SelectRoot;
use gpui_kit::base::actions::{SelectDown, SelectUp};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, ElementId, EventEmitter, FocusHandle, FontWeight, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Render, Role, SharedString, StatefulInteractiveElement, Styled,
    Window, anchored, deferred, div, px, relative,
};

use super::{FieldFlags, FieldSize, Frame, frame_look, select_padding_end};
use crate::icons::{IconName, glyph};
use crate::motion::{MENU, Moving as _, ToggleHandle};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_SANS, RADIUS_MENU, RADIUS_SM, ShadowToken};

const CHEVRON: f32 = 12.0;
const CHEVRON_INSET: f32 = 11.0;
const MENU_GAP: f32 = 4.0;
const MENU_PAD: f32 = 4.0;
const OPTION_PAD_X: f32 = 10.0;
const OPTION_HEIGHT: f32 = 32.0;
const MIN_WIDTH: f32 = 148.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOption {
    pub value: SharedString,
    pub label: SharedString,
}

impl SelectOption {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectChanged {
    pub value: SharedString,
}

pub struct Select {
    id: ElementId,
    options: Vec<SelectOption>,
    selected: Option<usize>,
    highlighted: usize,
    open: bool,
    size: FieldSize,
    invalid: bool,
    disabled: bool,
    label: Option<SharedString>,
    placeholder: Option<SharedString>,
    trigger: FocusHandle,
    list: FocusHandle,
    motion: ToggleHandle,
    theme: Option<Theme>,
}

impl EventEmitter<SelectChanged> for Select {}

pub(crate) fn step(current: usize, len: usize, down: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if down {
        (current + 1).min(len - 1)
    } else {
        current.saturating_sub(1)
    }
}

impl Select {
    pub fn new(
        id: impl Into<ElementId>,
        options: Vec<SelectOption>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            id: id.into(),
            options,
            selected: None,
            highlighted: 0,
            open: false,
            size: FieldSize::default(),
            invalid: false,
            disabled: false,
            label: None,
            placeholder: None,
            trigger: cx.focus_handle(),
            list: cx.focus_handle(),
            motion: ToggleHandle::new(&MENU),
            theme: None,
        }
    }

    pub fn size(mut self, size: FieldSize) -> Self {
        self.size = size;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn selected_value(&self) -> Option<&SharedString> {
        self.selected
            .and_then(|ix| self.options.get(ix))
            .map(|option| &option.value)
    }

    pub fn set_selected_value(&mut self, value: &str, cx: &mut Context<Self>) {
        self.selected = self.options.iter().position(|o| o.value.as_ref() == value);
        cx.notify();
    }

    pub fn set_invalid(&mut self, invalid: bool, cx: &mut Context<Self>) {
        self.invalid = invalid;
        cx.notify();
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        if disabled {
            self.open = false;
        }
        cx.notify();
    }

    fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if open && !self.open {
            self.highlighted = self.selected.unwrap_or(0);
        }
        self.open = open && !self.disabled;
        cx.notify();
    }

    fn choose(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(option) = self.options.get(ix) else {
            return;
        };
        let changed = self.selected != Some(ix);
        self.selected = Some(ix);
        let value = option.value.clone();
        self.open = false;
        self.trigger.focus(window, cx);
        if changed {
            cx.emit(SelectChanged { value });
        }
        cx.notify();
    }

    fn child_id(&self, name: &'static str) -> ElementId {
        ElementId::NamedChild(self.id.clone().into(), name.into())
    }
}

impl Render for Select {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let metrics = self.size.metrics();
        let focused = self.trigger.is_focused(window) || self.list.contains_focused(window, cx);
        let look = frame_look(FieldFlags {
            focused: focused && !self.disabled,
            invalid: self.invalid,
            disabled: self.disabled,
            read_only: false,
        });
        let current = self.selected.and_then(|ix| self.options.get(ix));
        let shown = current
            .map(|option| (option.label.clone(), ColorToken::FgStrong))
            .or_else(|| {
                self.placeholder
                    .clone()
                    .map(|text| (text, ColorToken::FgMuted))
            });
        let menu_look = MENU.sample_handle(&self.motion, self.open, window, cx);
        let entity = cx.entity();

        let trigger = Frame {
            id: self.child_id("frame"),
            radius: metrics.control.radius,
            height: Some(metrics.control.height),
            look,
            theme,
        }
        .render(window, cx)
        .id(self.child_id("trigger"))
        .min_w(px(MIN_WIDTH))
        .pl(px(metrics.pad_x))
        .pr(px(select_padding_end()))
        .font_family(FONT_SANS.name)
        .text_size(px(metrics.font * FONT_SANS.size_adjust))
        .line_height(relative(1.3))
        .when(!self.disabled, |el| el.cursor_pointer())
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .when(!self.disabled, |el| {
            el.on_click(cx.listener(|this, _, window, cx| {
                let open = !this.open;
                this.set_open(open, cx);
                if open {
                    this.list.focus(window, cx);
                } else {
                    this.trigger.focus(window, cx);
                }
            }))
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .children(
                    shown.map(|(text, token)| div().text_color(theme.color(token)).child(text)),
                ),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(px(CHEVRON_INSET))
                .flex()
                .items_center()
                .child(glyph(
                    IconName::ChevronDown,
                    px(CHEVRON),
                    theme.color(ColorToken::FgFaint),
                )),
        );

        let highlighted = self.highlighted;
        let selected = self.selected;
        let options = self.options.iter().enumerate().map(|(ix, option)| {
            let active = ix == highlighted;
            let id = ElementId::NamedChild(self.id.clone().into(), option.value.clone());
            div()
                .id(id)
                .flex()
                .items_center()
                .h(px(OPTION_HEIGHT))
                .px(px(OPTION_PAD_X))
                .rounded(px(RADIUS_SM))
                .cursor_pointer()
                .role(Role::ListBoxOption)
                .aria_label(option.label.clone())
                .aria_selected(selected == Some(ix))
                .when(active, |el| {
                    el.aria_active_descendant()
                        .bg(theme.color(ColorToken::NeutralFill))
                })
                .text_color(theme.color(ColorToken::Fg))
                .when(selected == Some(ix), |el| {
                    el.font_weight(FontWeight::SEMIBOLD)
                })
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    if *hovered {
                        this.highlighted = ix;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.choose(ix, window, cx)))
                .child(option.label.clone())
        });

        let list = div()
            .id(self.child_id("list"))
            .track_focus(&self.list)
            .role(Role::ListBox)
            .look(&menu_look)
            .mt(px(MENU_GAP))
            .min_w(px(MIN_WIDTH))
            .p(px(MENU_PAD))
            .rounded(px(RADIUS_MENU))
            .bg(theme.color(ColorToken::BgRaised))
            .border_1()
            .border_color(theme.color(ColorToken::BorderSoft))
            .shadow(theme.shadow(ShadowToken::Md))
            .font_family(FONT_SANS.name)
            .text_size(px(metrics.font * FONT_SANS.size_adjust))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| {
                this.highlighted = step(this.highlighted, this.options.len(), false);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| {
                this.highlighted = step(this.highlighted, this.options.len(), true);
                cx.notify();
            }))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.set_open(false, cx)))
            .children(options);

        let open_entity = entity.clone();
        let confirm_entity = entity;
        let value = current.map(|option| option.label.clone());
        SelectRoot::new(self.id.clone())
            .open(self.open)
            .disabled(self.disabled)
            .focus_handle(&self.trigger)
            .content_focus_handle(&self.list)
            .when_some(self.label.clone(), |root, label| {
                root.accessibility_label(label)
            })
            .when_some(value, |root, value| root.accessibility_value(value))
            .on_open_change(move |open, _, cx| {
                open_entity.update(cx, |this, cx| this.set_open(open, cx));
            })
            .on_confirm(move |window, cx| {
                confirm_entity.update(cx, |this, cx| {
                    let ix = this.highlighted;
                    this.choose(ix, window, cx);
                });
            })
            .flex()
            .flex_col()
            .child(trigger)
            .when(menu_look.present, |root| {
                root.child(deferred(anchored().snap_to_window().child(list)).with_priority(1))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::step;

    #[test]
    fn stepping_stops_at_both_ends() {
        assert_eq!(step(0, 3, false), 0);
        assert_eq!(step(0, 3, true), 1);
        assert_eq!(step(2, 3, true), 2);
        assert_eq!(step(0, 0, true), 0);
    }
}
