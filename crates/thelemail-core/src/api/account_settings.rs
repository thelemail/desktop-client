use serde::Deserialize;
use serde_json::{Map, Value};

use super::{Api, ApiError, Call};

pub type Section = Map<String, Value>;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AccountSettingsResponse {
    #[serde(default)]
    pub sections: Map<String, Value>,
}

pub async fn get(api: Api<'_>, account_id: &str) -> Result<AccountSettingsResponse, ApiError> {
    api.json(account_id, Call::get("/v1/account/settings"))
        .await
}

pub async fn put(
    api: Api<'_>,
    account_id: &str,
    section: &str,
    value: &Section,
) -> Result<(), ApiError> {
    let path = format!("/v1/account/settings/{}", encode(section));
    api.empty(account_id, Call::put(path).json(value)).await
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}
