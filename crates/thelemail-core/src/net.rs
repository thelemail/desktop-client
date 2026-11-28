use thelemail_api::{ApiResponse, TransportError, UploadBegin};

use crate::Core;
use crate::api::{CLIENT, CLIENT_HEADER};

pub fn upload_begin(core: &Core, mut req: UploadBegin) -> Result<String, TransportError> {
    req.headers
        .insert(CLIENT_HEADER.to_owned(), CLIENT.to_owned());
    core.net().upload_begin(req)
}

pub async fn upload_chunk(core: &Core, id: &str, chunk: Vec<u8>) -> Result<(), TransportError> {
    core.net().upload_chunk(id, chunk).await
}

pub async fn upload_finish(core: &Core, id: &str) -> Result<ApiResponse, TransportError> {
    core.net().upload_finish(id).await
}

pub fn upload_abort(core: &Core, id: &str) {
    core.net().upload_abort(id);
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub version: &'static str,
    pub keychain_device_bound: bool,
}

pub fn build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        keychain_device_bound: crate::keychain::hardening_available(),
    }
}
