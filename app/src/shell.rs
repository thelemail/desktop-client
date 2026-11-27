use gpui_kit::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window,
    div,
};

use thelemail_ui::theme::{ActiveTokens, Theme};
use thelemail_ui::tokens::ColorToken;

#[cfg(debug_assertions)]
use crate::avatar_gallery::AvatarGallery;
#[cfg(debug_assertions)]
use crate::brand_gallery::BrandGallery;
#[cfg(debug_assertions)]
use crate::button_gallery::ButtonGallery;
#[cfg(debug_assertions)]
use crate::choice_gallery::ChoiceGallery;
#[cfg(debug_assertions)]
use crate::field_gallery::FieldGallery;
#[cfg(debug_assertions)]
use crate::icon_gallery::IconGallery;
#[cfg(debug_assertions)]
use crate::motion_gallery::MotionGallery;
use crate::nav::{Navigator, navigator};
#[cfg(debug_assertions)]
use crate::overlay_gallery::OverlayGallery;
#[cfg(debug_assertions)]
use crate::router::{DebugPage, GalleryTab, Route};
use crate::screens;
#[cfg(debug_assertions)]
use crate::states_gallery::StatesGallery;
use crate::window::FrameSaver;

pub struct Shell {
    nav: Entity<Navigator>,
    frame: FrameSaver,
    #[cfg(debug_assertions)]
    motion: Option<Entity<MotionGallery>>,
    #[cfg(debug_assertions)]
    icons: Option<Entity<IconGallery>>,
    #[cfg(debug_assertions)]
    buttons: Option<Entity<ButtonGallery>>,
    #[cfg(debug_assertions)]
    fields: Option<Entity<FieldGallery>>,
    #[cfg(debug_assertions)]
    choices: Option<Entity<ChoiceGallery>>,
    #[cfg(debug_assertions)]
    overlays: Option<Entity<OverlayGallery>>,
    #[cfg(debug_assertions)]
    states: Option<Entity<StatesGallery>>,
    #[cfg(debug_assertions)]
    avatars: Option<Entity<AvatarGallery>>,
    #[cfg(debug_assertions)]
    brand: Option<Entity<BrandGallery>>,
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
            #[cfg(debug_assertions)]
            buttons: None,
            #[cfg(debug_assertions)]
            fields: None,
            #[cfg(debug_assertions)]
            choices: None,
            #[cfg(debug_assertions)]
            overlays: None,
            #[cfg(debug_assertions)]
            states: None,
            #[cfg(debug_assertions)]
            avatars: None,
            #[cfg(debug_assertions)]
            brand: None,
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
        #[cfg(debug_assertions)]
        if route == Route::Debug(DebugPage::Tokens(GalleryTab::Buttons)) {
            let buttons = self
                .buttons
                .get_or_insert_with(|| cx.new(|_| ButtonGallery::new()))
                .clone();
            return root.child(buttons);
        }
        #[cfg(debug_assertions)]
        if route == Route::Debug(DebugPage::Tokens(GalleryTab::Fields)) {
            let fields = self
                .fields
                .get_or_insert_with(|| cx.new(|cx| FieldGallery::new(window, cx)))
                .clone();
            return root.child(fields);
        }
        #[cfg(debug_assertions)]
        if route == Route::Debug(DebugPage::Tokens(GalleryTab::Choices)) {
            let choices = self
                .choices
                .get_or_insert_with(|| cx.new(|cx| ChoiceGallery::new(window, cx)))
                .clone();
            return root.child(choices);
        }
        #[cfg(debug_assertions)]
        if route == Route::Debug(DebugPage::Tokens(GalleryTab::Overlays)) {
            let overlays = self
                .overlays
                .get_or_insert_with(|| cx.new(|cx| OverlayGallery::new(window, cx)))
                .clone();
            return root.child(overlays);
        }
        #[cfg(debug_assertions)]
        if route == Route::Debug(DebugPage::Tokens(GalleryTab::States)) {
            let states = self
                .states
                .get_or_insert_with(|| cx.new(|_| StatesGallery::new()))
                .clone();
            return root.child(states);
        }
        #[cfg(debug_assertions)]
        if route == Route::Debug(DebugPage::Tokens(GalleryTab::Avatars)) {
            let avatars = self
                .avatars
                .get_or_insert_with(|| cx.new(|cx| AvatarGallery::new(window, cx)))
                .clone();
            return root.child(avatars);
        }
        #[cfg(debug_assertions)]
        if route == Route::Debug(DebugPage::Tokens(GalleryTab::Brand)) {
            let brand = self
                .brand
                .get_or_insert_with(|| cx.new(|cx| BrandGallery::new(window, cx)))
                .clone();
            return root.child(brand);
        }
        let _ = window;
        root.child(screens::render(&route, cx))
    }
}
