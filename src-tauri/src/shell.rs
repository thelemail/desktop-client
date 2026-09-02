use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use url::Url;

pub const NOTIFICATION_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.Notifications-Settings.extension";

#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|_| "not a url".to_owned())?;
    if parsed.as_str() == NOTIFICATION_SETTINGS_URL {
        return app
            .opener()
            .open_url(parsed.as_str(), None::<&str>)
            .map_err(|e| e.to_string());
    }
    match parsed.scheme() {
        "https" | "http" | "mailto" => app
            .opener()
            .open_url(parsed.as_str(), None::<&str>)
            .map_err(|e| e.to_string()),
        other => Err(format!("refusing to open a {other} url")),
    }
}

pub(crate) fn safe_filename(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | '\0'))
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        "attachment".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveBytesArgs {
    pub filename: String,
    pub bytes: Vec<u8>,
}

#[tauri::command]
pub async fn save_bytes(app: AppHandle, args: SaveBytesArgs) -> Result<bool, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(safe_filename(&args.filename))
        .save_file(move |path| {
            let _ = tx.send(path);
        });

    let Some(path) = rx.await.map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    std::fs::write(path, args.bytes).map_err(|e| e.to_string())?;
    Ok(true)
}
