use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement, RenderOnce, Role,
    SharedString, StatefulInteractiveElement, Styled, Toggled, Window, accesskit, div, px,
    relative,
};

use super::look::{self, DISABLED_OPACITY};
use super::roving::{self, Pick};
use super::{Handler, child, hovered, is_enter, ring, set_hover};
use crate::a11y::A11yStateExt as _;
use crate::button::{Phase, TOOLTIP_DELAY, Tooltip};
use crate::icons::{IconName, glyph};
use crate::motion::{BASE, EASE, EASE_OUT, SLOW, Spec, ease_color};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{CONTROL_MD, CONTROL_SM, ColorToken, FONT_SANS, ShadowToken};

const SLIDE: Spec = Spec::new(SLOW, EASE_OUT);
const COLOR: Spec = Spec::new(BASE, EASE);
const ITEM_PAD: f32 = 12.0;
const GAP: f32 = 6.0;
const ICON: f32 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegmentedSize {
    #[default]
    Sm,
    Md,
}

impl SegmentedSize {
    fn height(self) -> f32 {
        match self {
            Self::Sm => CONTROL_SM.height,
            Self::Md => CONTROL_MD.height,
        }
    }

    fn pad(self) -> f32 {
        match self {
            Self::Sm => 3.0,
            Self::Md => 4.0,
        }
    }

    fn font(self) -> f32 {
        match self {
            Self::Sm => 13.0,
            Self::Md => 14.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SegmentOption {
    value: SharedString,
    label: SharedString,
    icon: Option<IconName>,
    disabled: bool,
    hide_label: bool,
}

impl SegmentOption {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            icon: None,
            disabled: false,
            hide_label: false,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn hide_label(mut self) -> Self {
        self.hide_label = true;
        self
    }
}

#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    options: Vec<SegmentOption>,
    value: Option<SharedString>,
    size: SegmentedSize,
    block: bool,
    disabled: bool,
    label: Option<SharedString>,
    theme: Option<Theme>,
    on_change: Option<Handler<SharedString>>,
}

impl Segmented {
    pub fn new(id: impl Into<ElementId>, options: Vec<SegmentOption>) -> Self {
        Self {
            id: id.into(),
            options,
            value: None,
            size: SegmentedSize::default(),
            block: false,
            disabled: false,
            label: None,
            theme: None,
            on_change: None,
        }
    }

    pub fn value(mut self, value: Option<impl Into<SharedString>>) -> Self {
        self.value = value.map(Into::into);
        self
    }

    pub fn size(mut self, size: SegmentedSize) -> Self {
        self.size = size;
        self
    }

    pub fn block(mut self) -> Self {
        self.block = true;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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

    pub fn on_change(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let count = self.options.len();
        let height = self.size.height();
        let pad = self.size.pad();
        let radius = CONTROL_SM.radius;
        let item_radius = radius - pad;
        let selected = self
            .options
            .iter()
            .position(|o| self.value.as_ref() == Some(&o.value));
        let enabled: Vec<bool> = self
            .options
            .iter()
            .map(|o| !o.disabled && !self.disabled)
            .collect();
        let handles = roving::handles(&self.id, count, window, cx);
        let stop = roving::stop(selected, &enabled);
        let at = crate::motion::animate(
            child(&self.id, "thumb"),
            selected.unwrap_or(0) as f32,
            SLIDE,
            window,
            cx,
        )
        .value;
        let share = 1.0 / count.max(1) as f32;
        let ring_color = theme.color(ColorToken::FocusRing);

        let values: Vec<SharedString> = self.options.iter().map(|o| o.value.clone()).collect();
        let on_change = self.on_change.clone();
        let pick: Pick = Rc::new(move |ix, window, cx| {
            if let (Some(on_change), Some(value)) = (&on_change, values.get(ix)) {
                on_change(value.clone(), window, cx);
            }
        });

        let thumb = div()
            .absolute()
            .top(px(pad))
            .bottom(px(pad))
            .left(px(pad))
            .right(px(pad))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(at * share))
                    .w(relative(share))
                    .rounded(px(item_radius + 1.0))
                    .bg(theme.color(look::THUMB))
                    .shadow(theme.shadow(ShadowToken::Thumb))
                    .when(selected.is_none(), |el| el.opacity(0.0)),
            );

        let mut items = Vec::with_capacity(count);
        for (ix, option) in self.options.into_iter().enumerate() {
            let item_id = ElementId::NamedChild(self.id.clone().into(), option.value.clone());
            let checked = selected == Some(ix);
            let interactive = enabled[ix];
            let hover = hovered(&item_id, window, cx);
            let phase = if *hover.read(cx) {
                Phase::Hover
            } else {
                Phase::Rest
            };
            let fg = ease_color(
                child(&item_id, "fg"),
                look::segment(checked, interactive, phase).resolve(&theme),
                COLOR,
                window,
                cx,
            );
            let handle = handles[ix].clone();
            let focused = handle.is_focused(window);
            let keyboard = focused && window.last_input_was_keyboard();
            let pick = pick.clone();
            let focus_on_click = handle.clone();
            let hidden = option.hide_label;
            let label = option.label.clone();
            items.push(
                div()
                    .id(item_id)
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(GAP))
                    .min_w_0()
                    .h_full()
                    .px(px(ITEM_PAD))
                    .rounded(px(item_radius))
                    .text_color(fg)
                    .font_family(FONT_SANS.name)
                    .text_size(px(self.size.font() * FONT_SANS.size_adjust))
                    .font_weight(if checked {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .line_height(relative(1.0))
                    .whitespace_nowrap()
                    .when(interactive, |el| el.cursor_pointer())
                    .when(!interactive, |el| {
                        el.cursor_not_allowed().opacity(DISABLED_OPACITY)
                    })
                    .role(Role::RadioButton)
                    .aria_label(option.label.clone())
                    .aria_toggled(if checked {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .aria_selected(checked)
                    .aria_position_in_set(ix + 1)
                    .aria_size_of_set(count)
                    .when(interactive || focused, |el| {
                        el.track_focus(&handle.tab_index(0).tab_stop(stop == Some(ix)))
                    })
                    .on_hover(set_hover(hover))
                    .on_click(move |event, window, cx| {
                        if !interactive || is_enter(event) {
                            return;
                        }
                        window.focus(&focus_on_click, cx);
                        if !checked {
                            pick(ix, window, cx);
                        }
                    })
                    .when_some(option.icon, |el, icon| el.child(glyph(icon, px(ICON), fg)))
                    .when(!hidden, |el| {
                        el.child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(label),
                        )
                    })
                    .when(hidden, |el| {
                        let text = option.label.clone();
                        el.tooltip(move |window, cx| {
                            Tooltip::build(text.clone(), None, Some(theme), window, cx)
                        })
                        .tooltip_show_delay(TOOLTIP_DELAY)
                    })
                    .when(keyboard, |el| el.child(ring(0.0, item_radius, ring_color)))
                    .a11y_state(false, !interactive),
            );
        }

        div()
            .id(self.id.clone())
            .relative()
            .grid()
            .grid_cols(count.max(1) as u16)
            .when(self.block, |el| el.w_full())
            .when(!self.block, |el| el.flex_none())
            .h(px(height))
            .p(px(pad))
            .rounded(px(radius))
            .bg(theme.color(look::TRACK))
            .role(Role::RadioGroup)
            .aria_orientation(accesskit::Orientation::Horizontal)
            .when_some(self.label, |el, label| el.aria_label(label))
            .on_key_down(roving::on_key(handles, enabled, true, pick))
            .child(thumb)
            .children(items)
            .a11y_state(false, self.disabled)
    }
}
