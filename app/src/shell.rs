use gpui_kit::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window,
    div,
};

use thelemail_ui::theme::{ActiveTokens, Theme};
use thelemail_ui::tokens::ColorToken;

#[cfg(debug_assertions)]
use crate::icon_gallery::IconGallery;
#[cfg(debug_assertions)]
use crate::motion_gallery::MotionGallery;
use crate::nav::{Navigator, navigator};
#[cfg(debug_assertions)]
use crate::router::{DebugPage, GalleryTab, Route};
use crate::screens;
use crate::window::FrameSaver;

pub struct Shell {
    nav: Entity<Navigator>,
    frame: FrameSaver,
    #[cfg(debug_assertions)]
    motion: Option<Entity<MotionGallery>>,
    #[cfg(debug_assertions)]
    icons: Option<Entity<IconGallery>>,
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
            #[cfg(debug_assertions)]
            motion: None,
            #[cfg(debug_assertions)]
            icons: None,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let route = self.nav.read(cx).current().clone();
        let root = div()
            .size_full()
            .bg(cx.tokens().color(ColorToken::Bg))
            .text_color(cx.tokens().color(ColorToken::FgMuted));
        #[cfg(debug_assertions)]
        if route == Route::Debug(DebugPage::Tokens(GalleryTab::Motion)) {
            let motion = self
                .motion
                .get_or_insert_with(|| cx.new(|cx| MotionGallery::new(window, cx)))
                .clone();
            return root.child(motion);
        }
        #[cfg(debug_assertions)]
        if route == Route::Debug(DebugPage::Tokens(GalleryTab::Icons)) {
            let icons = self
                .icons
                .get_or_insert_with(|| cx.new(|_| IconGallery::new()))
                .clone();
            return root.child(icons);
        }
        let _ = window;
        root.child(screens::render(&route, cx))
    }
}
