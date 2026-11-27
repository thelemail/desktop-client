use std::time::{Duration, Instant};

use gpui_kit::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, Role, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use thelemail_i18n::m;

use super::layer::{self, Layer};
use super::toast::{Toast, Toasts};
use crate::a11y::A11yStateExt as _;
use crate::spinner::Spinner;
use crate::state::{Delayed, INDICATOR_DELAY};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_SANS};

pub const ARRIVAL: Duration = Duration::from_millis(4500);
const SPINNER: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    Switch,
    SignOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub kind: TransitionKind,
    pub address: Option<SharedString>,
}

impl Transition {
    pub fn switch(address: impl Into<SharedString>) -> Self {
        Self {
            kind: TransitionKind::Switch,
            address: Some(address.into()),
        }
    }

    pub fn sign_out() -> Self {
        Self {
            kind: TransitionKind::SignOut,
            address: None,
        }
    }

    pub fn status(&self) -> SharedString {
        match (self.kind, self.address.as_ref()) {
            (TransitionKind::SignOut, _) => m::account_transition_signing_out().into(),
            (TransitionKind::Switch, Some(address)) => {
                m::account_transition_opening(address).into()
            }
            (TransitionKind::Switch, None) => m::account_transition_opening_plain().into(),
        }
    }
}

pub fn announce_arrival(email: &str, window: &mut Window, cx: &mut App) {
    Toasts::push(
        window,
        cx,
        Toast::success(m::handoff_tab_signed_in(email)).duration(ARRIVAL),
    );
}

#[derive(IntoElement)]
pub struct AccountTransition {
    id: ElementId,
    current: Option<Transition>,
    theme: Option<Theme>,
}

impl AccountTransition {
    pub fn new(id: impl Into<ElementId>, current: Option<Transition>) -> Self {
        Self {
            id: id.into(),
            current,
            theme: None,
        }
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    fn child_id(&self, name: &'static str) -> ElementId {
        ElementId::NamedChild(self.id.clone().into(), name.into())
    }
}

impl RenderOnce for AccountTransition {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let delayed = window.use_keyed_state(self.child_id("delayed"), cx, |_, _| {
            Delayed::<Option<Transition>>::with_timing(INDICATOR_DELAY, Duration::ZERO)
        });
        let now = Instant::now();
        let current = self.current.clone();
        let wake = delayed.update(cx, |delayed, _| {
            delayed.update(current.is_some(), current.clone(), now)
        });
        if let Some(at) = wake {
            let wait = at.saturating_duration_since(now);
            window
                .spawn(cx, async move |cx| {
                    cx.background_executor().timer(wait).await;
                    let _ = cx.update(|window, _| window.refresh());
                })
                .detach();
        }
        let cover = layer::cover(&self.id, window, cx);
        let Some(transition) = current else {
            cover.clear(window, cx);
            return div().into_any_element();
        };
        let status = delayed.read(cx).visible().then(|| {
            div()
                .id(self.child_id("status"))
                .flex()
                .items_center()
                .gap(px(10.0))
                .font_family(FONT_SANS.name)
                .text_size(px(13.5 * FONT_SANS.size_adjust))
                .text_color(theme.color(ColorToken::FgMuted))
                .role(Role::Status)
                .aria_label(transition.status())
                .child(Spinner::new(
                    self.child_id("spinner"),
                    SPINNER,
                    theme.color(ColorToken::Pine600),
                ))
                .child(transition.status())
        });
        let viewport = window.viewport_size();
        layer::host(
            Layer::Transition,
            div()
                .id(self.child_id("cover"))
                .occlude()
                .w(viewport.width)
                .h(viewport.height)
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.color(ColorToken::Bg))
                .role(Role::Pane)
                .children(status)
                .child(layer::tracker(cover))
                .a11y_state(true, false),
        )
    }
}
