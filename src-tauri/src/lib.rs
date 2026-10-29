#[cfg(all(debug_assertions, feature = "devtools"))]
mod devbridge;
mod ids;
mod keychain;
mod keystore;
mod locale;
pub mod mirror;
mod mirror_cmds;
mod net;
mod notify;
mod session;
mod shell;
mod sse;
mod updates;

use crate::mirror::Mirror;
use crate::updates::Updates;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use thelemail_api::{ApiConfig, Net};
use thelemail_keystore::{Broadcast, Keystore};

#[cfg(not(windows))]
fn allowed_navigation(url: &tauri::Url) -> bool {
    matches!(url.scheme(), "tauri" | "ipc") && url.host_str() == Some("localhost")
}

#[cfg(windows)]
fn allowed_navigation(url: &tauri::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && matches!(url.host_str(), Some("tauri.localhost" | "ipc.localhost"))
        && url.port().is_none()
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let lang = locale::current(app);
    let show = MenuItem::with_id(
        app,
        "show",
        locale::text(lang, locale::Text::ShowApp),
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        locale::text(lang, locale::Text::Quit),
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&show, &separator, &quit])?;
    app.state::<locale::UiLocale>()
        .attach_tray(show.clone(), quit.clone());

    let tray = TrayIconBuilder::with_id("main")
        .tooltip("Thelemail")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        });

    #[cfg(not(windows))]
    let tray = tray
        .icon(
            tauri::image::Image::from_bytes(include_bytes!("../icons/tray@2x.png"))
                .expect("tray icon"),
        )
        .icon_as_template(true)
        .show_menu_on_left_click(true);

    #[cfg(windows)]
    let tray = tray
        .icon(
            tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
                .expect("tray icon"),
        )
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });

    tray.build(app)?;
    Ok(())
}

pub fn run() {
    eprintln!("[ui:startup] thelemail desktop starting");
    #[cfg(windows)]
    notify::claim_app_id();
    let config = ApiConfig::from_env().expect("desktop api configuration");
    let net = Net::new(config).expect("http client");

    let builder = tauri::Builder::default();
    #[cfg(windows)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
        show_main(app)
    }));

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(net)
        .manage(Keystore::new())
        .manage(keystore::PersistentHalves::default())
        .manage(Mirror::default())
        .manage(locale::UiLocale::default())
        .manage(sse::Streams::default())
        .manage(Updates::new(&Mirror::root()))
        .invoke_handler(tauri::generate_handler![
            net::api_request,
            net::submission_request,
            net::api_blob_request,
            net::upload_begin,
            net::upload_chunk,
            net::upload_finish,
            net::upload_abort,
            net::ui_diagnostic,
            net::app_build_info,
            notify::notify_status,
            notify::notify_take_opened,
            keystore::keystore_status,
            locale::system_locales,
            locale::set_ui_locale,
            keystore::keystore_opaque_start_auth,
            keystore::keystore_opaque_finish_auth,
            keystore::keystore_opaque_complete_login_unlock,
            keystore::keystore_opaque_abandon_operation,
            keystore::keystore_opaque_start_registration,
            keystore::keystore_opaque_finish_registration,
            keystore::keystore_opaque_finalize_register,
            keystore::keystore_enroll_persistent,
            keystore::keystore_invalidate_persisted_vault,
            keystore::keystore_try_restore_from_persistent,
            keystore::keystore_disable_persistent,
            keystore::keystore_clear,
            keystore::keystore_lock,
            keystore::keystore_clear_all,
            keystore::keystore_decrypt,
            keystore::keystore_load_alias_keys,
            keystore::keystore_unload_alias_keys,
            keystore::keystore_reformat_key_with_uids,
            keystore::keystore_attachment_header,
            keystore::keystore_attachment_bytes,
            keystore::keystore_encrypt,
            keystore::keystore_encrypt_to_keys,
            keystore::keystore_sign_detached,
            keystore::keystore_get_public_key,
            keystore::keystore_load_account_keys,
            keystore::keystore_list_account_keys,
            keystore::keystore_prepare_key_rotation,
            keystore::keystore_commit_key_rotation,
            keystore::keystore_abandon_key_rotation,
            keystore::keystore_opaque_recovery_setup_start,
            keystore::keystore_opaque_recovery_setup_finish,
            keystore::keystore_opaque_complete_recovery_unlock,
            keystore::keystore_opaque_prepare_credential_reset,
            keystore::keystore_opaque_finish_credential_reset,
            keystore::keystore_opaque_password_change_start,
            keystore::keystore_opaque_password_change_finish,
            keystore::keystore_opaque_password_change_commit,
            keystore::keystore_discard_recovery,
            keystore::keystore_abandon_password_change,
            keystore::keystore_create_alias_key,
            keystore::keystore_commit_reformatted_key,
            mirror_cmds::mirror_open,
            mirror_cmds::mirror_close,
            mirror_cmds::mirror_search,
            mirror_cmds::mirror_list,
            mirror_cmds::mirror_set_scope,
            mirror_cmds::mirror_scope,
            mirror_cmds::mirror_message,
            mirror_cmds::mirror_thread,
            mirror_cmds::mirror_start_sync,
            mirror_cmds::mirror_set_token,
            mirror_cmds::mirror_stop_watch,
            session::session_persist,
            session::session_restore,
            session::session_forget,
            shell::open_external,
            shell::save_bytes,
            sse::realtime_open,
            sse::realtime_close,
            updates::updates_status,
            updates::updates_check,
            updates::updates_snooze,
            updates::updates_install
        ])
        .setup(|app| {
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("Thelemail")
                .inner_size(1280.0, 840.0)
                .min_inner_size(960.0, 600.0)
                .center()
                .visible(false)
                .disable_drag_drop_handler()
                .on_navigation(allowed_navigation)
                .build()?;
            window.show()?;

            let hide_on_close = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = hide_on_close.hide();
                }
            });

            build_tray(app.handle())?;
            notify::prepare(app.handle());
            updates::spawn(app.handle());

            #[cfg(all(debug_assertions, feature = "devtools"))]
            devbridge::spawn(app.handle());

            let handle = app.handle().clone();
            let mut events = app.state::<Keystore>().subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    let event = match events.recv().await {
                        Ok(event) => event,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    if let Broadcast::AccountKeysChanged {
                        account_id,
                        changed: true,
                        ..
                    } = &event
                    {
                        let app = handle.clone();
                        let id = account_id.clone();
                        tauri::async_runtime::spawn_blocking(move || {
                            keystore::save_key_history(&app.state::<Keystore>(), &id);
                            keystore::rewrap_persisted(
                                &app.state::<Keystore>(),
                                &app.state::<keystore::PersistentHalves>(),
                                &id,
                            );
                        });
                        tauri::async_runtime::spawn(mirror::retry_undecryptable(
                            handle.clone(),
                            account_id.clone(),
                        ));
                    }
                    if let Broadcast::VaultChanged { account_id, .. } = &event {
                        let app = handle.clone();
                        let id = account_id.clone();
                        tauri::async_runtime::spawn_blocking(move || {
                            keystore::save_key_history(&app.state::<Keystore>(), &id);
                        });
                    }
                    let _ = handle.emit("keystore", event);
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("thelemail desktop failed to start")
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => show_main(app),
            tauri::RunEvent::Exit => {
                app.state::<sse::Streams>().close_all();
                app.state::<Mirror>().shutdown();
            }
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use super::allowed_navigation;

    fn allowed(raw: &str) -> bool {
        allowed_navigation(&raw.parse().expect("url"))
    }

    #[test]
    fn navigation_stays_inside_the_app() {
        for foreign in [
            "https://thelemail.com/",
            "http://localhost/",
            "http://tauri.localhost.evil.com/",
            "file:///C:/Windows/",
            "javascript:alert(1)",
        ] {
            assert!(
                !allowed(foreign),
                "{foreign} must not load in the app window"
            );
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn the_app_origin_loads() {
        assert!(allowed("tauri://localhost/index.html"));
        assert!(allowed("ipc://localhost/plugin"));
        assert!(!allowed("http://tauri.localhost/"));
    }

    #[cfg(windows)]
    #[test]
    fn the_webview2_app_origin_loads() {
        assert!(allowed("http://tauri.localhost/index.html"));
        assert!(allowed("http://ipc.localhost/plugin"));
        assert!(!allowed("http://tauri.localhost:8080/"));
        assert!(!allowed("tauri://localhost/"));
    }
}
