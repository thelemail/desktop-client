use std::path::{Path, PathBuf};

use rand::RngCore;
use rusqlite::Connection;

pub const DB_KEY_BYTES: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("the offline cache is not encrypted at rest")]
    PlaintextDatabase,
    #[error("not a valid account id")]
    InvalidAccountId,
    #[error("refusing to touch a path outside the application directory")]
    PathEscapesRoot,
    #[error("sqlcipher is not active")]
    CipherInactive,
    #[error("database is from a newer version of the app")]
    FromNewerVersion { found: i32, supported: i32 },
    #[error("database belongs to a different account")]
    WrongAccount,
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub fn generate_db_key() -> String {
    let mut bytes = [0u8; DB_KEY_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn is_valid_account_id(candidate: &str) -> bool {
    let bytes = candidate.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    bytes.iter().enumerate().all(|(i, b)| match i {
        8 | 13 | 18 | 23 => *b == b'-',
        _ => b.is_ascii_digit() || (b'a'..=b'f').contains(b),
    })
}

pub fn account_db_path(root: &Path, account_id: &str) -> Result<PathBuf, StoreError> {
    if !is_valid_account_id(account_id) {
        return Err(StoreError::InvalidAccountId);
    }
    let accounts = root.join("accounts");
    let path = accounts.join(account_id).join("offline.db");
    if !path.starts_with(&accounts) {
        return Err(StoreError::PathEscapesRoot);
    }
    Ok(path)
}

fn is_inside_account_store(path: &Path) -> bool {
    let mut components = path.components().rev();
    if components.next().and_then(|c| c.as_os_str().to_str()) != Some("offline.db") {
        return false;
    }
    let Some(account) = components.next().and_then(|c| c.as_os_str().to_str()) else {
        return false;
    };
    if !is_valid_account_id(account) {
        return false;
    }
    components.next().and_then(|c| c.as_os_str().to_str()) == Some("accounts")
}

fn looks_like_plaintext(path: &Path) -> Result<bool, StoreError> {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return Ok(false);
    };
    let mut head = [0u8; 16];
    match file.read_exact(&mut head) {
        Ok(()) => Ok(head.starts_with(b"SQLite format 3")),
        Err(_) => Ok(false),
    }
}

pub fn open_account_db(
    path: &Path,
    key_hex: &str,
    account_id: &str,
) -> Result<Connection, StoreError> {
    if !is_valid_account_id(account_id) {
        return Err(StoreError::InvalidAccountId);
    }
    if looks_like_plaintext(path)? {
        if !is_inside_account_store(path) {
            return Err(StoreError::PathEscapesRoot);
        }
        std::fs::remove_file(path)?;
        return Err(StoreError::PlaintextDatabase);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(path)?;
    conn.pragma_update(None, "key", format!("x'{key_hex}'"))?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;

    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })?;

    let cipher_version: Option<String> = conn
        .pragma_query_value(None, "cipher_version", |r| r.get(0))
        .ok();
    if cipher_version.is_none_or(|v| v.trim().is_empty()) {
        return Err(StoreError::CipherInactive);
    }

    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "recursive_triggers", "ON")?;
    conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;

    crate::migrations::migrate(&conn)?;
    assert_account(&conn, account_id)?;

    Ok(conn)
}

fn assert_account(conn: &Connection, account_id: &str) -> Result<(), StoreError> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'account_id'",
            [],
            |r| r.get(0),
        )
        .ok();

    match existing {
        Some(found) if found == account_id => Ok(()),
        Some(_) => Err(StoreError::WrongAccount),
        None => {
            conn.execute(
                "INSERT INTO schema_meta (key, value) VALUES ('account_id', ?1)",
                [account_id],
            )?;
            Ok(())
        }
    }
}
