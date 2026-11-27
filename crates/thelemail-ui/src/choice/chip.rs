use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, Role, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Toggled,
    Window, div, linear_color_stop, linear_gradient, px, relative,
};

use super::look::{self, DISABLED_OPACITY};
use super::{Handler, child, focus, hovered, ring, set_hover};
use crate::a11y::A11yStateExt as _;
use crate::button::{Phase, TOOLTIP_DELAY, Tooltip};
use crate::icons::{IconName, glyph};
use crate::motion::{BASE, EASE, Spec, ease_color};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_SANS, RADIUS_PILL};

const COLOR: Spec = Spec::new(BASE, EASE);
const GAP: f32 = 6.0;
const ICON: f32 = 13.0;
const LEADING_PAD: f32 = 3.0;
const REMOVABLE_PAD: f32 = 4.0;
const REMOVE: f32 = 20.0;
const REMOVE_ICON: f32 = 12.0;
const REMOVE_REST: f32 = 0.7;
const REMOVE_HOVER: f32 = 0.14;
const ROW_GAP: f32 = 6.0;
const FADE: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChipSize {
    Sm,
    #[default]
    Md,
}

impl ChipSize {
    fn height(self) -> f32 {
        match self {
            Self::Sm => 24.0,
            Self::Md => 28.0,
        }
    }

    fn pad(self) -> f32 {
        match self {
            Self::Sm => 9.0,
            Self::Md => 11.0,
        }
    }

    fn font(self) -> f32 {
        match self {
            Self::Sm => 12.0,
            Self::Md => 12.5,
        }
    }
}

enum Form {
    Toggle(bool, Option<Handler<bool>>),
    Action(Handler<()>),
    Link(Handler<()>),
    Static,
}

#[derive(IntoElement)]
pub struct Chip {
    id: ElementId,
    label: SharedString,
    form: Form,
    size: ChipSize,
    leading: Option<AnyElement>,
    remove: Option<(SharedString, Handler<()>)>,
    disabled: bool,
    theme: Option<Theme>,
}

impl Chip {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            form: Form::Static,
            size: ChipSize::default(),
            leading: None,
            remove: None,
            disabled: false,
            theme: None,
        }
    }

    pub fn pressed(
        mut self,
        pressed: bool,
        on_toggle: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.form = Form::Toggle(pressed, Some(Rc::new(on_toggle)));
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.form = Form::Action(Rc::new(move |(), window, cx| handler(window, cx)));
        self
    }

    pub fn link(mut self, open: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.form = Form::Link(Rc::new(move |(), window, cx| open(window, cx)));
        self
    }

    pub fn on_remove(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.remove = Some((
            label.into(),
            Rc::new(move |(), window, cx| handler(window, cx)),
        ));
        self
    }

    pub fn leading(mut self, element: impl IntoElement) -> Self {
        self.leading = Some(element.into_any_element());
        self
    }

    pub fn size(mut self, size: ChipSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl RenderOnce for Chip {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let removable = self.remove.is_some() && matches!(self.form, Form::Static);
        let pressable = !matches!(self.form, Form::Static) && !removable;
        let interactive = pressable && !self.disabled;
        let on = matches!(self.form, Form::Toggle(true, _));
        let hover = hovered(&self.id, window, cx);
        let phase = if *hover.read(cx) {
            Phase::Hover
        } else {
            Phase::Rest
        };
        let paint = look::chip(on, interactive, phase);
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
        let fg = ease_color(
            child(&self.id, "fg"),
            paint.fg.resolve(&theme),
            COLOR,
            window,
            cx,
        );
        let ring_color = theme.color(ColorToken::FocusRing);
        let height = self.size.height();
        let pad = self.size.pad();

        let remove = self.remove.filter(|_| removable).map(|(label, handler)| {
            let id = child(&self.id, "remove");
            let handle = focus(&id, window, cx);
            let over = hovered(&id, window, cx);
            let lit = *over.read(cx) && !self.disabled;
            let keyboard = handle.is_focused(window) && window.last_input_was_keyboard();
            let disabled = self.disabled;
            let tip = label.clone();
            div()
                .id(id)
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(REMOVE))
                .rounded_full()
                .when(lit, |el| el.bg(fg.opacity(REMOVE_HOVER)))
                .when(!disabled, |el| el.cursor_pointer())
                .role(Role::Button)
                .aria_label(label)
                .when(!disabled, |el| el.track_focus(&handle.tab_index(0)))
                .on_hover(set_hover(over))
                .on_click(move |_, window, cx| {
                    if !disabled {
                        handler((), window, cx);
                    }
                })
                .tooltip(move |window, cx| Tooltip::build(tip.clone(), None, Some(theme), window, cx))
                .tooltip_show_delay(TOOLTIP_DELAY)
                .child(glyph(
                    IconName::Xmark,
                    px(REMOVE_ICON),
                    fg.opacity(if lit { 1.0 } else { REMOVE_REST }),
                ))
                .when(keyboard, |el| el.child(ring(0.0, REMOVE / 2.0, ring_color)))
                .a11y_state(false, disabled)
        });

        let has_leading = self.leading.is_some();
        let label = self.label.clone();
        let body = div()
            .id(self.id.clone())
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(GAP))
            .max_w_full()
            .h(px(height))
            .pl(px(if has_leading { LEADING_PAD } else { pad }))
            .pr(px(if removable { REMOVABLE_PAD } else { pad }))
            .rounded(px(RADIUS_PILL))
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_color(fg)
            .font_family(FONT_SANS.name)
            .text_size(px(self.size.font() * FONT_SANS.size_adjust))
            .font_weight(if on {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::MEDIUM
            })
            .line_height(relative(1.0))
            .whitespace_nowrap()
            .when(self.disabled, |el| el.opacity(DISABLED_OPACITY))
            .when(on, |el| el.child(glyph(IconName::Check, px(ICON), fg)))
            .when_some(self.leading, |el, leading| {
                el.child(div().flex().flex_none().items_center().child(leading))
            })
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(label.clone()),
            )
            .children(remove);

        if !pressable {
            return body
                .role(if removable { Role::Group } else { Role::Label })
                .aria_label(label)
                .into_any_element();
        }

        let handle = focus(&self.id, window, cx);
        let focused = handle.is_focused(window);
        let keyboard = focused && window.last_input_was_keyboard();
        let disabled = self.disabled;
        let (role, toggled, press): (Role, Option<bool>, Handler<()>) = match self.form {
            Form::Toggle(pressed, handler) => (
                Role::Button,
                Some(pressed),
                Rc::new(move |_, window, cx| {
                    if let Some(handler) = &handler {
                        handler(!pressed, window, cx);
                    }
                }),
            ),
            Form::Action(handler) => (Role::Button, None, handler),
            Form::Link(handler) => (Role::Link, None, handler),
            Form::Static => unreachable!(),
        };
        body.role(role)
            .aria_label(label)
            .when_some(toggled, |el, pressed| {
                el.aria_toggled(if pressed {
                    Toggled::True
                } else {
                    Toggled::False
                })
            })
            .when(interactive, |el| el.cursor_pointer())
            .when(disabled, |el| el.cursor_not_allowed())
            .when(!disabled || focused, |el| {
                el.track_focus(&handle.tab_index(0).tab_stop(!disabled))
            })
            .on_hover(set_hover(hover))
            .on_click(move |_, window, cx| {
                if !disabled {
                    press((), window, cx);
                }
            })
            .when(keyboard, |el| el.child(ring(1.0, height / 2.0, ring_color)))
            .a11y_state(false, disabled)
            .into_any_element()
    }
}

#[derive(IntoElement)]
pub struct ChipRow {
    id: ElementId,
    children: Vec<AnyElement>,
    fade: ColorToken,
    label: Option<SharedString>,
    theme: Option<Theme>,
}

impl ChipRow {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            children: Vec::new(),
            fade: ColorToken::Bg,
            label: None,
            theme: None,
        }
    }

    pub fn fade(mut self, ground: ColorToken) -> Self {
        self.fade = ground;
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
}

impl ParentElement for ChipRow {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for ChipRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let scroll = window
            .use_keyed_state(child(&self.id, "scroll"), cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        let more = scroll.max_offset().x + scroll.offset().x > px(0.5);
        let ground = theme.color(self.fade);
        div()
            .relative()
            .w_full()
            .min_w_0()
            .child(
                div()
                    .id(self.id)
                    .flex()
                    .flex_nowrap()
                    .items_center()
                    .gap(px(ROW_GAP))
                    .w_full()
                    .overflow_x_scroll()
                    .track_scroll(&scroll)
                    .role(Role::Group)
                    .when_some(self.label, |el, label| el.aria_label(label))
                    .on_scroll(|_, window, _| window.refresh())
                    .children(self.children),
            )
            .when(more, |el| {
                el.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .w(px(FADE))
                        .bg(linear_gradient(
                            90.0,
                            linear_color_stop(ground.opacity(0.0), 0.0),
                            linear_color_stop(ground, 1.0),
                        )),
                )
            })
    }
}
