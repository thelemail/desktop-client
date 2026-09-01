use rusqlite::{Connection, Transaction};

use crate::db::StoreError;

pub struct Migration {
    pub version: i32,
    pub name: &'static str,
    pub up: fn(&Transaction) -> rusqlite::Result<()>,
}

pub static MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial",
    up: m0001_initial,
}];

pub fn migrate(conn: &Connection) -> Result<(), StoreError> {
    let from: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let supported = MIGRATIONS.last().map_or(0, |m| m.version);

    if from > supported {
        return Err(StoreError::FromNewerVersion {
            found: from,
            supported,
        });
    }

    for migration in MIGRATIONS.iter().filter(|m| m.version > from) {
        let tx = conn.unchecked_transaction()?;
        (migration.up)(&tx)?;
        tx.pragma_update(None, "user_version", migration.version)?;
        tx.commit()?;
    }
    Ok(())
}

fn m0001_initial(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        r#"
CREATE TABLE schema_meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE messages (
  rowid                  INTEGER PRIMARY KEY,
  id                     TEXT NOT NULL UNIQUE,
  direction              TEXT NOT NULL,
  source                 TEXT NOT NULL DEFAULT '',
  mailbox_state          TEXT NOT NULL,
  starred                INTEGER NOT NULL DEFAULT 0,
  read                   INTEGER NOT NULL DEFAULT 0,
  snoozed_until          TEXT,
  stored_at              TEXT NOT NULL,
  updated_at             TEXT,
  body_size_bytes        INTEGER NOT NULL DEFAULT 0,
  attachment_count       INTEGER NOT NULL DEFAULT 0,
  thread_root_id         TEXT,
  thread_count           INTEGER,
  external_message_id    TEXT,
  in_reply_to            TEXT,
  references_json        TEXT NOT NULL DEFAULT '[]',
  labels_json            TEXT NOT NULL DEFAULT '[]',
  signature_status       TEXT,
  subject                TEXT NOT NULL DEFAULT '',
  sender_display         TEXT NOT NULL DEFAULT '',
  sender_address         TEXT NOT NULL DEFAULT '',
  recipients_json        TEXT NOT NULL DEFAULT '[]',
  snippet                TEXT NOT NULL DEFAULT '',
  display_date           TEXT NOT NULL DEFAULT '',
  preview_state          TEXT NOT NULL DEFAULT 'pending',
  dirty                  INTEGER NOT NULL DEFAULT 0,
  deleted                INTEGER NOT NULL DEFAULT 0,
  synced_at              TEXT NOT NULL
);

CREATE INDEX ix_msg_mailbox_time ON messages(mailbox_state, stored_at DESC, id DESC) WHERE deleted = 0;
CREATE INDEX ix_msg_thread ON messages(COALESCE(thread_root_id, id), stored_at) WHERE deleted = 0;
CREATE INDEX ix_msg_starred ON messages(stored_at DESC) WHERE starred = 1 AND deleted = 0;
CREATE INDEX ix_msg_unread ON messages(mailbox_state, stored_at DESC) WHERE read = 0 AND deleted = 0;
CREATE INDEX ix_msg_dirty ON messages(rowid) WHERE dirty = 1;

CREATE TABLE search_docs (
  rowid      INTEGER PRIMARY KEY REFERENCES messages(rowid) ON DELETE CASCADE,
  subject    TEXT NOT NULL DEFAULT '',
  sender     TEXT NOT NULL DEFAULT '',
  recipients TEXT NOT NULL DEFAULT '',
  snippet    TEXT NOT NULL DEFAULT '',
  body       TEXT NOT NULL DEFAULT ''
);

CREATE VIRTUAL TABLE message_fts USING fts5(
  subject, sender, recipients, snippet, body,
  content       = 'search_docs',
  content_rowid = 'rowid',
  tokenize      = 'unicode61 remove_diacritics 2',
  prefix        = '2 3'
);

CREATE TRIGGER trg_search_docs_ai AFTER INSERT ON search_docs BEGIN
  INSERT INTO message_fts(rowid, subject, sender, recipients, snippet, body)
  VALUES (new.rowid, new.subject, new.sender, new.recipients, new.snippet, new.body);
END;

CREATE TRIGGER trg_search_docs_ad AFTER DELETE ON search_docs BEGIN
  INSERT INTO message_fts(message_fts, rowid, subject, sender, recipients, snippet, body)
  VALUES ('delete', old.rowid, old.subject, old.sender, old.recipients, old.snippet, old.body);
END;

CREATE TRIGGER trg_search_docs_au AFTER UPDATE ON search_docs BEGIN
  INSERT INTO message_fts(message_fts, rowid, subject, sender, recipients, snippet, body)
  VALUES ('delete', old.rowid, old.subject, old.sender, old.recipients, old.snippet, old.body);
  INSERT INTO message_fts(rowid, subject, sender, recipients, snippet, body)
  VALUES (new.rowid, new.subject, new.sender, new.recipients, new.snippet, new.body);
END;

CREATE TABLE bodies (
  rowid       INTEGER PRIMARY KEY REFERENCES messages(rowid) ON DELETE CASCADE,
  mime        BLOB,
  html        TEXT,
  plain_text  TEXT,
  size_bytes  INTEGER NOT NULL DEFAULT 0,
  last_access TEXT NOT NULL
);
CREATE INDEX ix_bodies_lru ON bodies(last_access);

CREATE TABLE attachments (
  id             TEXT PRIMARY KEY,
  message_rowid  INTEGER NOT NULL REFERENCES messages(rowid) ON DELETE CASCADE,
  ordinal        INTEGER NOT NULL DEFAULT 0,
  filename       TEXT NOT NULL DEFAULT '',
  content_type   TEXT NOT NULL DEFAULT '',
  disposition    TEXT NOT NULL DEFAULT 'attachment',
  content_id     TEXT,
  plaintext_size INTEGER NOT NULL DEFAULT 0,
  is_inline      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX ix_att_msg ON attachments(message_rowid, ordinal);

CREATE TABLE attachment_blobs (
  id          TEXT PRIMARY KEY REFERENCES attachments(id) ON DELETE CASCADE,
  bytes       BLOB NOT NULL,
  size_bytes  INTEGER NOT NULL,
  last_access TEXT NOT NULL
);
CREATE INDEX ix_attblob_lru ON attachment_blobs(last_access);

CREATE TABLE sync_state (
  scope            TEXT PRIMARY KEY,
  cursor           TEXT,
  delta_token      TEXT,
  backfill_done    INTEGER NOT NULL DEFAULT 0,
  oldest_stored_at TEXT,
  date_floor       TEXT,
  last_full_sweep  TEXT
);

CREATE TABLE outbox (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  message_id      TEXT NOT NULL,
  op              TEXT NOT NULL,
  payload_json    TEXT NOT NULL DEFAULT '{}',
  idempotency_key TEXT NOT NULL UNIQUE,
  attempts        INTEGER NOT NULL DEFAULT 0,
  next_attempt_at TEXT NOT NULL,
  last_error      TEXT,
  created_at      TEXT NOT NULL
);
CREATE INDEX ix_outbox_due ON outbox(next_attempt_at);

CREATE TABLE mailbox_counts (
  scope      TEXT PRIMARY KEY,
  total      INTEGER NOT NULL DEFAULT 0,
  unread     INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);
"#,
    )
}
