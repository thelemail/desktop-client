use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::base::{Align, Placement, Root, RootPlugin};
use gpui_kit::{
    App, Bounds, Context, Div, ElementId, Entity, FocusHandle, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, Role, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Task, Window, div, px, relative,
};

use super::anchor::{self, Place};
use super::layer::{self, Layer};
use crate::motion::{Moving as _, TOOLTIP};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_MONO, FONT_SANS, RADIUS_MENU, RADIUS_XS};

pub const TOOLTIP_DELAY: Duration = Duration::from_millis(350);
pub const TOOLTIP_SKIP: Duration = Duration::from_millis(300);
const OFFSET: f32 = 6.0;
const MAX_WIDTH: f32 = 280.0;
const KBD_BORDER: f32 = 0.35;
const DEPTH: usize = 50;

#[derive(Clone)]
pub struct Tip {
    id: ElementId,
    text: SharedString,
    shortcut: Option<SharedString>,
    placement: Placement,
    theme: Option<Theme>,
}

impl Tip {
    pub fn new(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            shortcut: None,
            placement: Placement::Top,
            theme: None,
        }
    }

    pub fn shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    pub fn shortcut_opt(mut self, keys: Option<SharedString>) -> Self {
        self.shortcut = keys;
        self
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Pointer,
    Keyboard,
}

struct Shown {
    tip: Tip,
    trigger: Bounds<Pixels>,
    source: Source,
}

#[derive(Default)]
pub struct Tips {
    shown: Option<Shown>,
    pending: Option<(ElementId, Task<()>)>,
    last_hidden: Option<Instant>,
    epoch: usize,
}

impl RootPlugin for Tips {}

impl Tips {
    fn entity(window: &Window, cx: &App) -> Option<Entity<Self>> {
        window.root::<Root>().flatten()?;
        Root::read(window, cx).plugin::<Self>()
    }

    fn owns(&self, id: &ElementId) -> bool {
        self.shown.as_ref().is_some_and(|shown| &shown.tip.id == id)
    }

    fn show(
        &mut self,
        tip: Tip,
        trigger: Bounds<Pixels>,
        source: Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let skip = self.shown.is_some()
            || self
                .last_hidden
                .is_some_and(|at| at.elapsed() < TOOLTIP_SKIP);
        if source == Source::Keyboard || skip {
            self.pending = None;
            self.epoch += 1;
            self.shown = Some(Shown {
                tip,
                trigger,
                source,
            });
            cx.notify();
            return;
        }
        if self.pending.as_ref().is_some_and(|(id, _)| *id == tip.id) {
            return;
        }
        let id = tip.id.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(TOOLTIP_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                this.pending = None;
                this.epoch += 1;
                this.shown = Some(Shown {
                    tip,
                    trigger,
                    source,
                });
                cx.notify();
            });
        });
        self.pending = Some((id, task));
    }

    fn hide(&mut self, id: &ElementId, source: Option<Source>, cx: &mut Context<Self>) {
        if self.pending.as_ref().is_some_and(|(owner, _)| owner == id) {
            self.pending = None;
        }
        let matches = self.shown.as_ref().is_some_and(|shown| {
            &shown.tip.id == id && source.is_none_or(|source| shown.source == source)
        });
        if matches {
            self.shown = None;
            self.last_hidden = Some(Instant::now());
            cx.notify();
        }
    }
}

impl Render for Tips {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shown) = self.shown.as_ref() else {
            layer::cover(&ElementId::Name("tooltip".into()), window, cx).clear(window);
            return div().into_any_element();
        };
        let theme = shown.tip.theme.unwrap_or(*cx.tokens());
        let look_id = ElementId::NamedChild(
            ElementId::Name("tooltip".into()).into(),
            SharedString::from(format!("look-{}", self.epoch)),
        );
        let look = TOOLTIP.sample(look_id, true, window, cx);
        let fg = theme.color(ColorToken::FgOnPine);
        let bubble = div()
            .id("tooltip-bubble")
            .role(Role::Tooltip)
            .aria_label(shown.tip.text.clone())
            .look(&look)
            .flex()
            .items_center()
            .gap(px(8.0))
            .max_w(px(MAX_WIDTH))
            .px(px(10.0))
            .py(px(7.0))
            .rounded(px(RADIUS_MENU))
            .bg(theme.color(ColorToken::BgInverse))
            .text_color(fg)
            .font_family(FONT_SANS.name)
            .text_size(px(12.5 * FONT_SANS.size_adjust))
            .font_weight(FontWeight::MEDIUM)
            .line_height(relative(1.3))
            .child(shown.tip.text.clone())
            .children(shown.tip.shortcut.clone().map(|keys| {
                div()
                    .px(px(4.0))
                    .py(px(2.0))
                    .border_1()
                    .border_color(fg.alpha(KBD_BORDER))
                    .rounded(px(RADIUS_XS))
                    .font_family(FONT_MONO.name)
                    .text_size(px(11.0 * FONT_MONO.size_adjust))
                    .line_height(relative(1.0))
                    .child(keys)
            }));
        anchor::anchored_passive(
            &ElementId::Name("tooltip".into()),
            Layer::Popover,
            DEPTH,
            Place::Side {
                trigger: shown.trigger,
                placement: shown.tip.placement,
                align: Align::Center,
                offset: px(OFFSET),
            },
            bubble,
            window,
            cx,
        )
    }
}

pub(crate) fn register(cx: &mut App) {
    Root::register_plugin::<Tips>(cx, |_, _| Tips::default());
}

fn bounds_slot(tip: &Tip, window: &mut Window, cx: &mut App) -> Rc<Cell<Bounds<Pixels>>> {
    anchor::slot(&tip.id, "tooltip-trigger", window, cx)
}

fn update(
    window: &mut Window,
    cx: &mut App,
    f: impl FnOnce(&mut Tips, &mut Window, &mut Context<Tips>) + 'static,
) {
    if let Some(tips) = Tips::entity(window, cx) {
        window.defer(cx, move |window, cx| {
            tips.update(cx, |tips, cx| f(tips, window, cx));
        });
    }
}

pub(crate) fn attach(
    el: Stateful<Div>,
    tip: Option<Tip>,
    focus: Option<&FocusHandle>,
    on_hover: impl Fn(&bool, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let Some(tip) = tip else {
        return el.on_hover(on_hover);
    };
    let slot = bounds_slot(&tip, window, cx);
    let keyboard =
        focus.is_some_and(|focus| focus.is_focused(window) && window.last_input_was_keyboard());
    let owned = Tips::entity(window, cx).is_some_and(|tips| tips.read(cx).owns(&tip.id));
    if keyboard && !owned {
        let (tip, slot) = (tip.clone(), slot.clone());
        update(window, cx, move |tips, window, cx| {
            tips.show(tip, slot.get(), Source::Keyboard, window, cx)
        });
    } else if !keyboard && owned {
        let id = tip.id.clone();
        update(window, cx, move |tips, _, cx| {
            tips.hide(&id, Some(Source::Keyboard), cx)
        });
    }
    let hover_tip = tip.clone();
    let hover_slot = slot.clone();
    let down_id = tip.id.clone();
    el.on_hover(move |hovered, window, cx| {
        on_hover(hovered, window, cx);
        let (tip, slot) = (hover_tip.clone(), hover_slot.clone());
        if *hovered {
            update(window, cx, move |tips, window, cx| {
                tips.show(tip, slot.get(), Source::Pointer, window, cx)
            });
        } else {
            update(window, cx, move |tips, _, cx| tips.hide(&tip.id, None, cx));
        }
    })
    .on_any_mouse_down(move |_, window, cx| {
        let id = down_id.clone();
        update(window, cx, move |tips, _, cx| tips.hide(&id, None, cx));
    })
    .child(anchor::bounds_of(slot))
}

pub trait Tooltipped: Sized {
    fn tip(self, tip: Tip, focus: Option<&FocusHandle>, window: &mut Window, cx: &mut App) -> Self;
}

impl Tooltipped for Stateful<Div> {
    fn tip(self, tip: Tip, focus: Option<&FocusHandle>, window: &mut Window, cx: &mut App) -> Self {
        attach(self.relative(), Some(tip), focus, |_, _, _| {}, window, cx)
    }
}
