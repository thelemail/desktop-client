use tauri::State;
use tauri::ipc::{InvokeBody, Request};
use thelemail_api::{ApiRequest, ApiResponse, Net, TransportError, UploadBegin};

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

#[tauri::command]
pub fn upload_begin(net: State<'_, Net>, req: UploadBegin) -> Result<String, TransportError> {
    net.upload_begin(req)
}

#[tauri::command]
pub async fn upload_chunk(net: State<'_, Net>, request: Request<'_>) -> Result<(), TransportError> {
    let id = request
        .headers()
        .get("x-upload-id")
        .and_then(|v| v.to_str().ok())
        .ok_or(TransportError::InvalidRequest)?
        .to_owned();
    let InvokeBody::Raw(chunk) = request.body() else {
        net.upload_abort(&id);
        return Err(TransportError::InvalidRequest);
    };
    net.upload_chunk(&id, chunk.clone()).await
}

#[tauri::command]
pub async fn upload_finish(net: State<'_, Net>, id: String) -> Result<ApiResponse, TransportError> {
    net.upload_finish(&id).await
}

#[tauri::command]
pub fn upload_abort(net: State<'_, Net>, id: String) {
    net.upload_abort(&id);
}

#[tauri::command]
pub fn app_build_info() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "keychainDeviceBound": crate::keychain::hardening_available(),
    })
}
