use tauri::State;
use thelemail_api::{ApiRequest, ApiResponse, Net, TransportError};

#[tauri::command]
pub async fn api_request(
    net: State<'_, Net>,
    req: ApiRequest,
) -> Result<ApiResponse, TransportError> {
    net.request(req).await
}

#[tauri::command]
pub async fn submission_request(
    net: State<'_, Net>,
    req: ApiRequest,
) -> Result<ApiResponse, TransportError> {
    net.submit(req).await
}

#[tauri::command]
pub async fn blob_get(net: State<'_, Net>, url: String) -> Result<Vec<u8>, TransportError> {
    net.blob_get(&url).await
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiDiagnostic {
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub detail: Option<String>,
}

#[tauri::command]
pub fn ui_diagnostic(report: UiDiagnostic) {
    let truncate = |s: &str| s.chars().take(400).collect::<String>();
    eprintln!(
        "[ui:{}] {}{}",
        report.kind,
        truncate(&report.message),
        report
            .detail
            .as_deref()
            .map(|d| format!(" | {}", truncate(d)))
            .unwrap_or_default()
    );
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobPutArgs {
    pub url: String,
    pub bytes: Vec<u8>,
    #[serde(default)]
    pub content_type: Option<String>,
}

#[tauri::command]
pub async fn blob_put(net: State<'_, Net>, args: BlobPutArgs) -> Result<u16, TransportError> {
    net.blob_put(&args.url, args.bytes, args.content_type).await
}

#[tauri::command]
pub fn app_build_info() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "keychainDeviceBound": crate::keychain::hardening_available(),
    })
}
