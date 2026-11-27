use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ElementId, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled, Window, div, px, relative,
};
use thelemail_i18n::m;

use super::dialog::{Action, Dialog, DialogSize, Tone};
use super::focus;
use crate::button::{Button, Variant};
use crate::icons::IconName;
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_SANS, RADIUS_SM};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfirmTone {
    Danger,
    #[default]
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Initial {
    Cancel,
    Confirm,
    Body,
}

pub(crate) fn initial(tone: ConfirmTone, body_focus: bool) -> Initial {
    match (body_focus, tone) {
        (true, _) => Initial::Body,
        (false, ConfirmTone::Danger) => Initial::Cancel,
        (false, ConfirmTone::Neutral) => Initial::Confirm,
    }
}

#[derive(IntoElement)]
pub struct ConfirmDialog {
    id: ElementId,
    open: bool,
    tone: ConfirmTone,
    icon: Option<IconName>,
    title: SharedString,
    subtitle: Option<SharedString>,
    body: Vec<AnyElement>,
    confirm_label: SharedString,
    cancel_label: Option<SharedString>,
    busy: bool,
    disabled: bool,
    error: Option<SharedString>,
    wide: bool,
    body_focus: Option<FocusHandle>,
    on_confirm: Option<Action>,
    on_cancel: Option<Action>,
    on_close: Option<Action>,
    theme: Option<Theme>,
}

impl ConfirmDialog {
    pub fn new(
        id: impl Into<ElementId>,
        title: impl Into<SharedString>,
        confirm_label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            open: false,
            tone: ConfirmTone::default(),
            icon: None,
            title: title.into(),
            subtitle: None,
            body: Vec::new(),
            confirm_label: confirm_label.into(),
            cancel_label: None,
            busy: false,
            disabled: false,
            error: None,
            wide: false,
            body_focus: None,
            on_confirm: None,
            on_cancel: None,
            on_close: None,
            theme: None,
        }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn tone(mut self, tone: ConfirmTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn danger(self) -> Self {
        self.tone(ConfirmTone::Danger)
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn subtitle(mut self, text: impl Into<SharedString>) -> Self {
        self.subtitle = Some(text.into());
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.body.push(child.into_any_element());
        self
    }

    pub fn cancel_label(mut self, label: impl Into<SharedString>) -> Self {
        self.cancel_label = Some(label.into());
        self
    }

    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn error(mut self, error: Option<impl Into<SharedString>>) -> Self {
        self.error = error.map(Into::into);
        self
    }

    pub fn wide(mut self) -> Self {
        self.wide = true;
        self
    }

    pub fn focus_body(mut self, handle: &FocusHandle) -> Self {
        self.body_focus = Some(handle.clone());
        self
    }

    pub fn on_confirm(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_confirm = Some(Rc::new(handler));
        self
    }

    pub fn on_cancel(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_cancel = Some(Rc::new(handler));
        self
    }

    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
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

impl RenderOnce for ConfirmDialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let danger = self.tone == ConfirmTone::Danger;
        let cancel_focus = focus::handle(&self.id, "cancel", window, cx);
        let confirm_focus = focus::handle(&self.id, "confirm", window, cx);
        let first = match initial(self.tone, self.body_focus.is_some()) {
            Initial::Body => self.body_focus.clone().unwrap_or(confirm_focus.clone()),
            Initial::Cancel => cancel_focus.clone(),
            Initial::Confirm => confirm_focus.clone(),
        };
        let cancel: Option<Action> = self.on_cancel.clone().or_else(|| self.on_close.clone());
        let busy = self.busy;
        let confirm = self.on_confirm.clone();

        let error = self.error.clone().map(|error| {
            div()
                .id(self.child_id("error"))
                .mt(px(4.0))
                .px(px(12.0))
                .py(px(9.0))
                .rounded(px(RADIUS_SM))
                .bg(theme.color(ColorToken::Danger100))
                .text_color(theme.color(ColorToken::Danger700))
                .font_family(FONT_SANS.name)
                .text_size(px(13.0 * FONT_SANS.size_adjust))
                .line_height(relative(1.5))
                .role(Role::Alert)
                .aria_label(error.clone())
                .child(error)
        });

        let cancel_click = cancel.clone();
        let cancel_button = Button::new(self.child_id("cancel-button"))
            .variant(Variant::Neutral)
            .label(
                self.cancel_label
                    .clone()
                    .unwrap_or_else(|| m::common_cancel().into()),
            )
            .disabled(busy)
            .track_focus(&cancel_focus)
            .theme(theme)
            .on_click(move |_, window, cx| {
                if let Some(cancel) = cancel_click.as_ref() {
                    cancel(window, cx);
                }
            });
        let confirm_button = Button::new(self.child_id("confirm-button"))
            .variant(if danger {
                Variant::Danger
            } else {
                Variant::Primary
            })
            .label(self.confirm_label.clone())
            .busy(busy)
            .busy_label(m::mail_confirm_working())
            .disabled(self.disabled)
            .track_focus(&confirm_focus)
            .theme(theme)
            .on_click(move |_, window, cx| {
                if busy {
                    return;
                }
                if let Some(confirm) = confirm.as_ref() {
                    confirm(window, cx);
                }
            });

        let close = cancel;
        Dialog::new(self.id.clone())
            .open(self.open)
            .alert(danger)
            .tone(if danger { Tone::Danger } else { Tone::Brand })
            .when_some(self.icon, |dialog, icon| dialog.icon(icon))
            .title(self.title.clone())
            .when_some(self.subtitle.clone(), |dialog, sub| dialog.subtitle(sub))
            .size(if self.wide {
                DialogSize::Wide
            } else {
                DialogSize::Md
            })
            .dismissible(!busy)
            .initial_focus(&first)
            .theme(theme)
            .on_close(move |window, cx| {
                if let Some(close) = close.as_ref() {
                    close(window, cx);
                }
            })
            .children(self.body)
            .children(error)
            .footer(cancel_button)
            .footer(confirm_button)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn danger_lands_on_the_way_out() {
        assert_eq!(initial(ConfirmTone::Danger, false), Initial::Cancel);
        assert_eq!(initial(ConfirmTone::Neutral, false), Initial::Confirm);
        assert_eq!(initial(ConfirmTone::Danger, true), Initial::Body);
    }
}
