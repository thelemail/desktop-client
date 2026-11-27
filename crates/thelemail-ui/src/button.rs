mod look;
mod split;
mod tooltip;

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, Corners, ElementId, FocusHandle, FontWeight, Hsla,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, RenderOnce, Role,
    SharedString, StatefulInteractiveElement, Styled, Toggled, Window, div, px, relative,
};

use crate::a11y::A11yStateExt as _;
use crate::icons::{IconName, glyph};
use crate::motion::{BASE, EASE, FAST, Spec, animate, ease_color};
use crate::spinner::Spinner;
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{
    CONTROL_LG, CONTROL_MD, CONTROL_SM, CONTROL_XS, ColorToken, ControlHeight, FONT_SANS,
};

pub use look::{Fill, Flags, Look, Phase, look};
pub use split::SplitButton;
pub use tooltip::{TOOLTIP_DELAY, Tooltip};

const NUDGE: f32 = 1.0;
const BUSY_RISE: f32 = 4.0;
const RING_WIDTH: f32 = 2.0;
const RING_OFFSET: f32 = 2.0;
const COLOR: Spec = Spec::new(BASE, EASE);
const PRESS: Spec = Spec::new(FAST, EASE);
const SWAP: Spec = Spec::new(FAST, EASE);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variant {
    Primary,
    Tonal,
    #[default]
    Neutral,
    Ghost,
    GhostDanger,
    Danger,
    DangerSolid,
    Caution,
}

impl Variant {
    pub const ALL: [Self; 8] = [
        Self::Primary,
        Self::Tonal,
        Self::Neutral,
        Self::Ghost,
        Self::GhostDanger,
        Self::Danger,
        Self::DangerSolid,
        Self::Caution,
    ];

    pub fn is_ghost(self) -> bool {
        matches!(self, Self::Ghost | Self::GhostDanger)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Size {
    Sm,
    #[default]
    Md,
    Lg,
    IconXs,
    IconSm,
    Icon,
    IconLg,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub control: ControlHeight,
    pub font: f32,
    pub pad_x: f32,
    pub gap: f32,
    pub icon: f32,
    pub square: bool,
}

impl Size {
    pub const ALL: [Self; 7] = [
        Self::Sm,
        Self::Md,
        Self::Lg,
        Self::IconXs,
        Self::IconSm,
        Self::Icon,
        Self::IconLg,
    ];

    pub const fn metrics(self) -> Metrics {
        let (control, font, pad_x, gap, icon) = match self {
            Self::IconXs => (CONTROL_XS, 12.0, 8.0, 4.0, 14.0),
            Self::Sm | Self::IconSm => (CONTROL_SM, 13.0, 12.0, 6.0, 15.0),
            Self::Md | Self::Icon => (CONTROL_MD, 14.5, 16.0, 8.0, 17.0),
            Self::Lg | Self::IconLg => (CONTROL_LG, 15.5, 16.0, 8.0, 18.0),
        };
        let square = matches!(self, Self::IconXs | Self::IconSm | Self::Icon | Self::IconLg);
        Metrics {
            control,
            font,
            pad_x: if square { 0.0 } else { pad_x },
            gap,
            icon,
            square,
        }
    }
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(Debug, Clone, Copy, Default)]
struct Pointer {
    hovered: bool,
    down: bool,
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    variant: Variant,
    size: Size,
    label: Option<SharedString>,
    icon: Option<IconName>,
    trailing: Option<IconName>,
    content: Option<AnyElement>,
    block: bool,
    busy: bool,
    busy_label: Option<SharedString>,
    tooltip: Option<SharedString>,
    shortcut: Option<SharedString>,
    accessibility_label: Option<SharedString>,
    pressed: Option<bool>,
    expanded: Option<bool>,
    star: bool,
    inverse: bool,
    disabled: bool,
    corners: Corners<bool>,
    focus: Option<FocusHandle>,
    tab_index: isize,
    theme: Option<Theme>,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            variant: Variant::default(),
            size: Size::default(),
            label: None,
            icon: None,
            trailing: None,
            content: None,
            block: false,
            busy: false,
            busy_label: None,
            tooltip: None,
            shortcut: None,
            accessibility_label: None,
            pressed: None,
            expanded: None,
            star: false,
            inverse: false,
            disabled: false,
            corners: Corners::all(true),
            focus: None,
            tab_index: 0,
            theme: None,
            on_click: None,
        }
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    pub fn primary(self) -> Self {
        self.variant(Variant::Primary)
    }

    pub fn tonal(self) -> Self {
        self.variant(Variant::Tonal)
    }

    pub fn ghost(self) -> Self {
        self.variant(Variant::Ghost)
    }

    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn trailing(mut self, icon: IconName) -> Self {
        self.trailing = Some(icon);
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn block(mut self) -> Self {
        self.block = true;
        self
    }

    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    pub fn busy_label(mut self, label: impl Into<SharedString>) -> Self {
        self.busy_label = Some(label.into());
        self
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = Some(pressed);
        self
    }

    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    pub fn star(mut self) -> Self {
        self.star = true;
        self
    }

    pub fn on_inverse(mut self) -> Self {
        self.inverse = true;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn corners(mut self, corners: Corners<bool>) -> Self {
        self.corners = corners;
        self
    }

    pub fn track_focus(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }

    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = index;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    fn child_id(&self, name: &'static str) -> ElementId {
        ElementId::NamedChild(self.id.clone().into(), name.into())
    }
}

fn rounded(radius: Pixels, corners: Corners<bool>) -> Corners<Pixels> {
    let pick = |on: bool| if on { radius } else { px(0.0) };
    Corners {
        top_left: pick(corners.top_left),
        top_right: pick(corners.top_right),
        bottom_right: pick(corners.bottom_right),
        bottom_left: pick(corners.bottom_left),
    }
}

impl Button {
    fn row(&self, color: Hsla, metrics: Metrics) -> gpui_kit::Div {
        let icon = px(metrics.icon);
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(metrics.gap))
            .when_some(self.icon, |row, icon_name| {
                row.child(glyph(icon_name, icon, color))
            })
            .when_some(self.label.clone(), |row, label| {
                row.child(div().whitespace_nowrap().child(label))
            })
            .when_some(self.trailing, |row, icon_name| {
                row.child(glyph(icon_name, icon, color))
            })
    }
}

impl RenderOnce for Button {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let metrics = self.size.metrics();
        let height = px(metrics.control.height);
        let radius = px(metrics.control.radius);
        let interactive = !self.disabled && !self.busy;

        let pointer = window.use_keyed_state(self.child_id("pointer"), cx, |_, _| Pointer::default());
        let Pointer { hovered, down } = *pointer.read(cx);
        let phase = match (interactive, down && hovered, hovered) {
            (false, _, _) => Phase::Rest,
            (true, true, _) => Phase::Active,
            (true, false, true) => Phase::Hover,
            (true, false, false) => Phase::Rest,
        };
        let flags = Flags {
            on: self.pressed == Some(true) || self.expanded == Some(true),
            star: self.star && self.pressed == Some(true),
            busy: self.busy,
            disabled: self.disabled,
            inverse: self.inverse,
        };
        let target = look(self.variant, flags, phase);
        let bg = ease_color(self.child_id("bg"), target.bg.resolve(&theme), COLOR, window, cx);
        let fg = ease_color(self.child_id("fg"), target.fg.resolve(&theme), COLOR, window, cx);

        let nudge_to = if phase == Phase::Active && !cx.reduce_motion() {
            NUDGE
        } else {
            0.0
        };
        let nudge = animate(self.child_id("nudge"), nudge_to, PRESS, window, cx).value;
        let swap = animate(
            self.child_id("swap"),
            if self.busy { 1.0 } else { 0.0 },
            SWAP,
            window,
            cx,
        )
        .value
        .clamp(0.0, 1.0);
        let rise = if cx.reduce_motion() { 0.0 } else { BUSY_RISE };

        let focus = self.focus.clone().unwrap_or_else(|| {
            window
                .use_keyed_state(self.child_id("focus"), cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone()
        });
        let focused = focus.is_focused(window);
        let ring = focused && window.last_input_was_keyboard();

        let label_for_a11y = self
            .accessibility_label
            .clone()
            .or_else(|| self.label.clone())
            .or_else(|| self.tooltip.clone());

        let idle = self.content.take().map_or_else(
            || self.row(fg, metrics).into_any_element(),
            |content| content,
        );
        let busy_row = div()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(metrics.gap))
            .child(Spinner::new(self.child_id("spin"), metrics.font, fg))
            .when_some(self.busy_label.clone(), |row, label| {
                row.child(div().whitespace_nowrap().child(label))
            });
        let cell = div()
            .grid()
            .grid_cols(1)
            .grid_rows(1)
            .relative()
            .top(px(nudge))
            .child(
                div()
                    .col_start(1)
                    .row_start(1)
                    .flex()
                    .items_center()
                    .justify_center()
                    .relative()
                    .top(px(-rise * swap))
                    .opacity(1.0 - swap)
                    .child(idle),
            )
            .child(
                    div()
                        .col_start(1)
                        .row_start(1)
                        .flex()
                        .items_center()
                        .justify_center()
                        .relative()
                        .top(px(rise * (1.0 - swap)))
                        .opacity(swap)
                        .child(busy_row),
            );

        let corners = rounded(radius, self.corners);
        let ring_corners = rounded(radius + px(RING_OFFSET + RING_WIDTH), self.corners);
        let ring_color = theme.color(ColorToken::FocusRing);
        let tooltip = self
            .tooltip
            .clone()
            .filter(|_| self.expanded != Some(true));
        let shortcut = self.shortcut.clone();
        let on_click = self.on_click.clone().filter(|_| interactive);
        let pointer_hover = pointer.clone();
        let pointer_down = pointer.clone();
        let pointer_up = pointer.clone();
        let pointer_out = pointer;
        let disabled = self.disabled;

        div()
            .id(self.id.clone())
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(height)
            .min_w(height)
            .when(metrics.square, |el| el.w(height))
            .when(self.block, |el| el.w_full())
            .px(px(metrics.pad_x))
            .corner_radii(corners)
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .bg(bg)
            .text_color(fg)
            .font_family(FONT_SANS.name)
            .text_size(px(metrics.font * FONT_SANS.size_adjust))
            .font_weight(FontWeight::SEMIBOLD)
            .line_height(relative(1.0))
            .when(self.busy, |el| el.cursor_progress())
            .when(self.disabled, |el| el.cursor_not_allowed())
            .when(interactive, |el| el.cursor_pointer())
            .role(Role::Button)
            .when_some(label_for_a11y, |el, label| el.aria_label(label))
            .when_some(self.pressed, |el, pressed| {
                el.aria_toggled(if pressed { Toggled::True } else { Toggled::False })
            })
            .when_some(self.expanded, |el, expanded| el.aria_expanded(expanded))
            .when(!disabled || focused, |el| {
                el.track_focus(&focus.tab_index(self.tab_index).tab_stop(!disabled))
            })
            .on_hover(move |hovered, window, cx| {
                pointer_hover.update(cx, |p, _| {
                    p.hovered = *hovered;
                    if !*hovered {
                        p.down = false;
                    }
                });
                window.refresh();
            })
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.prevent_default();
                if disabled {
                    cx.stop_propagation();
                    return;
                }
                pointer_down.update(cx, |p, _| p.down = true);
                window.refresh();
            })
            .on_mouse_up(MouseButton::Left, move |_, window, cx| {
                pointer_up.update(cx, |p, _| p.down = false);
                window.refresh();
            })
            .on_mouse_up_out(MouseButton::Left, move |_, window, cx| {
                pointer_out.update(cx, |p, _| p.down = false);
                window.refresh();
            })
            .when_some(on_click, |el, on_click| {
                el.on_click(move |event, window, cx| on_click(event, window, cx))
            })
            .when_some(tooltip, |el, text| {
                el.tooltip(move |window, cx| {
                    Tooltip::build(text.clone(), shortcut.clone(), Some(theme), window, cx)
                })
                .tooltip_show_delay(TOOLTIP_DELAY)
            })
            .child(cell)
            .when(ring, |el| {
                el.child(
                    div()
                        .absolute()
                        .top(px(-(RING_OFFSET + RING_WIDTH + 1.0)))
                        .left(px(-(RING_OFFSET + RING_WIDTH + 1.0)))
                        .right(px(-(RING_OFFSET + RING_WIDTH + 1.0)))
                        .bottom(px(-(RING_OFFSET + RING_WIDTH + 1.0)))
                        .corner_radii(ring_corners)
                        .border(px(RING_WIDTH))
                        .border_color(ring_color),
                )
            })
            .a11y_state(self.busy, self.disabled)
    }
}
