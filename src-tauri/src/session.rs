use std::sync::{Mutex, MutexGuard};

use sha2::{Digest, Sha256};
use tauri::State;
use thelemail_api::{ApiRequest, Net};

use crate::keychain;

static SAVED_SESSION: Mutex<()> = Mutex::new(());

fn saved_session_lock() -> MutexGuard<'static, ()> {
    SAVED_SESSION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn refresh_cookie_suffix(account_id: &str) -> String {
    let digest = Sha256::digest(account_id.as_bytes());
    hex::encode(&digest[..4])
}

fn refresh_cookie_name(account_id: &str) -> String {
    format!("rt_{}", refresh_cookie_suffix(account_id))
}

fn legacy_refresh_cookie_name(account_id: &str) -> String {
    format!("refresh_token_{}", refresh_cookie_suffix(account_id))
}

fn find_refresh_cookie(cookies: &[(String, String)], account_id: &str) -> Option<String> {
    let name = refresh_cookie_name(account_id);
    let legacy = legacy_refresh_cookie_name(account_id);
    cookies
        .iter()
        .find(|(cookie_name, _)| *cookie_name == name)
        .or_else(|| {
            cookies
                .iter()
                .find(|(cookie_name, _)| *cookie_name == legacy)
        })
        .map(|(_, value)| value.to_owned())
}

pub fn auth_account(req: &ApiRequest) -> Option<String> {
    let url = url::Url::parse(&req.url).ok()?;
    if !url.path().contains("/v1/auth/") {
        return None;
    }
    let (_, id) = req
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("x-account-id"))?;
    crate::ids::account_id(id).ok().map(str::to_owned)
}

fn changed_cookie<'a>(saved: &str, current: Option<&'a str>) -> Option<&'a str> {
    current.filter(|value| !value.is_empty() && *value != saved)
}

pub fn sync_persisted(net: &Net, account_id: &str) {
    let _guard = saved_session_lock();
    let keychain::Read::Found(saved) = keychain::refresh_cookie(account_id) else {
        return;
    };
    let current = find_refresh_cookie(&net.export_cookies(), account_id);
    let Some(value) = changed_cookie(&saved, current.as_deref()) else {
        return;
    };
    if let Err(err) = keychain::put_refresh_cookie(account_id, value) {
        eprintln!("session: could not update the saved sign-in: {err}");
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionArgs {
    pub account_id: String,
}

#[tauri::command]
pub fn session_persist(net: State<'_, Net>, args: SessionArgs) -> Result<bool, String> {
    crate::ids::account_id(&args.account_id)?;
    let _guard = saved_session_lock();
    let Some(value) = find_refresh_cookie(&net.export_cookies(), &args.account_id) else {
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
    halves: State<'_, crate::keystore::PersistentHalves>,
    args: SessionArgs,
) -> Result<(), String> {
    crate::ids::account_id(&args.account_id)?;
    let _guard = saved_session_lock();
    net.forget_cookie(&refresh_cookie_name(&args.account_id));
    net.forget_cookie(&legacy_refresh_cookie_name(&args.account_id));
    crate::keystore::forget_persisted(&halves, &args.account_id);
    let purged = mirror.purge(&args.account_id);
    let cookie = keychain::forget_refresh_cookie(&args.account_id);
    let db_key = keychain::forget_db_key(&args.account_id);
    purged.and(cookie).and(db_key)
}

#[cfg(test)]
mod tests {
    use super::{legacy_refresh_cookie_name, refresh_cookie_name};

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
        format!("rt_{}", hex::encode(&sum[..4]))
    }

    #[test]
    fn the_suffix_is_eight_hex_characters() {
        let name = refresh_cookie_name("11111111-2222-3333-4444-555555555555");
        let suffix = name.strip_prefix("rt_").expect("prefix");
        assert_eq!(suffix.len(), 8);
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    fn jar(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn a_session_left_under_the_old_name_is_still_persisted() {
        let id = "11111111-2222-3333-4444-555555555555";
        let cookies = jar(&[(&legacy_refresh_cookie_name(id), "carried-over")]);
        assert_eq!(
            super::find_refresh_cookie(&cookies, id),
            Some("carried-over".to_owned())
        );
    }

    #[test]
    fn the_host_only_cookie_wins_over_a_stale_legacy_one() {
        let id = "11111111-2222-3333-4444-555555555555";
        let cookies = jar(&[
            (&legacy_refresh_cookie_name(id), "stale"),
            (&refresh_cookie_name(id), "current"),
        ]);
        assert_eq!(
            super::find_refresh_cookie(&cookies, id),
            Some("current".to_owned())
        );
    }

    #[test]
    fn another_accounts_cookie_is_never_persisted() {
        let id = "11111111-2222-3333-4444-555555555555";
        let other = "99999999-2222-3333-4444-555555555555";
        let cookies = jar(&[(&refresh_cookie_name(other), "theirs")]);
        assert_eq!(super::find_refresh_cookie(&cookies, id), None);
    }

    fn request(url: &str, account: Option<&str>) -> thelemail_api::ApiRequest {
        let mut headers = std::collections::HashMap::new();
        if let Some(id) = account {
            headers.insert("X-Account-Id".to_owned(), id.to_owned());
        }
        thelemail_api::ApiRequest {
            url: url.to_owned(),
            method: "POST".to_owned(),
            headers,
            body: None,
        }
    }

    #[test]
    fn only_auth_calls_for_a_named_account_are_synced() {
        let id = "11111111-2222-3333-4444-555555555555";
        assert_eq!(
            super::auth_account(&request(
                "https://api.thelemail.com/v1/auth/refresh",
                Some(id)
            )),
            Some(id.to_owned())
        );
        assert_eq!(
            super::auth_account(&request("https://api.thelemail.com/v1/me", Some(id))),
            None
        );
        assert_eq!(
            super::auth_account(&request("https://api.thelemail.com/v1/auth/refresh", None)),
            None
        );
        assert_eq!(
            super::auth_account(&request(
                "https://api.thelemail.com/v1/auth/refresh",
                Some("../etc/passwd")
            )),
            None
        );
    }

    #[test]
    fn the_saved_cookie_is_replaced_only_when_the_jar_moved_on() {
        assert_eq!(super::changed_cookie("old", Some("new")), Some("new"));
        assert_eq!(super::changed_cookie("same", Some("same")), None);
        assert_eq!(super::changed_cookie("old", None), None);
        assert_eq!(super::changed_cookie("old", Some("")), None);
    }

    #[test]
    fn both_names_share_one_suffix() {
        let id = "11111111-2222-3333-4444-555555555555";
        let suffix = refresh_cookie_name(id)
            .strip_prefix("rt_")
            .expect("prefix")
            .to_owned();
        assert_eq!(
            legacy_refresh_cookie_name(id),
            format!("refresh_token_{suffix}")
        );
    }

    #[test]
    fn a_panic_while_saving_does_not_block_later_saves() {
        let _ = std::thread::spawn(|| {
            let _guard = super::saved_session_lock();
            panic!("keychain write failed");
        })
        .join();
        drop(super::saved_session_lock());
    }
}
