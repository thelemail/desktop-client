use gpui_kit::{App, QuitMode, application};
use thelemail_api::ApiConfig;
use thelemail_core::notify::{self, NotificationTarget};
use thelemail_core::{Core, CoreEvent};

use crate::bridge::{self, HubEvent};
use crate::instance::{self, Claim};
use crate::nav::{self, navigator};
use crate::router::{AuthPage, Route};
use crate::services::{self, Services};
use crate::window::{self, show_main};
use crate::{appearance, language, logging, menus, tray};

pub fn run() {
    let log = logging::init();
    let runtime = match services::runtime() {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::error!("boot: cannot start the core runtime: {err}");
            return;
        }
    };

    let listener = match runtime.block_on(instance::claim()) {
        Ok(Claim::Running) => {
            tracing::info!("boot: already running, handed over");
            return;
        }
        Ok(Claim::First(listener)) => Some(listener),
        Err(err) => {
            tracing::warn!("boot: single instance unavailable: {err}");
            None
        }
    };

    #[cfg(windows)]
    notify::claim_app_id();

    let config = match ApiConfig::from_env() {
        Ok(config) => config,
        Err(err) => {
            tracing::error!("boot: invalid API configuration: {err}");
            return;
        }
    };
    let core = {
        let _enter = runtime.enter();
        let core = match Core::new(config) {
            Ok(core) => core,
            Err(err) => {
                tracing::error!("boot: cannot build the network client: {err}");
                return;
            }
        };
        core.start();
        core
    };

    let (show_tx, show_rx) = async_channel::unbounded::<()>();
    if let Some(listener) = listener {
        runtime.spawn(instance::serve(listener, show_tx));
    }

    let start = initial_route();
    let app = application()
        .with_assets(thelemail_ui::icons::Assets)
        .with_quit_mode(QuitMode::Explicit);
    app.on_reopen(show_main);
    app.run(move |cx| {
        gpui_kit::init(cx);
        thelemail_ui::overlay::init(cx);
        thelemail_ui::fonts::register(cx);
        let handle = runtime.handle().clone();
        cx.set_global(Services::new(core.clone(), runtime, log));
        appearance::init(cx);
        language::init(cx);
        cx.on_app_quit(|cx| {
            Services::shutdown(cx);
            async {}
        })
        .detach();

        nav::init(start, cx);
        let hub = bridge::start(cx);
        cx.subscribe(&hub, |_, event, cx| {
            if let HubEvent::Core(CoreEvent::NotificationOpened(target)) = event {
                open_notification(target, cx);
            }
        })
        .detach();

        window::init(cx);
        menus::init(cx);
        tray::init(cx);

        {
            let _enter = handle.enter();
            notify::prepare(&core);
        }

        cx.spawn(async move |cx| {
            while show_rx.recv().await.is_ok() {
                cx.update(show_main);
            }
        })
        .detach();

        window::open_main(cx);
        if let Some(target) = notify::take_opened() {
            open_notification(&target, cx);
        }
    });
}

fn open_notification(target: &NotificationTarget, cx: &mut App) {
    show_main(cx);
    navigator(cx).update(cx, |nav, cx| {
        nav.open_target(target, cx);
    });
}

fn initial_route() -> Route {
    #[cfg(debug_assertions)]
    {
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--route"
                && let Some(route) = args.next().as_deref().and_then(Route::parse)
            {
                return route;
            }
        }
    }
    Route::Auth(AuthPage::Welcome)
}
