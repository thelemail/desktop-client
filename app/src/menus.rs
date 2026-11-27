use gpui_kit::base::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
use gpui_kit::{App, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType, Window, actions};

use crate::nav::navigator;
use crate::router::{AccountPage, SettingsSection};
#[cfg(debug_assertions)]
use crate::router::{DebugPage, GalleryTab, Route};
use crate::window::show_main;

#[cfg(target_os = "macos")]
macro_rules! secondary {
    ($key:literal) => {
        concat!("cmd-", $key)
    };
}

#[cfg(not(target_os = "macos"))]
macro_rules! secondary {
    ($key:literal) => {
        concat!("ctrl-", $key)
    };
}

const HELP_URL: &str = "https://thelemail.com/documentation";

actions!(
    thelemail,
    [
        About,
        Settings,
        Hide,
        HideOthers,
        ShowAll,
        Quit,
        Back,
        Forward,
        ToggleFullScreen,
        Minimize,
        Zoom,
        ShowMain,
        Help,
        TokenGallery
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys(bindings());
    cx.on_action(|_: &About, cx| open_settings(SettingsSection::About, cx));
    cx.on_action(|_: &Settings, cx| open_settings(SettingsSection::Account, cx));
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Back, cx| navigator(cx).update(cx, |nav, cx| nav.back(cx)));
    cx.on_action(|_: &Forward, cx| navigator(cx).update(cx, |nav, cx| nav.forward(cx)));
    cx.on_action(|_: &ToggleFullScreen, cx| {
        with_active_window(cx, |window| window.toggle_fullscreen())
    });
    cx.on_action(|_: &Minimize, cx| with_active_window(cx, |window| window.minimize_window()));
    cx.on_action(|_: &Zoom, cx| with_active_window(cx, |window| window.zoom_window()));
    cx.on_action(|_: &ShowMain, cx| show_main(cx));
    cx.on_action(|_: &Help, cx| cx.open_url(HELP_URL));
    #[cfg(debug_assertions)]
    cx.on_action(|_: &TokenGallery, cx| {
        show_main(cx);
        navigator(cx).update(cx, |nav, cx| {
            nav.push(Route::Debug(DebugPage::Tokens(GalleryTab::Colours)), cx)
        });
    });
    cx.set_menus(menus());
}

fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(secondary!(","), Settings, None),
        KeyBinding::new(secondary!("q"), Quit, None),
        KeyBinding::new(secondary!("["), Back, None),
        KeyBinding::new(secondary!("]"), Forward, None),
        KeyBinding::new(secondary!("m"), Minimize, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-h", Hide, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("f11", ToggleFullScreen, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("alt-left", Back, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("alt-right", Forward, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("f1", Help, None),
    ]
}

fn menus() -> Vec<Menu> {
    vec![
        Menu::new("Thelemail").items([
            MenuItem::action("About Thelemail", About),
            MenuItem::separator(),
            MenuItem::action("Settings…", Settings),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide Thelemail", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit Thelemail", Quit),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items(view_items()),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
            MenuItem::separator(),
            MenuItem::action("Thelemail", ShowMain),
        ]),
        Menu::new("Help").items([MenuItem::action("Thelemail Help", Help)]),
    ]
}

fn view_items() -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem::action("Back", Back),
        MenuItem::action("Forward", Forward),
        MenuItem::separator(),
        MenuItem::action("Enter Full Screen", ToggleFullScreen),
    ];
    if cfg!(debug_assertions) {
        items.push(MenuItem::separator());
        items.push(MenuItem::action("Token Gallery", TokenGallery));
    }
    items
}

fn open_settings(section: SettingsSection, cx: &mut App) {
    show_main(cx);
    navigator(cx).update(cx, |nav, cx| {
        nav.open_account_page(AccountPage::Settings(section), cx)
    });
}

fn with_active_window(cx: &mut App, f: impl FnOnce(&mut Window)) {
    if let Some(handle) = cx.active_window() {
        let _ = handle.update(cx, |_, window, _| f(window));
    }
}
