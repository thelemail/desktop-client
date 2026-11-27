use std::io::Cursor;
use std::sync::Arc;

use gpui_kit::{App, Global};
use thelemail_core::locale::{self, Text};

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::services::Services;
use crate::window::show_main;

#[cfg(target_os = "macos")]
const ICON: &[u8] = include_bytes!("../assets/tray@2x.png");
#[cfg(not(target_os = "macos"))]
const ICON: &[u8] = include_bytes!("../assets/32x32.png");

const SHOW: &str = "show";
const QUIT: &str = "quit";

struct Tray {
    _icon: TrayIcon,
}

impl Global for Tray {}

enum Request {
    Show,
    Quit,
}

pub fn init(cx: &mut App) {
    let (tx, rx) = async_channel::unbounded::<Request>();
    let lang = cx.global::<Services>().core().locale().current();
    match build(lang, tx) {
        Ok(icon) => cx.set_global(Tray { _icon: icon }),
        Err(err) => {
            tracing::warn!("tray: unavailable: {err}");
            return;
        }
    }
    cx.spawn(async move |cx| {
        while let Ok(request) = rx.recv().await {
            cx.update(|cx| match request {
                Request::Show => show_main(cx),
                Request::Quit => cx.quit(),
            });
        }
    })
    .detach();
}

fn build(lang: &str, tx: async_channel::Sender<Request>) -> anyhow::Result<TrayIcon> {
    let show = MenuItem::with_id(SHOW, locale::text(lang, Text::ShowApp), true, None);
    let quit = MenuItem::with_id(QUIT, locale::text(lang, Text::Quit), true, None);
    let menu = Menu::with_items(&[&show, &PredefinedMenuItem::separator(), &quit])?;

    let menu_tx = tx.clone();
    MenuEvent::set_event_handler(Some(Arc::new(move |event: MenuEvent| {
        let request = if event.id == MenuId::new(SHOW) {
            Request::Show
        } else if event.id == MenuId::new(QUIT) {
            Request::Quit
        } else {
            return;
        };
        let _ = menu_tx.try_send(request);
    })));

    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if cfg!(windows)
            && let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
        {
            let _ = tx.try_send(Request::Show);
        }
    }));

    let builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Thelemail");
    #[cfg(target_os = "macos")]
    let builder = builder
        .with_icon_templated(icon()?)
        .with_menu_on_left_click(true);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.with_icon(icon()?).with_menu_on_left_click(false);
    Ok(builder.build()?)
}

fn icon() -> anyhow::Result<Icon> {
    let mut decoder = png::Decoder::new(Cursor::new(ICON));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info()?;
    let mut rgba = vec![0; reader.output_buffer_size().unwrap_or_default()];
    let info = reader.next_frame(&mut rgba)?;
    rgba.truncate(info.buffer_size());
    let scale = if cfg!(target_os = "macos") { 2.0 } else { 1.0 };
    Ok(Icon::from_rgba_with_scale(
        rgba,
        info.width,
        info.height,
        scale,
    )?)
}
