use std::rc::Rc;

use gpui_kit::base::{Align, Placement};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ElementId, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use super::anchor::{self, Place};
use super::dialog::Action;
use super::layer::{self, Layer};
use super::{Dismiss, MODAL, focus};
use crate::motion::{MENU, Moving as _};
use crate::theme::{ActiveTokens, Appearance, Theme};
use crate::tokens::{ColorToken, FONT_SANS, RADIUS_MENU, ShadowToken};

const WIDTH: f32 = 288.0;
const PAD: f32 = 12.0;
const OFFSET: f32 = 6.0;
const EDGE: f32 = 24.0;

type Trigger = Box<dyn FnOnce(bool, Action, FocusHandle) -> AnyElement>;
type Content = Box<dyn FnOnce(Action) -> AnyElement>;

#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    trigger: Option<Trigger>,
    content: Option<Content>,
    width: f32,
    placement: Placement,
    align: Align,
    label: Option<SharedString>,
    theme: Option<Theme>,
}

impl Popover {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            trigger: None,
            content: None,
            width: WIDTH,
            placement: Placement::Bottom,
            align: Align::Center,
            label: None,
            theme: None,
        }
    }

    pub fn trigger<E: IntoElement>(
        mut self,
        build: impl FnOnce(bool, Action, FocusHandle) -> E + 'static,
    ) -> Self {
        self.trigger = Some(Box::new(move |open, toggle, focus| {
            build(open, toggle, focus).into_any_element()
        }));
        self
    }

    pub fn content<E: IntoElement>(mut self, build: impl FnOnce(Action) -> E + 'static) -> Self {
        self.content = Some(Box::new(move |close| build(close).into_any_element()));
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    fn child_id(&self, name: &'static str) -> ElementId {
        ElementId::NamedChild(self.id.clone().into(), name.into())
    }
}

pub(crate) fn surface_colors(theme: &Theme) -> (ColorToken, ColorToken) {
    if theme.appearance == Appearance::Dark {
        (ColorToken::BgRaised, ColorToken::BorderStrong)
    } else {
        (ColorToken::Surface, ColorToken::BorderSoft)
    }
}

impl RenderOnce for Popover {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let state = window.use_keyed_state(self.child_id("open"), cx, |_, _| false);
        let open = *state.read(cx);
        let toggle: Action = {
            let state = state.clone();
            Rc::new(move |window, cx| {
                state.update(cx, |open, _| *open = !*open);
                window.refresh();
            })
        };
        let close: Action = {
            let state = state.clone();
            Rc::new(move |window, cx| {
                state.update(cx, |open, _| *open = false);
                window.refresh();
            })
        };
        let trigger_bounds = anchor::slot(&self.id, "trigger", window, cx);
        let container = focus::handle(&self.id, "panel", window, cx);
        let trigger_focus = focus::handle(&self.id, "trigger-focus", window, cx);
        focus::follow_to(
            &self.id,
            open,
            &container,
            None,
            Some(trigger_focus.clone()),
            window,
            cx,
        );
        let look = MENU.sample(self.child_id("motion"), open, window, cx);
        let trigger = self
            .trigger
            .take()
            .map(|build| build(open, toggle, trigger_focus))
            .unwrap_or_else(|| div().into_any_element());

        let panel_id = self.child_id("panel");
        let overlay = if look.present {
            let (bg, border) = surface_colors(&theme);
            let escape = close.clone();
            let outside = close.clone();
            let hit = trigger_bounds.clone();
            let content = self
                .content
                .take()
                .map(|build| build(close))
                .unwrap_or_else(|| div().into_any_element());
            let viewport = window.viewport_size();
            let panel = div()
                .id(self.child_id("surface"))
                .track_focus(&container)
                .key_context(MODAL)
                .role(Role::Dialog)
                .when_some(self.label.clone(), |el, label| el.aria_label(label))
                .on_action(move |_: &Dismiss, window, cx| escape(window, cx))
                .on_mouse_down_out(move |event, window, cx| {
                    if !hit.get().contains(&event.position) {
                        outside(window, cx);
                    }
                })
                .look(&look)
                .w(px(self.width))
                .max_w(viewport.width - px(EDGE))
                .p(px(PAD))
                .rounded(px(RADIUS_MENU))
                .border_1()
                .border_color(theme.color(border))
                .bg(theme.color(bg))
                .shadow(theme.shadow(ShadowToken::Md))
                .font_family(FONT_SANS.name)
                .text_color(theme.color(ColorToken::Fg))
                .child(content);
            Some(anchor::anchored(
                &panel_id,
                Layer::Popover,
                0,
                Place::Side {
                    trigger: trigger_bounds.get(),
                    placement: self.placement,
                    align: self.align,
                    offset: px(OFFSET),
                },
                None,
                panel,
                window,
                cx,
            ))
        } else {
            layer::cover(&panel_id, window, cx).clear(window, cx);
            None
        };

        div()
            .relative()
            .flex()
            .flex_none()
            .child(trigger)
            .child(anchor::bounds_of(trigger_bounds))
            .children(overlay)
    }
}
