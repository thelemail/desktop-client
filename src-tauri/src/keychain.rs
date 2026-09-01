use security_framework::access_control::{ProtectionMode, SecAccessControl};
use security_framework::passwords::{
    delete_generic_password, delete_generic_password_options, generic_password,
    get_generic_password, set_generic_password_options,
};
use security_framework::passwords_options::PasswordOptions;

const DB_KEY_SERVICE: &str = "com.thelemail.desktop.dbkey";
const REFRESH_SERVICE: &str = "com.thelemail.desktop.refresh";

#[derive(Debug)]
pub enum Read {
    Found(String),
    NotPresent,
    Failed(String),
}

const ERR_ITEM_NOT_FOUND: i32 = -25300;
const ERR_MISSING_ENTITLEMENT: i32 = -34018;

fn hardened(service: &str, account: &str) -> Result<PasswordOptions, String> {
    let mut options = PasswordOptions::new_generic_password(service, account);
    let control = SecAccessControl::create_with_protection(
        Some(ProtectionMode::AccessibleAfterFirstUnlockThisDeviceOnly),
        0,
    )
    .map_err(|e| e.to_string())?;
    options.set_access_control(control);
    Ok(options)
}

static HARDENING: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn hardening_available() -> bool {
    *HARDENING.get_or_init(|| {
        const PROBE_ACCOUNT: &str = "00000000-0000-0000-0000-000000000000";
        let service = "com.thelemail.desktop.capability-probe";
        let Ok(options) = hardened(service, PROBE_ACCOUNT) else {
            return false;
        };
        match set_generic_password_options(b"probe", options) {
            Ok(()) => {
                if let Ok(cleanup) = hardened(service, PROBE_ACCOUNT) {
                    let _ = delete_generic_password_options(cleanup);
                }
                true
            }
            Err(_) => false,
        }
    })
}

fn options_for(service: &str, account: &str) -> Result<PasswordOptions, String> {
    if hardening_available() {
        hardened(service, account)
    } else {
        Ok(PasswordOptions::new_generic_password(service, account))
    }
}

fn decode(bytes: Vec<u8>) -> Read {
    match String::from_utf8(bytes) {
        Ok(value) => Read::Found(value),
        Err(_) => Read::Failed("stored value is not valid utf-8".to_owned()),
    }
}

fn read(service: &str, account: &str) -> Read {
    if crate::ids::account_id(account).is_err() {
        return Read::Failed("not a valid account id".to_owned());
    }

    if hardening_available() {
        let options = match hardened(service, account) {
            Ok(options) => options,
            Err(err) => return Read::Failed(err),
        };
        match generic_password(options) {
            Ok(bytes) => return decode(bytes),
            Err(err)
                if err.code() == ERR_ITEM_NOT_FOUND || err.code() == ERR_MISSING_ENTITLEMENT => {}
            Err(err) => return Read::Failed(err.to_string()),
        }
    }

    match get_generic_password(service, account) {
        Ok(bytes) => match decode(bytes) {
            Read::Found(value) => {
                if hardening_available()
                    && let Err(err) = harden_existing(service, account, &value)
                {
                    return Read::Failed(err);
                }
                Read::Found(value)
            }
            other => other,
        },
        Err(err) if err.code() == ERR_ITEM_NOT_FOUND => Read::NotPresent,
        Err(err) => Read::Failed(err.to_string()),
    }
}

fn harden_existing(service: &str, account: &str, value: &str) -> Result<(), String> {
    match delete_generic_password(service, account) {
        Ok(()) => {}
        Err(err) if err.code() == ERR_ITEM_NOT_FOUND => {}
        Err(err) => return Err(err.to_string()),
    }
    write(service, account, value)
}

fn write(service: &str, account: &str, value: &str) -> Result<(), String> {
    crate::ids::account_id(account)?;
    let options = options_for(service, account)?;
    set_generic_password_options(value.as_bytes(), options).map_err(|e| e.to_string())
}

fn remove(service: &str, account: &str) -> Result<(), String> {
    crate::ids::account_id(account)?;
    let options = options_for(service, account)?;
    match delete_generic_password_options(options) {
        Ok(()) => {}
        Err(err) if err.code() == ERR_ITEM_NOT_FOUND => {}
        Err(err) => return Err(err.to_string()),
    }
    match delete_generic_password(service, account) {
        Ok(()) => Ok(()),
        Err(err) if err.code() == ERR_ITEM_NOT_FOUND => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

pub fn db_key(account_id: &str) -> Read {
    read(DB_KEY_SERVICE, account_id)
}

pub fn put_db_key(account_id: &str, key_hex: &str) -> Result<(), String> {
    write(DB_KEY_SERVICE, account_id, key_hex)
}

pub fn forget_db_key(account_id: &str) -> Result<(), String> {
    remove(DB_KEY_SERVICE, account_id)
}

pub fn refresh_cookie(account_id: &str) -> Read {
    read(REFRESH_SERVICE, account_id)
}

pub fn put_refresh_cookie(account_id: &str, value: &str) -> Result<(), String> {
    write(REFRESH_SERVICE, account_id, value)
}

pub fn forget_refresh_cookie(account_id: &str) -> Result<(), String> {
    remove(REFRESH_SERVICE, account_id)
}

pub fn ensure_db_key(account_id: &str) -> Result<String, String> {
    match db_key(account_id) {
        Read::Found(key) => Ok(key),
        Read::NotPresent => {
            let key = thelemail_store::generate_db_key();
            put_db_key(account_id, &key)?;
            Ok(key)
        }
        Read::Failed(err) => Err(format!(
            "keychain read failed ({err}); refusing to mint a replacement key because that would orphan the existing mailbox cache"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use security_framework::passwords::set_generic_password;

    const TEST_SERVICE: &str = "com.thelemail.desktop.test.migration";
    const TEST_ACCOUNT: &str = "11111111-2222-3333-4444-555555555555";

    fn cleanup() {
        let _ = remove(TEST_SERVICE, TEST_ACCOUNT);
    }

    #[test]
    fn a_hostile_account_never_reaches_the_keychain() {
        for hostile in ["../../etc/passwd", "", "NOT-A-UUID"] {
            assert!(matches!(read(TEST_SERVICE, hostile), Read::Failed(_)));
            assert!(write(TEST_SERVICE, hostile, "x").is_err());
            assert!(remove(TEST_SERVICE, hostile).is_err());
        }
    }

    #[test]
    #[ignore = "touches the real macOS keychain"]
    fn a_legacy_item_is_migrated_instead_of_being_lost() {
        cleanup();
        set_generic_password(TEST_SERVICE, TEST_ACCOUNT, b"legacy-secret").expect("seed legacy");

        match read(TEST_SERVICE, TEST_ACCOUNT) {
            Read::Found(value) => assert_eq!(value, "legacy-secret"),
            other => panic!(
                "a legacy item must be found, not {other:?}: losing it would orphan the mirror"
            ),
        }

        match read(TEST_SERVICE, TEST_ACCOUNT) {
            Read::Found(value) => assert_eq!(value, "legacy-secret"),
            other => panic!("after migration the hardened read must find it, got {other:?}"),
        }

        cleanup();
    }

    #[test]
    #[ignore = "touches the real macOS keychain"]
    fn a_hardened_item_round_trips_and_can_be_removed() {
        cleanup();
        assert!(matches!(read(TEST_SERVICE, TEST_ACCOUNT), Read::NotPresent));

        write(TEST_SERVICE, TEST_ACCOUNT, "hardened-secret").expect("write");
        match read(TEST_SERVICE, TEST_ACCOUNT) {
            Read::Found(value) => assert_eq!(value, "hardened-secret"),
            other => panic!("expected the hardened item, got {other:?}"),
        }

        remove(TEST_SERVICE, TEST_ACCOUNT).expect("remove");
        assert!(
            matches!(read(TEST_SERVICE, TEST_ACCOUNT), Read::NotPresent),
            "remove must clear both the hardened and legacy forms"
        );
    }
}
