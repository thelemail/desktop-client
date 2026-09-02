#[cfg(all(debug_assertions, feature = "devtools"))]
mod devbridge;
mod ids;
mod keychain;
mod keystore;
pub mod mirror;
mod mirror_cmds;
mod net;
mod notify;
mod session;
mod shell;
mod sse;

use crate::mirror::Mirror;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use thelemail_api::{ApiConfig, Net};
use thelemail_keystore::Keystore;

fn allowed_navigation(url: &tauri::Url) -> bool {
    matches!(url.scheme(), "tauri" | "ipc") && url.host_str() == Some("localhost")
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Thelemail", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &separator, &quit])?;

    let icon =
        tauri::image::Image::from_bytes(include_bytes!("../icons/tray@2x.png")).expect("tray icon");

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Thelemail")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    eprintln!("[ui:startup] thelemail desktop starting");
    let config = ApiConfig::from_env().expect("desktop api configuration");
    let net = Net::new(config).expect("http client");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(net)
        .manage(Keystore::new())
        .manage(Mirror::default())
        .manage(sse::Streams::default())
        .invoke_handler(tauri::generate_handler![
            net::api_request,
            net::submission_request,
            net::blob_get,
            net::blob_put,
            net::ui_diagnostic,
            net::app_build_info,
            keystore::keystore_status,
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
            keystore::keystore_get_public_key,
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
            sse::realtime_close
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

            #[cfg(all(debug_assertions, feature = "devtools"))]
            devbridge::spawn(app.handle());

            let handle = app.handle().clone();
            let mut events = app.state::<Keystore>().subscribe();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = events.recv().await {
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
        .run(|app, event| {
            if let tauri::RunEvent::Reopen { .. } = event {
                show_main(app);
            }
        });
}
