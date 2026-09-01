use rusqlite::params;
use thelemail_store::db::{StoreError, account_db_path, generate_db_key, open_account_db};
use thelemail_store::search::{parse_query, search_messages};

const ACCOUNT: &str = "11111111-2222-3333-4444-555555555555";

fn temp_db() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("offline.db");
    (dir, path)
}

fn seed(conn: &rusqlite::Connection, id: &str, subject: &str, sender: &str, body: &str) {
    conn.execute(
        "INSERT INTO messages (id, direction, mailbox_state, stored_at, subject, sender_display, \
         sender_address, snippet, synced_at) VALUES (?1,'received','inbox','2026-08-31T10:00:00Z',?2,?3,?4,?5,'2026-08-31T10:00:00Z')",
        params![id, subject, sender, sender, body],
    )
    .expect("insert message");
    let rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO search_docs (rowid, subject, sender, recipients, snippet, body) VALUES (?1,?2,?3,'',?4,?5)",
        params![rowid, subject, sender, body, body],
    )
    .expect("insert search doc");
}

#[test]
fn opens_an_encrypted_database_and_migrates() {
    let (_dir, path) = temp_db();
    let key = generate_db_key();
    let conn = open_account_db(&path, &key, ACCOUNT).expect("open");
    let version: i32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .expect("user_version");
    assert!(version >= 1);
}

#[test]
fn the_database_file_is_not_plaintext_sqlite() {
    let (_dir, path) = temp_db();
    let key = generate_db_key();
    let conn = open_account_db(&path, &key, ACCOUNT).expect("open");
    seed(
        &conn,
        "m1",
        "Quarterly report",
        "alice@example.com",
        "the numbers look good",
    );
    drop(conn);

    let bytes = std::fs::read(&path).expect("read db");
    assert!(
        !bytes.starts_with(b"SQLite format 3"),
        "database header is plaintext"
    );
    let haystack = String::from_utf8_lossy(&bytes);
    for needle in [
        "Quarterly report",
        "alice@example.com",
        "the numbers look good",
    ] {
        assert!(
            !haystack.contains(needle),
            "plaintext {needle:?} found in the database file"
        );
    }
}

#[test]
fn a_wrong_key_cannot_open_the_database() {
    let (_dir, path) = temp_db();
    let key = generate_db_key();
    drop(open_account_db(&path, &key, ACCOUNT).expect("open"));
    assert!(open_account_db(&path, &generate_db_key(), ACCOUNT).is_err());
}

#[test]
fn refuses_a_database_belonging_to_another_account() {
    let (_dir, path) = temp_db();
    let key = generate_db_key();
    drop(open_account_db(&path, &key, ACCOUNT).expect("open"));
    let err = open_account_db(&path, &key, "99999999-9999-9999-9999-999999999999")
        .expect_err("must refuse");
    assert!(matches!(err, StoreError::WrongAccount));
}

#[test]
fn full_text_search_finds_body_matches_and_ranks_subject_higher() {
    let (_dir, path) = temp_db();
    let key = generate_db_key();
    let conn = open_account_db(&path, &key, ACCOUNT).expect("open");

    seed(
        &conn,
        "m1",
        "Unrelated",
        "bob@example.com",
        "mentions invoice deep in the body",
    );
    seed(
        &conn,
        "m2",
        "Invoice for August",
        "alice@example.com",
        "nothing else",
    );

    let hits = search_messages(&conn, "invoice", None).expect("search");
    assert_eq!(hits.len(), 2, "body and subject matches must both be found");
    assert_eq!(hits[0].id, "m2", "a subject hit must outrank a body hit");
}

#[test]
fn search_supports_prefix_and_structured_operators() {
    let (_dir, path) = temp_db();
    let key = generate_db_key();
    let conn = open_account_db(&path, &key, ACCOUNT).expect("open");

    seed(
        &conn,
        "m1",
        "Roadmap",
        "alice@example.com",
        "planning notes",
    );
    seed(&conn, "m2", "Roadside", "bob@example.com", "planning notes");

    assert_eq!(
        search_messages(&conn, "roadm", None).expect("prefix").len(),
        1
    );
    let from_alice = search_messages(&conn, "planning from:alice", None).expect("from");
    assert_eq!(from_alice.len(), 1);
    assert_eq!(from_alice[0].id, "m1");
}

#[test]
fn the_index_stays_in_step_with_edits_and_deletes() {
    let (_dir, path) = temp_db();
    let key = generate_db_key();
    let conn = open_account_db(&path, &key, ACCOUNT).expect("open");

    seed(&conn, "m1", "Original", "alice@example.com", "first body");
    conn.execute(
        "UPDATE search_docs SET snippet = 'replaced body', body = 'replaced body' \
         WHERE rowid = (SELECT rowid FROM messages WHERE id = 'm1')",
        [],
    )
    .expect("update");

    assert_eq!(
        search_messages(&conn, "first", None).expect("stale").len(),
        0
    );
    assert_eq!(
        search_messages(&conn, "replaced", None)
            .expect("fresh")
            .len(),
        1
    );

    conn.execute("DELETE FROM messages WHERE id = 'm1'", [])
        .expect("delete");
    assert_eq!(
        search_messages(&conn, "replaced", None)
            .expect("gone")
            .len(),
        0
    );

    conn.execute(
        "INSERT INTO message_fts(message_fts) VALUES('integrity-check')",
        [],
    )
    .expect("fts integrity check");
}

#[test]
fn a_query_cannot_inject_fts_operators() {
    for hostile in ["foo OR bar", "\"unbalanced", "a* NEAR/2 b", "NOT x", "()"] {
        let (_dir, path) = temp_db();
        let key = generate_db_key();
        let conn = open_account_db(&path, &key, ACCOUNT).expect("open");
        seed(&conn, "m1", "Subject", "a@example.com", "body");
        search_messages(&conn, hostile, None)
            .unwrap_or_else(|e| panic!("query {hostile:?} must not error: {e}"));
    }
}

#[test]
fn structured_operators_parse_without_leaking_into_the_match() {
    let parsed = parse_query("invoice from:alice@example.com in:archive is:unread has:attachment");
    assert_eq!(parsed.from, vec!["alice@example.com"]);
    assert_eq!(parsed.mailbox.as_deref(), Some("archive"));
    assert_eq!(parsed.unread, Some(true));
    assert!(parsed.has_attachment);
    assert_eq!(parsed.fts.as_deref(), Some("\"invoice\"*"));
}

#[test]
fn a_mirrored_mailbox_lists_newest_first_and_survives_being_offline() {
    use thelemail_store::list::list_mailbox;

    let (_dir, path) = temp_db();
    let key = generate_db_key();
    let conn = open_account_db(&path, &key, ACCOUNT).expect("open");

    for (id, subject, stored_at) in [
        ("m1", "Oldest", "2026-08-01T10:00:00Z"),
        ("m2", "Middle", "2026-08-15T10:00:00Z"),
        ("m3", "Newest", "2026-08-31T10:00:00Z"),
    ] {
        conn.execute(
            "INSERT INTO messages (id, direction, mailbox_state, stored_at, subject, \
             sender_display, sender_address, snippet, synced_at) \
             VALUES (?1,'received','inbox',?2,?3,'Alice','alice@example.com','hello','t')",
            params![id, stored_at, subject],
        )
        .expect("insert");
    }
    conn.execute(
        "INSERT INTO messages (id, direction, mailbox_state, stored_at, subject, synced_at) \
         VALUES ('m4','received','archive','2026-08-31T11:00:00Z','Archived','t')",
        [],
    )
    .expect("insert archived");

    let rows = list_mailbox(&conn, "inbox", None, 50).expect("list");
    assert_eq!(
        rows.len(),
        3,
        "archive must not leak into the inbox listing"
    );
    assert_eq!(rows[0].subject, "Newest", "listing must be newest first");
    assert_eq!(rows[2].subject, "Oldest");
    assert_eq!(rows[0].sender_display, "Alice");
    assert!(!rows[0].read);

    conn.execute("UPDATE messages SET deleted = 1 WHERE id = 'm3'", [])
        .expect("tombstone");
    let rows = list_mailbox(&conn, "inbox", None, 50).expect("list");
    assert_eq!(rows.len(), 2, "tombstoned rows must not be listed");
}

#[test]
fn the_inbox_listing_separates_sent_copies_from_received_ones() {
    use thelemail_store::list::list_mailbox;

    let (_dir, path) = temp_db();
    let key = generate_db_key();
    let conn = open_account_db(&path, &key, ACCOUNT).expect("open");

    for (id, direction, subject) in [("m1", "received", "From Alice"), ("m2", "sent", "To Alice")] {
        conn.execute(
            "INSERT INTO messages (id, direction, mailbox_state, stored_at, subject, \
             sender_display, sender_address, snippet, synced_at) \
             VALUES (?1,?2,'inbox','2026-08-31T10:00:00Z',?3,'Alice','alice@example.com','hi','t')",
            params![id, direction, subject],
        )
        .expect("insert");
    }

    let received = list_mailbox(&conn, "inbox", Some("received"), 50).expect("list");
    assert_eq!(
        received.len(),
        1,
        "a sent copy must not show up in the inbox"
    );
    assert_eq!(received[0].subject, "From Alice");

    let sent = list_mailbox(&conn, "inbox", Some("sent"), 50).expect("list");
    assert_eq!(sent.len(), 1, "sent is a direction, not a mailbox state");
    assert_eq!(sent[0].subject, "To Alice");

    let both = list_mailbox(&conn, "inbox", None, 50).expect("list");
    assert_eq!(both.len(), 2, "an unfiltered listing keeps both directions");
}

const HOSTILE_IDS: &[&str] = &[
    "../../../../../../etc/passwd",
    "..",
    "../sibling",
    "/Users/someone/Library/Safari/History.db",
    "11111111-2222-3333-4444-555555555555/../../escape",
    "11111111-2222-3333-4444-55555555555G",
    "11111111-2222-3333-4444-555555555555extra",
    "11111111222233334444555555555555",
    "",
    " ",
    "\u{2024}\u{2024}/passwd",
    "acc/../../..",
];

#[test]
fn a_hostile_account_id_never_produces_a_path() {
    let root = std::path::Path::new("/tmp/thelemail-test-root");
    for id in HOSTILE_IDS {
        let result = account_db_path(root, id);
        assert!(
            result.is_err(),
            "account_db_path accepted a hostile id: {id:?}"
        );
    }
}

#[test]
fn an_uppercase_uuid_is_refused() {
    let root = std::path::Path::new("/tmp/thelemail-test-root");
    assert!(account_db_path(root, "11111111-2222-3333-4444-555555555555").is_ok());
    assert!(
        account_db_path(root, "11111111-2222-3333-4444-55555555555A").is_err(),
        "only the canonical lowercase form is accepted"
    );
}

#[test]
fn a_valid_account_id_stays_under_the_root() {
    let root = std::path::Path::new("/tmp/thelemail-test-root");
    let path = account_db_path(root, ACCOUNT).expect("valid id");
    assert!(path.starts_with(root.join("accounts")));
    assert!(path.ends_with("offline.db"));
}

#[test]
fn opening_with_a_hostile_account_id_is_refused() {
    let (_dir, path) = temp_db();
    for id in HOSTILE_IDS {
        let err = open_account_db(&path, &generate_db_key(), id)
            .expect_err("hostile account id must be refused");
        assert!(
            matches!(err, StoreError::InvalidAccountId),
            "{id:?} produced {err:?} instead of InvalidAccountId"
        );
    }
}

#[test]
fn a_plaintext_file_outside_the_account_store_is_never_deleted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let victim = dir.path().join("someone-elses.db");
    rusqlite::Connection::open(&victim)
        .expect("create victim")
        .execute_batch("CREATE TABLE t (a); INSERT INTO t VALUES (1);")
        .expect("write victim");
    assert!(victim.exists());

    let err = open_account_db(&victim, &generate_db_key(), ACCOUNT)
        .expect_err("a plaintext file outside our store must be refused");
    assert!(
        matches!(err, StoreError::PathEscapesRoot),
        "expected PathEscapesRoot, got {err:?}"
    );
    assert!(
        victim.exists(),
        "the guard deleted a file outside the application directory"
    );
}

#[test]
fn a_plaintext_file_inside_the_account_store_is_still_destroyed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = account_db_path(dir.path(), ACCOUNT).expect("valid id");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    rusqlite::Connection::open(&path)
        .expect("create plaintext")
        .execute_batch("CREATE TABLE t (a);")
        .expect("write");

    let err = open_account_db(&path, &generate_db_key(), ACCOUNT).expect_err("must refuse");
    assert!(matches!(err, StoreError::PlaintextDatabase));
    assert!(!path.exists(), "our own plaintext cache must be destroyed");
}

#[test]
fn a_body_written_to_search_docs_becomes_findable_and_survives_a_preview_refresh() {
    let (_dir, path) = temp_db();
    let key = generate_db_key();
    let conn = open_account_db(&path, &key, ACCOUNT).expect("open");

    seed(
        &conn,
        "m1",
        "Lunch plans",
        "alice@example.com",
        "see you soon",
    );
    let rowid: i64 = conn
        .query_row("SELECT rowid FROM messages WHERE id = 'm1'", [], |r| {
            r.get(0)
        })
        .expect("rowid");

    assert_eq!(
        search_messages(&conn, "photosynthesis", None)
            .expect("before")
            .len(),
        0,
        "the word must not be findable before the body is indexed"
    );

    conn.execute(
        "UPDATE search_docs SET body = 'a long discussion about photosynthesis' WHERE rowid = ?1",
        params![rowid],
    )
    .expect("index body");

    let hits = search_messages(&conn, "photosynthesis", None).expect("after");
    assert_eq!(
        hits.len(),
        1,
        "a body-only word must be findable once indexed"
    );
    assert!(
        !hits[0].excerpt.is_empty(),
        "the excerpt comes from the body column and must not be empty"
    );

    conn.execute(
        "UPDATE messages SET subject = 'Lunch plans (updated)' WHERE rowid = ?1",
        params![rowid],
    )
    .expect("refresh preview");
    conn.execute(
        "INSERT INTO search_docs (rowid, subject, sender, recipients, snippet, body) \
         VALUES (?1,'Lunch plans (updated)','Alice','','still on', \
                 COALESCE((SELECT body FROM search_docs WHERE rowid = ?1), '')) \
         ON CONFLICT(rowid) DO UPDATE SET subject=excluded.subject, sender=excluded.sender, \
           recipients=excluded.recipients, snippet=excluded.snippet",
        params![rowid],
    )
    .expect("refresh search doc");

    let hits = search_messages(&conn, "photosynthesis", None).expect("after refresh");
    assert_eq!(
        hits.len(),
        1,
        "a preview refresh must not clear the indexed body"
    );
    assert_eq!(hits[0].subject, "Lunch plans (updated)");
}

#[test]
fn deleting_a_message_removes_its_body_and_index_entry() {
    let (_dir, path) = temp_db();
    let key = generate_db_key();
    let conn = open_account_db(&path, &key, ACCOUNT).expect("open");

    seed(&conn, "m1", "Subject", "a@example.com", "snippet");
    let rowid: i64 = conn
        .query_row("SELECT rowid FROM messages WHERE id = 'm1'", [], |r| {
            r.get(0)
        })
        .expect("rowid");
    conn.execute(
        "INSERT INTO bodies (rowid, mime, plain_text, size_bytes, last_access) \
         VALUES (?1, ?2, 'mitochondria', 12, 't')",
        params![rowid, b"raw".to_vec()],
    )
    .expect("insert body");
    conn.execute(
        "UPDATE search_docs SET body = 'mitochondria' WHERE rowid = ?1",
        params![rowid],
    )
    .expect("index");

    assert_eq!(
        search_messages(&conn, "mitochondria", None)
            .expect("found")
            .len(),
        1
    );

    conn.execute("DELETE FROM messages WHERE id = 'm1'", [])
        .expect("delete");

    let bodies: i64 = conn
        .query_row("SELECT count(*) FROM bodies", [], |r| r.get(0))
        .expect("count bodies");
    assert_eq!(bodies, 0, "the body must cascade away with the message");
    assert_eq!(
        search_messages(&conn, "mitochondria", None)
            .expect("gone")
            .len(),
        0,
        "the index must not retain a deleted message"
    );
}

#[test]
fn a_tombstone_removes_a_message_from_the_list_and_the_index() {
    let (_dir, path) = temp_db();
    let key = generate_db_key();
    let conn = open_account_db(&path, &key, ACCOUNT).expect("open");

    seed(&conn, "m1", "Quarterly", "alice@example.com", "numbers");
    let rowid: i64 = conn
        .query_row("SELECT rowid FROM messages WHERE id = 'm1'", [], |r| {
            r.get(0)
        })
        .expect("rowid");
    conn.execute(
        "INSERT INTO bodies (rowid, mime, plain_text, size_bytes, last_access) \
         VALUES (?1, ?2, 'ribosome', 8, 't')",
        params![rowid, b"raw".to_vec()],
    )
    .expect("body");
    conn.execute(
        "UPDATE search_docs SET body = 'ribosome' WHERE rowid = ?1",
        params![rowid],
    )
    .expect("index");

    assert_eq!(
        search_messages(&conn, "ribosome", None)
            .expect("found")
            .len(),
        1
    );
    assert_eq!(
        thelemail_store::list::list_mailbox(&conn, "inbox", None, 50)
            .expect("list")
            .len(),
        1
    );

    conn.execute("UPDATE messages SET deleted = 1 WHERE id = 'm1'", [])
        .expect("tombstone");
    conn.execute("DELETE FROM search_docs WHERE rowid = ?1", params![rowid])
        .expect("drop index row");
    conn.execute("DELETE FROM bodies WHERE rowid = ?1", params![rowid])
        .expect("drop body");

    assert_eq!(
        search_messages(&conn, "ribosome", None)
            .expect("after")
            .len(),
        0,
        "a tombstoned message must leave the search index"
    );
    assert_eq!(
        thelemail_store::list::list_mailbox(&conn, "inbox", None, 50)
            .expect("list")
            .len(),
        0,
        "a tombstoned message must leave the mailbox listing"
    );
    let bodies: i64 = conn
        .query_row("SELECT count(*) FROM bodies", [], |r| r.get(0))
        .expect("count");
    assert_eq!(
        bodies, 0,
        "a tombstoned message must not leave its body on disk"
    );
}

#[test]
fn a_cached_attachment_round_trips_and_is_ignored_when_unknown() {
    use thelemail_store::list::{cached_attachment, store_attachment};

    let (_dir, path) = temp_db();
    let key = generate_db_key();
    let conn = open_account_db(&path, &key, ACCOUNT).expect("open");

    seed(&conn, "m1", "With file", "a@example.com", "see attached");
    let rowid: i64 = conn
        .query_row("SELECT rowid FROM messages WHERE id = 'm1'", [], |r| {
            r.get(0)
        })
        .expect("rowid");

    store_attachment(&conn, "unknown-id", b"nope", "t").expect("store unknown");
    assert!(
        cached_attachment(&conn, "unknown-id", "t")
            .expect("read")
            .is_none(),
        "bytes must not be stored for an attachment we have no metadata for"
    );

    conn.execute(
        "INSERT INTO attachments (id, message_rowid, ordinal, filename, content_type) \
         VALUES ('att-1', ?1, 0, 'report.pdf', 'application/pdf')",
        params![rowid],
    )
    .expect("metadata");

    store_attachment(&conn, "att-1", b"payload bytes", "t").expect("store");
    let got = cached_attachment(&conn, "att-1", "t").expect("read");
    assert_eq!(got.as_deref(), Some(&b"payload bytes"[..]));

    conn.execute("DELETE FROM messages WHERE id = 'm1'", [])
        .expect("delete message");
    let blobs: i64 = conn
        .query_row("SELECT count(*) FROM attachment_blobs", [], |r| r.get(0))
        .expect("count");
    assert_eq!(
        blobs, 0,
        "attachment bytes must cascade away with the message"
    );
}

#[test]
fn a_cached_message_carries_everything_the_reader_needs() {
    use thelemail_store::list::get_message;

    let (_dir, path) = temp_db();
    let key = generate_db_key();
    let conn = open_account_db(&path, &key, ACCOUNT).expect("open");

    seed(
        &conn,
        "m1",
        "Design review",
        "alice@example.com",
        "notes inside",
    );
    let rowid: i64 = conn
        .query_row("SELECT rowid FROM messages WHERE id = 'm1'", [], |r| {
            r.get(0)
        })
        .expect("rowid");
    conn.execute(
        "INSERT INTO bodies (rowid, mime, plain_text, size_bytes, last_access) \
         VALUES (?1, ?2, 'text', 4, 'old')",
        params![rowid, b"Content-Type: text/plain\r\n\r\nhello".to_vec()],
    )
    .expect("body");
    conn.execute(
        "INSERT INTO attachments (id, message_rowid, ordinal, filename, content_type, is_inline) \
         VALUES ('att-1', ?1, 0, 'spec.pdf', 'application/pdf', 0)",
        params![rowid],
    )
    .expect("attachment");

    let got = get_message(&conn, "m1", "now")
        .expect("query")
        .expect("message");
    assert_eq!(got.subject, "Design review");
    assert_eq!(got.sender_address, "alice@example.com");
    assert!(got.mime.as_deref().unwrap_or_default().contains("hello"));
    assert_eq!(got.attachments.len(), 1);
    assert_eq!(got.attachments[0].filename, "spec.pdf");

    let touched: String = conn
        .query_row(
            "SELECT last_access FROM bodies WHERE rowid = ?1",
            params![rowid],
            |r| r.get(0),
        )
        .expect("last_access");
    assert_eq!(touched, "now", "reading a body must refresh its LRU stamp");

    assert!(
        get_message(&conn, "missing", "now")
            .expect("query")
            .is_none()
    );
}
