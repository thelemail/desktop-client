const DB_KEY_SERVICE: &str = "com.thelemail.desktop.dbkey";
const REFRESH_SERVICE: &str = "com.thelemail.desktop.refresh";
const LOCAL_HALF_SERVICE: &str = "com.thelemail.desktop.localhalf";

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

#[derive(Debug)]
pub enum Read {
    Found(String),
    NotPresent,
    Failed(String),
}

pub fn hardening_available() -> bool {
    platform::hardening_available()
}

fn read(service: &str, account: &str) -> Read {
    if crate::ids::account_id(account).is_err() {
        return Read::Failed("not a valid account id".to_owned());
    }
    platform::read(service, account)
}

fn write(service: &str, account: &str, value: &str) -> Result<(), String> {
    crate::ids::account_id(account)?;
    platform::write(service, account, value)
}

fn remove(service: &str, account: &str) -> Result<(), String> {
    crate::ids::account_id(account)?;
    platform::remove(service, account)
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

pub fn local_half(account_id: &str) -> Read {
    read(LOCAL_HALF_SERVICE, account_id)
}

pub fn put_local_half(account_id: &str, value: &str) -> Result<(), String> {
    write(LOCAL_HALF_SERVICE, account_id, value)
}

pub fn forget_local_half(account_id: &str) -> Result<(), String> {
    remove(LOCAL_HALF_SERVICE, account_id)
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

    const TEST_SERVICE: &str = "com.thelemail.desktop.test.migration";

    #[test]
    fn a_hostile_account_never_reaches_the_keychain() {
        for hostile in ["../../etc/passwd", "", "NOT-A-UUID"] {
            assert!(matches!(read(TEST_SERVICE, hostile), Read::Failed(_)));
            assert!(write(TEST_SERVICE, hostile, "x").is_err());
            assert!(remove(TEST_SERVICE, hostile).is_err());
        }
    }
}
