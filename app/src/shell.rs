use gpui_kit::component::{ActiveTheme, Theme};
use gpui_kit::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
};

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
        Theme::sync_system_appearance(Some(window), cx);
        let nav = navigator(cx);
        let subscriptions = vec![
            cx.observe(&nav, |_, _, cx| cx.notify()),
            cx.observe_window_bounds(window, |this, window, cx| this.frame.changed(window, cx)),
            cx.observe_window_appearance(window, |_, window, cx| {
                Theme::sync_system_appearance(Some(window), cx);
            }),
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
            .bg(cx.theme().background)
            .text_color(cx.theme().muted_foreground)
            .child(screens::placeholder(&route))
    }
}
