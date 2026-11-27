use gpui_kit::component::input::{Copy, Cut, InputContentType, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, ElementId, Entity, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Render, Role, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use thelemail_i18n::m;

use super::{FieldSize, TextField};
use crate::icons::{IconName, StateIcon};
use crate::motion::{BASE, EASE, Spec, ease_color};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, RADIUS_CONTROL_COMPACT};

const REVEAL: f32 = 40.0;
const REVEAL_ICON: f32 = 17.0;
const RING_WIDTH: f32 = 2.0;
const COLOR: Spec = Spec::new(BASE, EASE);

pub struct PasswordField {
    id: ElementId,
    state: Entity<InputState>,
    reveal_focus: FocusHandle,
    revealed: bool,
    hovered: bool,
    size: FieldSize,
    invalid: bool,
    disabled: bool,
    label: Option<SharedString>,
    theme: Option<Theme>,
}

impl PasswordField {
    pub fn new(id: impl Into<ElementId>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| InputState::new(window, cx).masked(true).context_menu(false));
        Self {
            id: id.into(),
            state,
            reveal_focus: cx.focus_handle(),
            revealed: false,
            hovered: false,
            size: FieldSize::default(),
            invalid: false,
            disabled: false,
            label: None,
            theme: None,
        }
    }

    pub fn state(&self) -> &Entity<InputState> {
        &self.state
    }

    pub fn size(mut self, size: FieldSize) -> Self {
        self.size = size;
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

    pub fn set_invalid(&mut self, invalid: bool, cx: &mut Context<Self>) {
        self.invalid = invalid;
        cx.notify();
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        cx.notify();
    }

    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.revealed = !self.revealed;
        let masked = !self.revealed;
        self.state
            .update(cx, |state, cx| state.set_masked(masked, window, cx));
        cx.notify();
    }

    fn child_id(&self, name: &'static str) -> ElementId {
        ElementId::NamedChild(self.id.clone().into(), name.into())
    }
}

impl Render for PasswordField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let lit = self.hovered && !self.disabled;
        let fg = ease_color(
            self.child_id("reveal-fg"),
            theme.color(if lit {
                ColorToken::Fg
            } else {
                ColorToken::FgFaint
            }),
            COLOR,
            window,
            cx,
        );
        let bg_target = if lit {
            theme.color(ColorToken::BgSunken)
        } else {
            theme.color(ColorToken::BgSunken).alpha(0.0)
        };
        let bg = ease_color(self.child_id("reveal-bg"), bg_target, COLOR, window, cx);
        let ring = self.reveal_focus.is_focused(window) && window.last_input_was_keyboard();
        let label = if self.revealed {
            m::common_hide_password()
        } else {
            m::common_show_password()
        };
        let disabled = self.disabled;
        let reveal = div()
            .id(self.child_id("reveal"))
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .size(px(REVEAL))
            .rounded(px(RADIUS_CONTROL_COMPACT))
            .bg(bg)
            .when(disabled, |el| el.opacity(0.5))
            .when(!disabled, |el| el.cursor_pointer())
            .role(Role::Button)
            .aria_label(label)
            .when(!disabled, |el| {
                el.track_focus(&self.reveal_focus.clone().tab_stop(true))
                    .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx)))
            })
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.hovered = *hovered;
                cx.notify();
            }))
            .on_mouse_down(gpui_kit::MouseButton::Left, |_, window, _| {
                window.prevent_default()
            })
            .child(
                StateIcon::new(self.child_id("eye"), IconName::Eye)
                    .active_icon(IconName::EyeSlash)
                    .active(self.revealed)
                    .size(REVEAL_ICON)
                    .color(fg),
            )
            .when(ring, |el| {
                el.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(RADIUS_CONTROL_COMPACT))
                        .border(px(RING_WIDTH))
                        .border_color(theme.color(ColorToken::FocusRing)),
                )
            });
        div()
            .id(self.id.clone())
            .w_full()
            .capture_action(|_: &Copy, _, cx| cx.stop_propagation())
            .capture_action(|_: &Cut, _, cx| cx.stop_propagation())
            .child(
                TextField::new(self.child_id("field"), &self.state)
                    .size(self.size)
                    .invalid(self.invalid)
                    .disabled(self.disabled)
                    .mono()
                    .content_type(InputContentType::Password)
                    .theme(theme)
                    .when_some(self.label.clone(), |field, label| field.label(label))
                    .trailing(reveal, REVEAL),
            )
    }
}
