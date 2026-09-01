use rusqlite::Connection;
use serde::Serialize;

use crate::db::StoreError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorRow {
    pub id: String,
    pub direction: String,
    pub mailbox_state: String,
    pub subject: String,
    pub sender_display: String,
    pub sender_address: String,
    pub recipients_json: String,
    pub snippet: String,
    pub display_date: String,
    pub stored_at: String,
    pub read: bool,
    pub starred: bool,
    pub attachment_count: i64,
    pub thread_root_id: Option<String>,
    pub labels_json: String,
}

pub fn list_mailbox(
    conn: &Connection,
    mailbox: &str,
    direction: Option<&str>,
    limit: usize,
) -> Result<Vec<MirrorRow>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, direction, mailbox_state, subject, sender_display, sender_address, \
                recipients_json, snippet, display_date, stored_at, read, starred, \
                attachment_count, thread_root_id, labels_json \
         FROM messages \
         WHERE deleted = 0 AND mailbox_state = ?1 \
           AND (?2 IS NULL OR direction = ?2) \
         ORDER BY stored_at DESC, id DESC \
         LIMIT ?3",
    )?;

    let rows = stmt.query_map(rusqlite::params![mailbox, direction, limit as i64], |row| {
        Ok(MirrorRow {
            id: row.get(0)?,
            direction: row.get(1)?,
            mailbox_state: row.get(2)?,
            subject: row.get(3)?,
            sender_display: row.get(4)?,
            sender_address: row.get(5)?,
            recipients_json: row.get(6)?,
            snippet: row.get(7)?,
            display_date: row.get(8)?,
            stored_at: row.get(9)?,
            read: row.get::<_, i64>(10)? != 0,
            starred: row.get::<_, i64>(11)? != 0,
            attachment_count: row.get(12)?,
            thread_root_id: row.get(13)?,
            labels_json: row.get(14)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorAttachment {
    pub id: String,
    pub ordinal: i64,
    pub filename: String,
    pub content_type: String,
    pub disposition: String,
    pub content_id: Option<String>,
    pub plaintext_size: i64,
    pub is_inline: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorMessage {
    pub id: String,
    pub direction: String,
    pub source: String,
    pub mailbox_state: String,
    pub stored_at: String,
    pub read: bool,
    pub starred: bool,
    pub thread_root_id: Option<String>,
    pub external_message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub labels_json: String,
    pub signature_status: Option<String>,
    pub subject: String,
    pub sender_display: String,
    pub sender_address: String,
    pub recipients_json: String,
    pub snippet: String,
    pub display_date: String,
    pub attachment_count: i64,
    pub mime: Option<String>,
    pub attachments: Vec<MirrorAttachment>,
}

const MESSAGE_COLUMNS: &str = "m.rowid, m.id, m.direction, m.source, m.mailbox_state, m.stored_at, \
     m.read, m.starred, m.thread_root_id, m.external_message_id, m.in_reply_to, m.labels_json, \
     m.signature_status, m.subject, m.sender_display, m.sender_address, m.recipients_json, \
     m.snippet, m.display_date, m.attachment_count, b.mime";

fn read_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, MirrorMessage)> {
    let mime: Option<Vec<u8>> = row.get(20)?;
    Ok((
        row.get(0)?,
        MirrorMessage {
            id: row.get(1)?,
            direction: row.get(2)?,
            source: row.get(3)?,
            mailbox_state: row.get(4)?,
            stored_at: row.get(5)?,
            read: row.get::<_, i64>(6)? != 0,
            starred: row.get::<_, i64>(7)? != 0,
            thread_root_id: row.get(8)?,
            external_message_id: row.get(9)?,
            in_reply_to: row.get(10)?,
            labels_json: row.get(11)?,
            signature_status: row.get(12)?,
            subject: row.get(13)?,
            sender_display: row.get(14)?,
            sender_address: row.get(15)?,
            recipients_json: row.get(16)?,
            snippet: row.get(17)?,
            display_date: row.get(18)?,
            attachment_count: row.get(19)?,
            mime: mime.and_then(|bytes| String::from_utf8(bytes).ok()),
            attachments: Vec::new(),
        },
    ))
}

fn load_attachments(conn: &Connection, rowid: i64) -> Result<Vec<MirrorAttachment>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, ordinal, filename, content_type, disposition, content_id, plaintext_size, \
                is_inline FROM attachments WHERE message_rowid = ?1 ORDER BY ordinal",
    )?;
    let rows = stmt.query_map([rowid], |row| {
        Ok(MirrorAttachment {
            id: row.get(0)?,
            ordinal: row.get(1)?,
            filename: row.get(2)?,
            content_type: row.get(3)?,
            disposition: row.get(4)?,
            content_id: row.get(5)?,
            plaintext_size: row.get(6)?,
            is_inline: row.get::<_, i64>(7)? != 0,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn touch_body(conn: &Connection, rowid: i64, now: &str) {
    let _ = conn.execute(
        "UPDATE bodies SET last_access = ?2 WHERE rowid = ?1",
        rusqlite::params![rowid, now],
    );
}

pub fn get_message(
    conn: &Connection,
    message_id: &str,
    now: &str,
) -> Result<Option<MirrorMessage>, StoreError> {
    let sql = format!(
        "SELECT {MESSAGE_COLUMNS} FROM messages m LEFT JOIN bodies b ON b.rowid = m.rowid \
         WHERE m.id = ?1 AND m.deleted = 0"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([message_id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let (rowid, mut message) = read_message(row)?;
    drop(rows);

    message.attachments = load_attachments(conn, rowid)?;
    if message.mime.is_some() {
        touch_body(conn, rowid, now);
    }
    Ok(Some(message))
}

pub fn get_thread(
    conn: &Connection,
    message_id: &str,
    now: &str,
) -> Result<Vec<MirrorMessage>, StoreError> {
    let sql = format!(
        "SELECT {MESSAGE_COLUMNS} FROM messages m LEFT JOIN bodies b ON b.rowid = m.rowid \
         WHERE m.deleted = 0 AND COALESCE(m.thread_root_id, m.id) = ( \
             SELECT COALESCE(thread_root_id, id) FROM messages WHERE id = ?1) \
         ORDER BY m.stored_at ASC, m.id ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([message_id], read_message)?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    for (rowid, message) in out.iter_mut() {
        message.attachments = load_attachments(conn, *rowid)?;
        if message.mime.is_some() {
            touch_body(conn, *rowid, now);
        }
    }
    Ok(out.into_iter().map(|(_, m)| m).collect())
}

pub fn cached_attachment(
    conn: &Connection,
    attachment_id: &str,
    now: &str,
) -> Result<Option<Vec<u8>>, StoreError> {
    let bytes: Option<Vec<u8>> = conn
        .query_row(
            "SELECT bytes FROM attachment_blobs WHERE id = ?1",
            [attachment_id],
            |row| row.get(0),
        )
        .ok();
    if bytes.is_some() {
        let _ = conn.execute(
            "UPDATE attachment_blobs SET last_access = ?2 WHERE id = ?1",
            rusqlite::params![attachment_id, now],
        );
    }
    Ok(bytes)
}

pub fn store_attachment(
    conn: &Connection,
    attachment_id: &str,
    bytes: &[u8],
    now: &str,
) -> Result<(), StoreError> {
    let known: i64 = conn.query_row(
        "SELECT count(*) FROM attachments WHERE id = ?1",
        [attachment_id],
        |row| row.get(0),
    )?;
    if known == 0 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO attachment_blobs (id, bytes, size_bytes, last_access) VALUES (?1,?2,?3,?4) \
         ON CONFLICT(id) DO UPDATE SET bytes=excluded.bytes, size_bytes=excluded.size_bytes, \
           last_access=excluded.last_access",
        rusqlite::params![attachment_id, bytes, bytes.len() as i64, now],
    )?;
    Ok(())
}
