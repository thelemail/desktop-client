use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures_util::{StreamExt, stream};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use thelemail_api::{ApiRequest, Net};
use thelemail_keystore::Keystore;
use thelemail_store::search::{SearchHit, search_messages};
use thelemail_store::{account_db_path, open_account_db};
use tokio::sync::Notify;

use crate::keychain;

const SCOPES: &[&str] = &["inbox", "archive", "spam", "trash"];
const PAGE_LIMIT: u32 = 100;
const BODY_CONCURRENCY: usize = 4;

const DELTA_SCOPE: &str = "__delta__";
const DELTA_LIMIT: u32 = 200;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageChangesResponse {
    changes: Vec<MessageChange>,
    next_cursor: String,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    resync_required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageChange {
    id: String,
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    message: Option<MessageListItem>,
}

#[derive(Debug, Deserialize)]
pub struct MessageListResponse {
    pub items: Vec<MessageListItem>,
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageListItem {
    id: String,
    direction: String,
    #[serde(default)]
    source: String,
    #[serde(rename = "mailboxState")]
    mailbox_state: String,
    #[serde(default)]
    starred: bool,
    #[serde(default)]
    read: bool,
    #[serde(rename = "storedAt")]
    stored_at: String,
    #[serde(default, rename = "bodySizeBytes")]
    body_size_bytes: i64,
    #[serde(default, rename = "attachmentCount")]
    attachment_count: i64,
    #[serde(default, rename = "threadRootId")]
    thread_root_id: Option<String>,
    #[serde(default, rename = "encryptedPreview")]
    encrypted_preview: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
struct MessagePreview {
    #[serde(default)]
    subject: String,
    #[serde(default)]
    sender: PreviewParty,
    #[serde(default)]
    snippet: String,
    #[serde(default)]
    display_date: String,
    #[serde(default)]
    recipients: Vec<PreviewParty>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct PreviewParty {
    #[serde(default)]
    display: String,
    #[serde(default)]
    address: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub account_id: String,
    pub scope: String,
    pub phase: String,
    pub previews_done: u64,
    pub bodies_done: u64,
    pub bodies_total: u64,
    pub complete: bool,
}

pub struct Mirror {
    connections: Mutex<HashMap<String, Connection>>,
    running: Mutex<HashMap<String, bool>>,
    tokens: Mutex<HashMap<String, String>>,
    token_wakers: Mutex<HashMap<String, Arc<Notify>>>,
    pokes: Mutex<HashSet<String>>,
    watching: Mutex<HashMap<String, bool>>,
    notify_armed: Mutex<HashMap<String, bool>>,
    started_at: i64,
}

impl Default for Mirror {
    fn default() -> Self {
        Self {
            connections: Mutex::default(),
            running: Mutex::default(),
            tokens: Mutex::default(),
            token_wakers: Mutex::default(),
            pokes: Mutex::default(),
            watching: Mutex::default(),
            notify_armed: Mutex::default(),
            started_at: now_unix(),
        }
    }
}

impl Mirror {
    pub fn root() -> PathBuf {
        dirs_root().join("com.thelemail.desktop")
    }

    pub fn open(&self, account_id: &str) -> Result<(), String> {
        crate::ids::account_id(account_id)?;
        let mut conns = self.connections.lock().expect("mirror connections");
        if conns.contains_key(account_id) {
            return Ok(());
        }
        let key = keychain::ensure_db_key(account_id)?;
        let path = account_db_path(&Self::root(), account_id).map_err(|e| e.to_string())?;
        let conn = open_account_db(&path, &key, account_id).map_err(|e| e.to_string())?;
        conns.insert(account_id.to_owned(), conn);
        Ok(())
    }

    pub fn purge(&self, account_id: &str) -> Result<(), String> {
        crate::ids::account_id(account_id)?;
        self.close(account_id);
        self.stop_watch(account_id);
        self.tokens
            .lock()
            .expect("mirror tokens")
            .remove(account_id);
        self.running
            .lock()
            .expect("mirror running")
            .remove(account_id);
        self.notify_armed
            .lock()
            .expect("mirror notify")
            .remove(account_id);
        self.token_wakers
            .lock()
            .expect("mirror wakers")
            .remove(account_id);

        let dir = Self::root().join("accounts").join(account_id);
        if !dir.starts_with(Self::root().join("accounts")) {
            return Err("refusing to remove a path outside the account store".to_owned());
        }
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }

    pub fn set_date_floor(&self, account_id: &str, floor: Option<&str>) -> Result<(), String> {
        self.with_conn(account_id, |conn| {
            conn.execute(
                "INSERT INTO sync_state (scope, date_floor) VALUES ('__scope__', ?1) \
                 ON CONFLICT(scope) DO UPDATE SET date_floor = excluded.date_floor",
                rusqlite::params![floor],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    }

    pub fn date_floor(&self, account_id: &str) -> Result<Option<String>, String> {
        self.with_conn(account_id, |conn| {
            Ok(conn
                .query_row(
                    "SELECT date_floor FROM sync_state WHERE scope = '__scope__'",
                    [],
                    |r| r.get::<_, Option<String>>(0),
                )
                .unwrap_or(None))
        })
    }

    pub fn close(&self, account_id: &str) {
        self.connections
            .lock()
            .expect("mirror connections")
            .remove(account_id);
    }

    pub fn search(
        &self,
        account_id: &str,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<SearchHit>, String> {
        let conns = self.connections.lock().expect("mirror connections");
        let conn = conns.get(account_id).ok_or("mirror is not open")?;
        search_messages(conn, query, limit).map_err(|e| e.to_string())
    }

    pub fn set_token(&self, account_id: &str, token: &str) {
        let changed = {
            let mut tokens = self.tokens.lock().expect("mirror tokens");
            if tokens.get(account_id).map(String::as_str) == Some(token) {
                false
            } else {
                tokens.insert(account_id.to_owned(), token.to_owned());
                true
            }
        };
        if changed && self.is_watching(account_id) {
            self.token_waker(account_id).notify_one();
        }
    }

    fn begin_poke(&self, account_id: &str) -> bool {
        self.pokes
            .lock()
            .expect("mirror pokes")
            .insert(account_id.to_owned())
    }

    fn finish_poke(&self, account_id: &str) {
        self.pokes
            .lock()
            .expect("mirror pokes")
            .remove(account_id);
        if self.is_watching(account_id) {
            self.token_waker(account_id).notify_one();
        }
    }

    pub fn token_waker(&self, account_id: &str) -> Arc<Notify> {
        self.token_wakers
            .lock()
            .expect("mirror wakers")
            .entry(account_id.to_owned())
            .or_default()
            .clone()
    }

    pub fn token(&self, account_id: &str) -> Option<String> {
        self.tokens
            .lock()
            .expect("mirror tokens")
            .get(account_id)
            .cloned()
    }

    pub fn arm_notifications(&self, account_id: &str) {
        self.notify_armed
            .lock()
            .expect("mirror notify")
            .insert(account_id.to_owned(), true);
    }

    fn notifications_armed(&self, account_id: &str) -> bool {
        self.notify_armed
            .lock()
            .expect("mirror notify")
            .get(account_id)
            .copied()
            .unwrap_or(false)
    }

    fn claim_watch(&self, account_id: &str) -> bool {
        let mut watching = self.watching.lock().expect("mirror watching");
        if watching.get(account_id).copied().unwrap_or(false) {
            return false;
        }
        watching.insert(account_id.to_owned(), true);
        true
    }

    pub fn stop_watch(&self, account_id: &str) {
        self.watching
            .lock()
            .expect("mirror watching")
            .insert(account_id.to_owned(), false);
    }

    fn is_watching(&self, account_id: &str) -> bool {
        self.watching
            .lock()
            .expect("mirror watching")
            .get(account_id)
            .copied()
            .unwrap_or(false)
    }

    pub fn adopt_connection(&self, account_id: &str, conn: Connection) -> Result<(), String> {
        crate::ids::account_id(account_id)?;
        self.connections
            .lock()
            .expect("mirror connections")
            .insert(account_id.to_owned(), conn);
        Ok(())
    }

    pub fn take_connection(&self, account_id: &str) -> Option<Connection> {
        self.connections
            .lock()
            .expect("mirror connections")
            .remove(account_id)
    }

    pub fn with_conn<T>(
        &self,
        account_id: &str,
        f: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let conns = self.connections.lock().expect("mirror connections");
        let conn = conns.get(account_id).ok_or("mirror is not open")?;
        f(conn)
    }

    fn claim(&self, account_id: &str) -> bool {
        let mut running = self.running.lock().expect("mirror running");
        if running.get(account_id).copied().unwrap_or(false) {
            return false;
        }
        running.insert(account_id.to_owned(), true);
        true
    }

    fn release(&self, account_id: &str) {
        self.running
            .lock()
            .expect("mirror running")
            .insert(account_id.to_owned(), false);
    }
}

fn dirs_root() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library")
        .join("Application Support")
}

fn upsert_message(
    conn: &Connection,
    item: &MessageListItem,
    preview: &MessagePreview,
    decrypted: bool,
) -> rusqlite::Result<()> {
    let recipients = serde_json::to_string(
        &preview
            .recipients
            .iter()
            .map(|r| r.address.clone())
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_owned());
    let now = now_iso();

    conn.execute(
        "INSERT INTO messages (id, direction, source, mailbox_state, starred, read, stored_at, \
          body_size_bytes, attachment_count, thread_root_id, subject, sender_display, \
          sender_address, recipients_json, snippet, display_date, preview_state, synced_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18) \
         ON CONFLICT(id) DO UPDATE SET \
          mailbox_state=excluded.mailbox_state, starred=excluded.starred, read=excluded.read, \
          attachment_count=excluded.attachment_count, thread_root_id=excluded.thread_root_id, \
          subject=excluded.subject, sender_display=excluded.sender_display, \
          sender_address=excluded.sender_address, recipients_json=excluded.recipients_json, \
          snippet=excluded.snippet, display_date=excluded.display_date, \
          preview_state=excluded.preview_state, deleted=0, synced_at=excluded.synced_at \
         WHERE messages.dirty = 0",
        params![
            item.id,
            item.direction,
            item.source,
            item.mailbox_state,
            item.starred as i64,
            item.read as i64,
            item.stored_at,
            item.body_size_bytes,
            item.attachment_count,
            item.thread_root_id,
            preview.subject,
            preview.sender.display,
            preview.sender.address,
            recipients,
            preview.snippet,
            preview.display_date,
            if decrypted { "ok" } else { "undecryptable" },
            now,
        ],
    )?;

    let rowid: i64 = conn.query_row(
        "SELECT rowid FROM messages WHERE id = ?1",
        [&item.id],
        |r| r.get(0),
    )?;

    conn.execute(
        "INSERT INTO search_docs (rowid, subject, sender, recipients, snippet, body) \
         VALUES (?1,?2,?3,?4,?5,COALESCE((SELECT body FROM search_docs WHERE rowid = ?1), '')) \
         ON CONFLICT(rowid) DO UPDATE SET subject=excluded.subject, sender=excluded.sender, \
          recipients=excluded.recipients, snippet=excluded.snippet",
        params![
            rowid,
            preview.subject,
            format!("{} {}", preview.sender.display, preview.sender.address),
            recipients,
            preview.snippet,
        ],
    )?;

    Ok(())
}

pub fn now_iso() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

pub async fn backfill(app: AppHandle, account_id: String, access_token: String) {
    let mirror = app.state::<Mirror>();
    if !mirror.claim(&account_id) {
        return;
    }
    if has_delta_token(&mirror, &account_id) {
        mirror.arm_notifications(&account_id);
    }

    for scope in SCOPES {
        let net = app.state::<Net>();
        let ks = app.state::<Keystore>();
        let mirror = app.state::<Mirror>();
        let result = sync_scope(
            &net,
            &ks,
            &mirror,
            &account_id,
            scope,
            &access_token,
            |done| {
                let _ = app.emit(
                    "mirror://progress",
                    SyncProgress {
                        account_id: account_id.clone(),
                        scope: scope.to_string(),
                        phase: "previews".to_owned(),
                        previews_done: done,
                        bodies_done: 0,
                        bodies_total: 0,
                        complete: false,
                    },
                );
            },
        )
        .await;
        match result {
            Ok((_, fresh)) => announce(&app, &account_id, &fresh),
            Err(err) => {
                let _ = app.emit(
                    "mirror://error",
                    serde_json::json!({ "accountId": account_id, "scope": scope, "error": err }),
                );
            }
        }
    }

    app.state::<Mirror>().arm_notifications(&account_id);

    {
        let net = app.state::<Net>();
        let ks = app.state::<Keystore>();
        let mirror = app.state::<Mirror>();
        let result = prefetch_bodies(
            &net,
            &ks,
            &mirror,
            &account_id,
            &access_token,
            |done, total| {
                let _ = app.emit(
                    "mirror://progress",
                    SyncProgress {
                        account_id: account_id.clone(),
                        scope: "all".to_owned(),
                        phase: "bodies".to_owned(),
                        previews_done: 0,
                        bodies_done: done,
                        bodies_total: total,
                        complete: false,
                    },
                );
            },
        )
        .await;
        if let Err(err) = result {
            let _ = app.emit(
                "mirror://error",
                serde_json::json!({ "accountId": account_id, "error": err }),
            );
        }
    }

    let _ = app.emit(
        "mirror://progress",
        SyncProgress {
            account_id: account_id.clone(),
            scope: "all".to_owned(),
            phase: "idle".to_owned(),
            previews_done: 0,
            bodies_done: 0,
            bodies_total: 0,
            complete: true,
        },
    );
    app.state::<Mirror>().release(&account_id);
}

pub fn apply_page(
    conn: &Connection,
    ks: &Keystore,
    account_id: &str,
    scope: &str,
    items: &[MessageListItem],
    next_cursor: &Option<String>,
    since: i64,
) -> Result<Vec<crate::notify::NewMail>, String> {
    let mut fresh = Vec::new();
    for item in items {
        let (preview, decrypted) = match &item.encrypted_preview {
            Some(b64) => decrypt_preview(ks, account_id, b64),
            None => (MessagePreview::default(), false),
        };
        let known = message_known(conn, &item.id);
        upsert_message(conn, item, &preview, decrypted).map_err(|e| e.to_string())?;
        if !known
            && let Some(mail) = arrival_for(account_id, item, &preview, decrypted, since)
        {
            fresh.push(mail);
        }
    }
    conn.execute(
        "INSERT INTO sync_state (scope, cursor) VALUES (?1, ?2) \
         ON CONFLICT(scope) DO UPDATE SET cursor = excluded.cursor",
        params![scope, next_cursor],
    )
    .map_err(|e| e.to_string())?;
    Ok(fresh)
}

fn message_known(conn: &Connection, id: &str) -> bool {
    conn.query_row(
        "SELECT count(*) FROM messages WHERE id = ?1",
        [id],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

fn arrival_for(
    account_id: &str,
    item: &MessageListItem,
    preview: &MessagePreview,
    decrypted: bool,
    since: i64,
) -> Option<crate::notify::NewMail> {
    if !decrypted || item.read || item.direction != "received" || item.mailbox_state != "inbox" {
        return None;
    }
    if chrono_parse(&item.stored_at).unwrap_or(i64::MIN) < since {
        return None;
    }
    Some(crate::notify::NewMail {
        account_id: account_id.to_owned(),
        message_id: item.id.clone(),
        sender: if preview.sender.display.is_empty() {
            preview.sender.address.clone()
        } else {
            preview.sender.display.clone()
        },
        subject: preview.subject.clone(),
        snippet: preview.snippet.clone(),
    })
}

fn reset_for_resync(conn: &Connection, watermark_cursor: &str) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for table in ["search_docs", "bodies", "messages", "sync_state"] {
        tx.execute(&format!("DELETE FROM {table}"), [])
            .map_err(|e| e.to_string())?;
    }
    tx.execute(
        "INSERT INTO sync_state (scope, delta_token) VALUES (?1, ?2)",
        params![DELTA_SCOPE, watermark_cursor],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

fn has_delta_token(mirror: &Mirror, account_id: &str) -> bool {
    mirror
        .with_conn(account_id, |conn| {
            Ok(conn
                .query_row(
                    "SELECT delta_token FROM sync_state WHERE scope = ?1",
                    [DELTA_SCOPE],
                    |r| r.get::<_, Option<String>>(0),
                )
                .unwrap_or(None))
        })
        .ok()
        .flatten()
        .is_some()
}

fn announce(app: &AppHandle, account_id: &str, fresh: &[crate::notify::NewMail]) {
    if fresh.is_empty() {
        return;
    }
    let armed = app.state::<Mirror>().notifications_armed(account_id);
    eprintln!(
        "mirror: {} fresh arrival(s) for {account_id}, notifications armed={armed}",
        fresh.len()
    );
    if armed {
        for mail in fresh {
            crate::notify::new_mail(app, mail);
        }
    }
    let _ = app.emit(
        "mirror://changed",
        serde_json::json!({ "accountId": account_id, "arrived": fresh.len() }),
    );
}

pub async fn sync_scope(
    net: &Net,
    ks: &Keystore,
    mirror: &Mirror,
    account_id: &str,
    scope: &str,
    access_token: &str,
    mut on_progress: impl FnMut(u64),
) -> Result<(u64, Vec<crate::notify::NewMail>), String> {
    let since = mirror.started_at;
    let mut fresh = Vec::new();
    let mut cursor: Option<String> = mirror.with_conn(account_id, |conn| {
        Ok(conn
            .query_row(
                "SELECT cursor FROM sync_state WHERE scope = ?1",
                [scope],
                |r| r.get::<_, Option<String>>(0),
            )
            .unwrap_or(None))
    })?;

    let mut previews_done: u64 = 0;

    loop {
        let mut url = format!(
            "{}v1/messages?mailbox={scope}&sort=oldest&limit={PAGE_LIMIT}",
            net.config().api_base
        );
        if let Some(c) = &cursor {
            url.push_str(&format!("&cursor={}", urlencode(c)));
        }

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_owned(), format!("Bearer {access_token}"));
        headers.insert("X-Account-Id".to_owned(), account_id.to_owned());
        headers.insert("X-Client".to_owned(), "desktop".to_owned());

        let resp = net
            .request(ApiRequest {
                url,
                method: "GET".to_owned(),
                headers,
                body: None,
            })
            .await
            .map_err(|e| e.to_string())?;

        if resp.status != 200 {
            return Err(format!("list {scope} returned {}", resp.status));
        }
        let body = resp.body.unwrap_or_default();
        let page: MessageListResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;

        if page.items.is_empty() {
            break;
        }

        let arrived = mirror.with_conn(account_id, |conn| {
            apply_page(conn, ks, account_id, scope, &page.items, &page.next_cursor, since)
        })?;
        fresh.extend(arrived);
        previews_done += page.items.len() as u64;
        on_progress(previews_done);

        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }

    Ok((previews_done, fresh))
}

fn decrypt_preview(ks: &Keystore, account_id: &str, encrypted_b64: &str) -> (MessagePreview, bool) {
    use base64::Engine as _;
    let Ok(ciphertext) = base64::engine::general_purpose::STANDARD.decode(encrypted_b64) else {
        return (MessagePreview::default(), false);
    };
    match ks.decrypt_bytes(account_id, &ciphertext) {
        Ok(plain) => match serde_json::from_slice::<MessagePreview>(&plain) {
            Ok(preview) => (preview, true),
            Err(_) => (MessagePreview::default(), false),
        },
        Err(_) => (MessagePreview::default(), false),
    }
}

fn urlencode(value: &str) -> String {
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

const SESSION_EXPIRED: &str = "session expired";
const HINT_LAG: std::time::Duration = std::time::Duration::from_secs(6);

pub fn poke(app: &AppHandle, account_id: &str) {
    let mirror = app.state::<Mirror>();
    if !mirror.is_watching(account_id) || !mirror.begin_poke(account_id) {
        return;
    }
    let app = app.clone();
    let account_id = account_id.to_owned();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(HINT_LAG).await;
        app.state::<Mirror>().finish_poke(&account_id);
    });
}
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(15 * 60);

fn next_delay(failures: u32) -> std::time::Duration {
    if failures == 0 {
        return jitter(POLL_INTERVAL);
    }
    let scaled = POLL_INTERVAL
        .saturating_mul(2u32.saturating_pow(failures.min(6)))
        .min(MAX_BACKOFF);
    jitter(scaled)
}

fn jitter(base: std::time::Duration) -> std::time::Duration {
    use rand::Rng;
    let spread = base.as_millis() as u64 / 5;
    if spread == 0 {
        return base;
    }
    let offset = rand::thread_rng().gen_range(0..=spread);
    base + std::time::Duration::from_millis(offset)
}

pub async fn watch_inbox(app: AppHandle, account_id: String) {
    if !app.state::<Mirror>().claim_watch(&account_id) {
        return;
    }

    let mut failures: u32 = 0;

    loop {
        if !app.state::<Mirror>().is_watching(&account_id) {
            break;
        }

        let token = app.state::<Mirror>().token(&account_id);
        if let Some(token) = token {
            match poll_changes(&app, &account_id, &token).await {
                Ok(arrivals) => {
                    failures = 0;
                    eprintln!(
                        "mirror: poll for {account_id} saw {} arrival(s)",
                        arrivals.len()
                    );
                    announce(&app, &account_id, &arrivals);
                }
                Err(err) => {
                    failures = failures.saturating_add(1);
                    eprintln!("mirror: poll for {account_id} failed ({failures}): {err}");
                    if err == SESSION_EXPIRED {
                        let _ = app.emit(
                            "mirror://token-expired",
                            serde_json::json!({ "accountId": account_id }),
                        );
                    }
                    let _ = app.emit(
                        "mirror://error",
                        serde_json::json!({ "accountId": account_id, "error": err }),
                    );
                }
            }
        }

        let waker = app.state::<Mirror>().token_waker(&account_id);
        if tokio::time::timeout(next_delay(failures), waker.notified())
            .await
            .is_ok()
        {
            failures = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_stay_silent_until_the_first_backfill_finishes() {
        let mirror = Mirror::default();
        assert!(
            !mirror.notifications_armed("acct"),
            "a fresh mirror must not notify: every message in the first sync looks new"
        );
        mirror.arm_notifications("acct");
        assert!(mirror.notifications_armed("acct"));
    }

    #[test]
    fn purging_an_account_forgets_its_sync_bookkeeping() {
        let mirror = Mirror::default();
        let account = "0f0f0f0f-0f0f-4f0f-8f0f-0f0f0f0f0f0f";
        assert!(mirror.claim(account));
        mirror.arm_notifications(account);
        mirror.purge(account).expect("purge");
        assert!(mirror.claim(account), "a purged account must be claimable again");
        assert!(!mirror.notifications_armed(account));
    }

    #[tokio::test]
    async fn a_changed_token_wakes_the_watcher_and_an_unchanged_one_does_not() {
        let mirror = Mirror::default();
        let account = "acct";
        assert!(mirror.claim_watch(account));
        let waker = mirror.token_waker(account);
        let wait = || tokio::time::timeout(std::time::Duration::from_millis(20), waker.notified());

        mirror.set_token(account, "first");
        assert!(wait().await.is_ok(), "a new token must wake the watcher");

        mirror.set_token(account, "first");
        assert!(wait().await.is_err(), "pushing the same token again must not wake it");

        mirror.set_token(account, "second");
        assert!(wait().await.is_ok());
    }

    #[tokio::test]
    async fn a_hint_wakes_the_watcher_once_and_only_while_it_runs() {
        let mirror = Mirror::default();
        let account = "acct";
        assert!(mirror.claim_watch(account));
        let waker = mirror.token_waker(account);
        assert!(mirror.begin_poke(account));
        assert!(!mirror.begin_poke(account), "a pending poke must coalesce");
        mirror.finish_poke(account);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), waker.notified())
                .await
                .is_ok()
        );
        assert!(mirror.begin_poke(account), "a finished poke must allow the next one");
    }

    #[test]
    fn a_token_set_before_the_watcher_starts_leaves_no_permit() {
        let mirror = Mirror::default();
        mirror.set_token("acct", "first");
        let waker = mirror.token_waker("acct");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime");
        let woke = rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(20), waker.notified()).await
        });
        assert!(woke.is_err());
    }

    fn item(id: &str, direction: &str, mailbox: &str, read: bool, stored_at: &str) -> MessageListItem {
        MessageListItem {
            id: id.to_owned(),
            direction: direction.to_owned(),
            source: String::new(),
            mailbox_state: mailbox.to_owned(),
            starred: false,
            read,
            stored_at: stored_at.to_owned(),
            body_size_bytes: 0,
            attachment_count: 0,
            thread_root_id: None,
            encrypted_preview: None,
        }
    }

    #[test]
    fn only_unread_inbox_mail_that_arrived_after_launch_is_announced() {
        let preview = MessagePreview::default();
        let since = chrono_parse("2026-09-02T12:00:00Z").expect("since");
        let fresh = item("m1", "received", "inbox", false, "2026-09-02T12:00:05Z");
        assert!(arrival_for("acct", &fresh, &preview, true, since).is_some());
        assert!(
            arrival_for("acct", &fresh, &preview, false, since).is_none(),
            "an undecryptable preview has nothing to show"
        );

        let old = item("m2", "received", "inbox", false, "2026-09-02T11:59:59Z");
        assert!(arrival_for("acct", &old, &preview, true, since).is_none());

        let read = item("m3", "received", "inbox", true, "2026-09-02T12:00:05Z");
        assert!(arrival_for("acct", &read, &preview, true, since).is_none());

        let sent = item("m4", "sent", "inbox", false, "2026-09-02T12:00:05Z");
        assert!(arrival_for("acct", &sent, &preview, true, since).is_none());

        let spam = item("m5", "received", "spam", false, "2026-09-02T12:00:05Z");
        assert!(arrival_for("acct", &spam, &preview, true, since).is_none());
    }

    #[test]
    fn repeated_failures_back_off_and_stay_bounded() {
        let first = next_delay(1);
        let later = next_delay(4);
        assert!(
            later > first,
            "consecutive failures must widen the poll interval"
        );
        for failures in 0..64 {
            assert!(
                next_delay(failures) <= MAX_BACKOFF + MAX_BACKOFF / 5,
                "backoff must stay bounded at {failures} failures"
            );
        }
    }

    #[test]
    fn a_healthy_poll_returns_to_the_base_interval() {
        let delay = next_delay(0);
        assert!(delay >= POLL_INTERVAL);
        assert!(delay <= POLL_INTERVAL + POLL_INTERVAL / 5);
    }

    #[test]
    fn only_one_watcher_runs_per_account() {
        let mirror = Mirror::default();
        assert!(mirror.claim_watch("acct"));
        assert!(
            !mirror.claim_watch("acct"),
            "a second watcher must not start"
        );
        mirror.stop_watch("acct");
        assert!(mirror.claim_watch("acct"), "stopping must allow a restart");
    }

    #[test]
    fn a_filename_from_a_hostile_header_cannot_escape_the_save_directory() {
        assert_eq!(crate::shell::safe_filename("../../etc/passwd"), "etcpasswd");
        assert_eq!(crate::shell::safe_filename("  .hidden"), "hidden");
        assert_eq!(crate::shell::safe_filename(""), "attachment");
        assert_eq!(crate::shell::safe_filename("report.pdf"), "report.pdf");
    }
}

const BODY_BATCH: usize = 16;
const PRESIGN_MARGIN_SECS: i64 = 10;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageDetailResponse {
    body: PresignedPointer,
    #[serde(default)]
    attachments: Vec<AttachmentDetailResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PresignedPointer {
    url: String,
    #[serde(default)]
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentDetailResponse {
    id: String,
    #[serde(default)]
    ordinal: i64,
    #[serde(default)]
    is_inline: bool,
}

struct FetchedBody {
    rowid: i64,
    mime: Vec<u8>,
    extracted: thelemail_mime::Extracted,
    attachment_ids: Vec<(String, i64, bool)>,
}

fn expires_too_soon(expires_at: &Option<String>) -> bool {
    let Some(raw) = expires_at else { return false };
    let Ok(when) = chrono_parse(raw) else {
        return false;
    };
    when - now_unix() < PRESIGN_MARGIN_SECS
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn chrono_parse(raw: &str) -> Result<i64, ()> {
    let bytes = raw.as_bytes();
    if bytes.len() < 19 {
        return Err(());
    }
    let num = |a: usize, b: usize| -> Result<i64, ()> {
        raw.get(a..b).ok_or(())?.parse::<i64>().map_err(|_| ())
    };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let days = days_from_civil(y, mo, d);
    Ok(days * 86_400 + h * 3600 + mi * 60 + sec)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub async fn prefetch_bodies(
    net: &Net,
    ks: &Keystore,
    mirror: &Mirror,
    account_id: &str,
    access_token: &str,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<u64, String> {
    let work = mirror.with_conn(account_id, pending_bodies)?;
    let total = work.len() as u64;
    if total == 0 {
        return Ok(0);
    }
    on_progress(0, total);

    let mut done: u64 = 0;
    for chunk in work.chunks(BODY_BATCH) {
        let pending: Vec<_> = chunk
            .iter()
            .map(|(rowid, message_id)| {
                fetch_one_body(net, ks, account_id, access_token, *rowid, message_id)
            })
            .collect();
        let results: Vec<Result<Option<FetchedBody>, String>> = stream::iter(pending)
            .buffer_unordered(BODY_CONCURRENCY)
            .collect()
            .await;

        let mut fetched = Vec::with_capacity(chunk.len());
        for result in results {
            match result {
                Ok(Some(body)) => fetched.push(body),
                Ok(None) => {}
                Err(err) if err == "session expired" => return Err(err),
                Err(_) => {}
            }
        }

        if !fetched.is_empty() {
            mirror.with_conn(account_id, |conn| store_bodies(conn, &fetched))?;
        }
        done += chunk.len() as u64;
        on_progress(done, total);
    }

    Ok(done)
}

fn pending_bodies(conn: &Connection) -> Result<Vec<(i64, String)>, String> {
    let floor: Option<String> = conn
        .query_row(
            "SELECT date_floor FROM sync_state WHERE scope = '__scope__'",
            [],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap_or(None)
        .filter(|value| value != "all");

    let mut stmt = conn
        .prepare(
            "SELECT m.rowid, m.id FROM messages m \
             LEFT JOIN bodies b ON b.rowid = m.rowid \
             WHERE m.deleted = 0 AND b.rowid IS NULL \
               AND (?1 IS NULL OR m.stored_at >= ?1) \
             ORDER BY m.stored_at DESC LIMIT 5000",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![floor], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

async fn fetch_one_body(
    net: &Net,
    ks: &Keystore,
    account_id: &str,
    access_token: &str,
    rowid: i64,
    message_id: &str,
) -> Result<Option<FetchedBody>, String> {
    for attempt in 0..2 {
        let detail = match get_detail(net, account_id, access_token, message_id).await {
            Ok(detail) => detail,
            Err(err) => return Err(err),
        };
        if expires_too_soon(&detail.body.expires_at) && attempt == 0 {
            continue;
        }

        let ciphertext = match net.blob_get(&detail.body.url).await {
            Ok(bytes) => bytes,
            Err(_) if attempt == 0 => continue,
            Err(e) => return Err(e.to_string()),
        };

        let plain = ks
            .decrypt_bytes(account_id, &ciphertext)
            .map_err(|code| code.to_owned())?;
        let mime = unwrap_pgp_layers(ks, account_id, plain)?;
        let extracted = thelemail_mime::extract(&mime, thelemail_mime::DEFAULT_MAX_TEXT_BYTES);

        return Ok(Some(FetchedBody {
            rowid,
            mime,
            extracted,
            attachment_ids: detail
                .attachments
                .into_iter()
                .map(|a| (a.id, a.ordinal, a.is_inline))
                .collect(),
        }));
    }
    Ok(None)
}

async fn get_detail(
    net: &Net,
    account_id: &str,
    access_token: &str,
    message_id: &str,
) -> Result<MessageDetailResponse, String> {
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_owned(), format!("Bearer {access_token}"));
    headers.insert("X-Account-Id".to_owned(), account_id.to_owned());
    headers.insert("X-Client".to_owned(), "desktop".to_owned());

    let resp = net
        .request(ApiRequest {
            url: format!("{}v1/messages/{message_id}", net.config().api_base),
            method: "GET".to_owned(),
            headers,
            body: None,
        })
        .await
        .map_err(|e| e.to_string())?;

    if resp.status == 401 {
        return Err("session expired".to_owned());
    }
    if resp.status != 200 {
        return Err(format!("message detail returned {}", resp.status));
    }
    serde_json::from_slice(&resp.body.unwrap_or_default()).map_err(|e| e.to_string())
}

fn unwrap_pgp_layers(ks: &Keystore, account_id: &str, outer: Vec<u8>) -> Result<Vec<u8>, String> {
    let mut current = outer;
    for _ in 0..thelemail_mime::MAX_PGP_LAYERS {
        let Ok(text) = std::str::from_utf8(&current) else {
            return Ok(current);
        };
        if !thelemail_mime::is_pgp_encrypted_mime(text) {
            return Ok(current);
        }
        let Some(armor) = thelemail_mime::extract_pgp_armor(text) else {
            return Err("missing PGP payload".to_owned());
        };
        current = ks
            .decrypt_bytes(account_id, armor.as_bytes())
            .map_err(|code| code.to_owned())?;
    }
    Ok(current)
}

fn store_bodies(conn: &Connection, bodies: &[FetchedBody]) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let now = now_iso();

    for body in bodies {
        tx.execute(
            "INSERT INTO bodies (rowid, mime, plain_text, size_bytes, last_access) \
             VALUES (?1,?2,?3,?4,?5) \
             ON CONFLICT(rowid) DO UPDATE SET mime=excluded.mime, \
               plain_text=excluded.plain_text, size_bytes=excluded.size_bytes, \
               last_access=excluded.last_access",
            params![
                body.rowid,
                body.mime,
                body.extracted.plain_text,
                body.mime.len() as i64,
                now,
            ],
        )
        .map_err(|e| e.to_string())?;

        tx.execute(
            "UPDATE search_docs SET body = ?2 WHERE rowid = ?1",
            params![body.rowid, body.extracted.plain_text],
        )
        .map_err(|e| e.to_string())?;

        tx.execute(
            "DELETE FROM attachments WHERE message_rowid = ?1",
            params![body.rowid],
        )
        .map_err(|e| e.to_string())?;

        for (index, meta) in body.extracted.attachments.iter().enumerate() {
            let (id, ordinal, is_inline) = match body.attachment_ids.get(index) {
                Some((id, ordinal, inline)) => (id.clone(), *ordinal, *inline || meta.is_inline),
                None => (
                    format!("{}:{}", body.rowid, meta.ordinal),
                    meta.ordinal as i64,
                    meta.is_inline,
                ),
            };
            tx.execute(
                "INSERT INTO attachments (id, message_rowid, ordinal, filename, content_type, \
                  disposition, content_id, plaintext_size, is_inline) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) \
                 ON CONFLICT(id) DO UPDATE SET filename=excluded.filename, \
                   content_type=excluded.content_type, disposition=excluded.disposition, \
                   content_id=excluded.content_id, plaintext_size=excluded.plaintext_size, \
                   is_inline=excluded.is_inline",
                params![
                    id,
                    body.rowid,
                    ordinal,
                    meta.filename,
                    meta.content_type,
                    meta.disposition,
                    meta.content_id,
                    meta.plaintext_size as i64,
                    is_inline as i64,
                ],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    tx.commit().map_err(|e| e.to_string())
}

pub fn apply_changes(
    conn: &Connection,
    ks: &Keystore,
    account_id: &str,
    changes: &[MessageChange],
    next_cursor: &str,
    since: i64,
) -> Result<Vec<crate::notify::NewMail>, String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut fresh = Vec::new();

    for change in changes {
        if change.deleted {
            tx.execute(
                "UPDATE messages SET deleted = 1 WHERE id = ?1",
                params![change.id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "DELETE FROM search_docs WHERE rowid = (SELECT rowid FROM messages WHERE id = ?1)",
                params![change.id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "DELETE FROM bodies WHERE rowid = (SELECT rowid FROM messages WHERE id = ?1)",
                params![change.id],
            )
            .map_err(|e| e.to_string())?;
            continue;
        }

        if let Some(item) = &change.message {
            let (preview, decrypted) = match &item.encrypted_preview {
                Some(b64) => decrypt_preview(ks, account_id, b64),
                None => (MessagePreview::default(), false),
            };
            let known = message_known(&tx, &item.id);
            upsert_message(&tx, item, &preview, decrypted).map_err(|e| e.to_string())?;
            if !known
                && let Some(mail) = arrival_for(account_id, item, &preview, decrypted, since)
            {
                fresh.push(mail);
            }
        }
    }

    tx.execute(
        "INSERT INTO sync_state (scope, delta_token) VALUES (?1, ?2) \
         ON CONFLICT(scope) DO UPDATE SET delta_token = excluded.delta_token",
        params![DELTA_SCOPE, next_cursor],
    )
    .map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(fresh)
}

async fn poll_changes(
    app: &AppHandle,
    account_id: &str,
    access_token: &str,
) -> Result<Vec<crate::notify::NewMail>, String> {
    let net = app.state::<Net>();
    let ks = app.state::<Keystore>();
    let mirror = app.state::<Mirror>();

    let mut arrivals = Vec::new();
    loop {
        let cursor: Option<String> = mirror.with_conn(account_id, |conn| {
            Ok(conn
                .query_row(
                    "SELECT delta_token FROM sync_state WHERE scope = ?1",
                    [DELTA_SCOPE],
                    |r| r.get::<_, Option<String>>(0),
                )
                .unwrap_or(None))
        })?;

        let mut url = format!(
            "{}v1/messages/changes?limit={DELTA_LIMIT}",
            net.config().api_base
        );
        if let Some(c) = &cursor {
            url.push_str(&format!("&cursor={}", urlencode(c)));
        }

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_owned(), format!("Bearer {access_token}"));
        headers.insert("X-Account-Id".to_owned(), account_id.to_owned());
        headers.insert("X-Client".to_owned(), "desktop".to_owned());

        let resp = net
            .request(ApiRequest {
                url,
                method: "GET".to_owned(),
                headers,
                body: None,
            })
            .await
            .map_err(|e| e.to_string())?;

        if resp.status == 401 {
            return Err(SESSION_EXPIRED.to_owned());
        }
        if resp.status != 200 {
            return Err(format!("changes returned {}", resp.status));
        }

        let page: MessageChangesResponse =
            serde_json::from_slice(&resp.body.unwrap_or_default()).map_err(|e| e.to_string())?;

        if page.resync_required {
            mirror.with_conn(account_id, |conn| {
                reset_for_resync(conn, &page.next_cursor)
            })?;
            eprintln!("mirror: {account_id} is past the change horizon, rebuilding from the watermark");
            tauri::async_runtime::spawn(backfill(
                app.clone(),
                account_id.to_owned(),
                access_token.to_owned(),
            ));
            return Ok(arrivals);
        }

        let empty = page.changes.is_empty();
        let arrived = mirror.with_conn(account_id, |conn| {
            apply_changes(
                conn,
                &ks,
                account_id,
                &page.changes,
                &page.next_cursor,
                mirror.started_at,
            )
        })?;
        arrivals.extend(arrived);

        if empty || !page.has_more {
            break;
        }
    }

    Ok(arrivals)
}
