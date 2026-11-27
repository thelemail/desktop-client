use gpui_kit::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
};

use thelemail_ui::theme::{ActiveTokens, Theme};
use thelemail_ui::tokens::ColorToken;

use crate::nav::{Navigator, navigator};
use crate::screens;
use crate::window::FrameSaver;

pub struct Shell {
    nav: Entity<Navigator>,
    frame: FrameSaver,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let nav = navigator(cx);
        let subscriptions = vec![
            cx.observe(&nav, |_, _, cx| cx.notify()),
            cx.observe_window_bounds(window, |this, window, cx| this.frame.changed(window, cx)),
            Theme::follow_system(window, cx),
        ];
        Self {
            nav,
            frame: FrameSaver::default(),
            _subscriptions: subscriptions,
        }
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let route = self.nav.read(cx).current().clone();
        div()
            .size_full()
            .bg(cx.tokens().color(ColorToken::Bg))
            .text_color(cx.tokens().color(ColorToken::FgMuted))
            .child(screens::render(&route, cx))
    }
}
