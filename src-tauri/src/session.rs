use sha2::{Digest, Sha256};
use tauri::State;
use thelemail_api::Net;

use crate::keychain;

fn refresh_cookie_name(account_id: &str) -> String {
    let digest = Sha256::digest(account_id.as_bytes());
    format!("refresh_token_{}", hex::encode(&digest[..4]))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionArgs {
    pub account_id: String,
}

#[tauri::command]
pub fn session_persist(net: State<'_, Net>, args: SessionArgs) -> Result<bool, String> {
    crate::ids::account_id(&args.account_id)?;
    let name = refresh_cookie_name(&args.account_id);
    let Some((_, value)) = net
        .export_cookies()
        .into_iter()
        .find(|(cookie_name, _)| *cookie_name == name)
    else {
        return Ok(false);
    };
    keychain::put_refresh_cookie(&args.account_id, &value)?;
    Ok(true)
}

#[tauri::command]
pub fn session_restore(net: State<'_, Net>, args: SessionArgs) -> Result<bool, String> {
    crate::ids::account_id(&args.account_id)?;
    match keychain::refresh_cookie(&args.account_id) {
        keychain::Read::Found(value) => {
            net.import_cookie(&refresh_cookie_name(&args.account_id), &value)
                .map_err(|e| e.to_string())?;
            Ok(true)
        }
        keychain::Read::NotPresent => Ok(false),
        keychain::Read::Failed(err) => Err(err),
    }
}

#[tauri::command]
pub fn session_forget(
    net: State<'_, Net>,
    mirror: State<'_, crate::mirror::Mirror>,
    args: SessionArgs,
) -> Result<(), String> {
    crate::ids::account_id(&args.account_id)?;
    net.forget_cookie(&refresh_cookie_name(&args.account_id));
    crate::keystore::forget_persisted(&args.account_id);
    let purged = mirror.purge(&args.account_id);
    let cookie = keychain::forget_refresh_cookie(&args.account_id);
    let db_key = keychain::forget_db_key(&args.account_id);
    purged.and(cookie).and(db_key)
}

#[cfg(test)]
mod tests {
    use super::refresh_cookie_name;

    #[test]
    fn cookie_name_matches_the_backend_derivation() {
        assert_eq!(
            refresh_cookie_name("11111111-2222-3333-4444-555555555555"),
            expected("11111111-2222-3333-4444-555555555555")
        );
        assert_ne!(
            refresh_cookie_name("11111111-2222-3333-4444-555555555555"),
            refresh_cookie_name("99999999-2222-3333-4444-555555555555"),
            "each account must get its own cookie"
        );
    }

    fn expected(account_id: &str) -> String {
        use sha2::{Digest, Sha256};
        let sum = Sha256::digest(account_id.as_bytes());
        format!("refresh_token_{}", hex::encode(&sum[..4]))
    }

    #[test]
    fn the_suffix_is_eight_hex_characters() {
        let name = refresh_cookie_name("11111111-2222-3333-4444-555555555555");
        let suffix = name.strip_prefix("refresh_token_").expect("prefix");
        assert_eq!(suffix.len(), 8);
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
