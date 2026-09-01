use serde::Deserialize;
use tauri::{AppHandle, State};
use thelemail_store::list::{MirrorMessage, MirrorRow, get_message, get_thread, list_mailbox};
use thelemail_store::search::SearchHit;

use crate::mirror::{Mirror, backfill, watch_inbox};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenMirrorArgs {
    pub account_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSyncArgs {
    pub account_id: String,
    pub access_token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchArgs {
    pub account_id: String,
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[tauri::command]
pub fn mirror_open(mirror: State<'_, Mirror>, args: OpenMirrorArgs) -> Result<(), String> {
    mirror.open(&args.account_id)
}

#[tauri::command]
pub fn mirror_close(mirror: State<'_, Mirror>, args: OpenMirrorArgs) -> Result<(), String> {
    crate::ids::account_id(&args.account_id)?;
    mirror.close(&args.account_id);
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListArgs {
    pub account_id: String,
    pub mailbox: String,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[tauri::command]
pub fn mirror_list(mirror: State<'_, Mirror>, args: ListArgs) -> Result<Vec<MirrorRow>, String> {
    crate::ids::account_id(&args.account_id)?;
    mirror.with_conn(&args.account_id, |conn| {
        list_mailbox(
            conn,
            &args.mailbox,
            args.direction.as_deref(),
            args.limit.unwrap_or(200),
        )
        .map_err(|e| e.to_string())
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageArgs {
    pub account_id: String,
    pub message_id: String,
}

#[tauri::command]
pub fn mirror_message(
    mirror: State<'_, Mirror>,
    args: MessageArgs,
) -> Result<Option<MirrorMessage>, String> {
    crate::ids::account_id(&args.account_id)?;
    let now = crate::mirror::now_iso();
    mirror.with_conn(&args.account_id, |conn| {
        get_message(conn, &args.message_id, &now).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn mirror_thread(
    mirror: State<'_, Mirror>,
    args: MessageArgs,
) -> Result<Vec<MirrorMessage>, String> {
    crate::ids::account_id(&args.account_id)?;
    let now = crate::mirror::now_iso();
    mirror.with_conn(&args.account_id, |conn| {
        get_thread(conn, &args.message_id, &now).map_err(|e| e.to_string())
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeArgs {
    pub account_id: String,
    #[serde(default)]
    pub date_floor: Option<String>,
}

#[tauri::command]
pub fn mirror_set_scope(mirror: State<'_, Mirror>, args: ScopeArgs) -> Result<(), String> {
    crate::ids::account_id(&args.account_id)?;
    mirror.set_date_floor(&args.account_id, args.date_floor.as_deref())
}

#[tauri::command]
pub fn mirror_scope(
    mirror: State<'_, Mirror>,
    args: OpenMirrorArgs,
) -> Result<Option<String>, String> {
    crate::ids::account_id(&args.account_id)?;
    mirror.date_floor(&args.account_id)
}

#[tauri::command]
pub fn mirror_search(
    mirror: State<'_, Mirror>,
    args: SearchArgs,
) -> Result<Vec<SearchHit>, String> {
    crate::ids::account_id(&args.account_id)?;
    mirror.search(&args.account_id, &args.query, args.limit)
}

#[tauri::command]
pub fn mirror_start_sync(
    app: AppHandle,
    mirror: State<'_, Mirror>,
    args: StartSyncArgs,
) -> Result<(), String> {
    crate::ids::account_id(&args.account_id)?;
    mirror.set_token(&args.account_id, &args.access_token);
    tauri::async_runtime::spawn(backfill(
        app.clone(),
        args.account_id.clone(),
        args.access_token,
    ));
    tauri::async_runtime::spawn(watch_inbox(app, args.account_id));
    Ok(())
}

#[tauri::command]
pub fn mirror_set_token(mirror: State<'_, Mirror>, args: StartSyncArgs) -> Result<(), String> {
    crate::ids::account_id(&args.account_id)?;
    mirror.set_token(&args.account_id, &args.access_token);
    Ok(())
}

#[tauri::command]
pub fn mirror_stop_watch(mirror: State<'_, Mirror>, args: OpenMirrorArgs) -> Result<(), String> {
    crate::ids::account_id(&args.account_id)?;
    mirror.stop_watch(&args.account_id);
    Ok(())
}
