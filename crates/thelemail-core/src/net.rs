use thelemail_api::{ApiRequest, ApiResponse, TransportError, UploadBegin};

use crate::Core;

const CLIENT_HEADER: &str = "X-Client";
const CLIENT: &str = "desktop";

fn tagged(mut req: ApiRequest) -> ApiRequest {
    req.headers
        .insert(CLIENT_HEADER.to_owned(), CLIENT.to_owned());
    req
}

pub async fn api_request(core: &Core, req: ApiRequest) -> Result<ApiResponse, TransportError> {
    let account = crate::session::auth_account(&req);
    let resp = core.net().request(tagged(req)).await;
    if let Some(account_id) = account {
        crate::session::sync_persisted(core.net(), &account_id);
    }
    resp
}

pub async fn submission_request(
    core: &Core,
    req: ApiRequest,
) -> Result<ApiResponse, TransportError> {
    core.net().submit(tagged(req)).await
}

pub async fn blob_request(core: &Core, req: ApiRequest) -> Result<ApiResponse, TransportError> {
    core.net().fetch_blob(tagged(req)).await
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_request_says_it_comes_from_the_desktop_client() {
        let req = tagged(ApiRequest {
            url: "https://api.thelemail.test/v1/me".to_owned(),
            method: "GET".to_owned(),
            headers: [("X-Client".to_owned(), "web".to_owned())].into(),
            body: None,
        });
        assert_eq!(
            req.headers.get("X-Client").map(String::as_str),
            Some("desktop")
        );
        assert_eq!(req.headers.len(), 1);
    }
}
