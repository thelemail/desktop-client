use gpui_kit::component::input::{InputContentType, InputEvent, InputState};
use gpui_kit::{
    AppContext as _, Context, ElementId, Entity, EventEmitter, Focusable as _, IntoElement, Render,
    SharedString, Subscription, Window,
};
use thelemail_i18n::m;

use super::confirm::ConfirmDialog;
use super::dialog::DialogField;
use crate::field::TextField;
use crate::icons::IconName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkDialogEvent {
    Apply(SharedString),
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkAction {
    Apply,
    Remove,
    Unavailable,
}

pub(crate) fn link_action(value: &str, initial: Option<&str>) -> LinkAction {
    let editing = initial.is_some_and(|initial| !initial.is_empty());
    match (value.trim().is_empty(), editing) {
        (false, _) => LinkAction::Apply,
        (true, true) => LinkAction::Remove,
        (true, false) => LinkAction::Unavailable,
    }
}

pub struct LinkDialog {
    id: ElementId,
    open: bool,
    initial: Option<SharedString>,
    input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LinkDialogEvent> for LinkDialog {}

impl LinkDialog {
    pub fn new(id: impl Into<ElementId>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("example.com"));
        let subscriptions =
            vec![
                cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::PressEnter { .. } => this.apply(window, cx),
                        InputEvent::Change => cx.notify(),
                        _ => {}
                    }
                }),
            ];
        Self {
            id: id.into(),
            open: false,
            initial: None,
            input,
            _subscriptions: subscriptions,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn show(
        &mut self,
        initial: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.initial = initial.filter(|value| !value.is_empty());
        let value = self.initial.clone().unwrap_or_default();
        self.input.update(cx, |input, cx| {
            input.set_value(value, window, cx);
            input.select_all(window, cx);
        });
        self.open = true;
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.emit(LinkDialogEvent::Close);
            cx.notify();
        }
    }

    fn apply(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        let value = self.input.read(cx).value();
        if link_action(&value, self.initial.as_deref()) == LinkAction::Unavailable {
            return;
        }
        self.open = false;
        cx.emit(LinkDialogEvent::Apply(value.trim().to_string().into()));
        cx.notify();
    }
}

impl Render for LinkDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let apply = cx.entity().downgrade();
        let close = apply.clone();
        let value = self.input.read(cx).value();
        let action = link_action(&value, self.initial.as_deref());
        let title = if self.initial.is_some() {
            m::mail_link_edit_title()
        } else {
            m::mail_link_insert_title()
        };
        let confirm = match action {
            LinkAction::Remove => m::mail_link_remove(),
            LinkAction::Apply | LinkAction::Unavailable => m::mail_link_apply(),
        };
        let field_focus = self.input.read(cx).focus_handle(cx);
        let field_id = ElementId::NamedChild(self.id.clone().into(), "field".into());
        ConfirmDialog::new(self.id.clone(), title, confirm)
            .open(self.open)
            .icon(IconName::Link)
            .disabled(action == LinkAction::Unavailable)
            .focus_body(&field_focus)
            .on_confirm(move |window, cx| {
                let _ = apply.update(cx, |this, cx| this.apply(window, cx));
            })
            .on_close(move |_, cx| {
                let _ = close.update(cx, |this, cx| this.close(cx));
            })
            .child(DialogField::new(
                m::mail_link_address(),
                TextField::new(field_id, &self.input)
                    .label(m::mail_link_address())
                    .content_type(InputContentType::Url),
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_button_says_what_will_happen() {
        assert_eq!(link_action("example.com", None), LinkAction::Apply);
        assert_eq!(
            link_action("  ", Some("https://a.test")),
            LinkAction::Remove
        );
        assert_eq!(link_action("", None), LinkAction::Unavailable);
        assert_eq!(
            link_action(" x ", Some("https://a.test")),
            LinkAction::Apply
        );
    }
}
