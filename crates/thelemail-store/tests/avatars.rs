use thelemail_store::avatars::{
    AvatarKind, CachedAvatar, all_avatars, cached_avatar, clear_avatars, prune_avatars,
    store_avatar,
};
use thelemail_store::db::{generate_db_key, open_account_db};

const ACCOUNT: &str = "11111111-2222-3333-4444-555555555555";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nAVATAR-PIXELS-0123456789";
const SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><title>Acme logo mark</title></svg>";

fn photo(bytes: &[u8], source: &str) -> CachedAvatar {
    CachedAvatar {
        source: source.into(),
        format: "png".into(),
        bytes: Some(bytes.to_vec()),
        checked_at: 100,
    }
}

fn memory() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().expect("db");
    thelemail_store::migrations::migrate(&conn).expect("migrate");
    conn
}

#[test]
fn a_cached_photo_round_trips_by_kind_and_key() {
    let conn = memory();
    let avatar = photo(PNG, "/v1/avatars/a/1");
    store_avatar(&conn, AvatarKind::Person, "ada@acme.test", &avatar, 5).expect("store");
    assert_eq!(
        cached_avatar(&conn, AvatarKind::Person, "ada@acme.test", 6).expect("read"),
        Some(avatar)
    );
    assert_eq!(
        cached_avatar(&conn, AvatarKind::Bimi, "ada@acme.test", 6).expect("read"),
        None
    );
}

#[test]
fn a_negative_answer_is_kept_so_offline_shows_initials_at_once() {
    let conn = memory();
    let none = CachedAvatar {
        source: String::new(),
        format: String::new(),
        bytes: None,
        checked_at: 42,
    };
    store_avatar(&conn, AvatarKind::Bimi, "acme.test", &none, 42).expect("store");
    let found = cached_avatar(&conn, AvatarKind::Bimi, "acme.test", 43)
        .expect("read")
        .expect("row");
    assert_eq!(found.bytes, None);
    assert_eq!(found.checked_at, 42);
}

#[test]
fn replacing_an_entry_overwrites_it() {
    let conn = memory();
    store_avatar(&conn, AvatarKind::Person, "k", &photo(PNG, "/old"), 1).expect("store");
    store_avatar(&conn, AvatarKind::Person, "k", &photo(b"new", "/new"), 2).expect("store");
    let found = cached_avatar(&conn, AvatarKind::Person, "k", 3)
        .expect("read")
        .expect("row");
    assert_eq!(found.source, "/new");
    assert_eq!(found.bytes.as_deref(), Some(&b"new"[..]));
}

#[test]
fn pruning_keeps_the_most_recently_used() {
    let conn = memory();
    for (i, key) in ["a", "b", "c", "d"].iter().enumerate() {
        store_avatar(&conn, AvatarKind::Person, key, &photo(PNG, key), i as i64).expect("store");
    }
    cached_avatar(&conn, AvatarKind::Person, "a", 10).expect("touch");
    assert_eq!(prune_avatars(&conn, 2).expect("prune"), 2);
    let kept: Vec<String> = all_avatars(&conn, 10)
        .expect("all")
        .into_iter()
        .map(|(_, key, _)| key)
        .collect();
    assert_eq!(kept, vec!["a".to_string(), "d".to_string()]);
    clear_avatars(&conn).expect("clear");
    assert!(all_avatars(&conn, 10).expect("all").is_empty());
}

#[test]
fn the_avatar_migration_keeps_every_sync_cursor() {
    let conn = rusqlite::Connection::open_in_memory().expect("db");
    for migration in thelemail_store::migrations::MIGRATIONS
        .iter()
        .filter(|m| m.version < 8)
    {
        let tx = conn.unchecked_transaction().expect("tx");
        (migration.up)(&tx).expect("migration");
        tx.pragma_update(None, "user_version", migration.version)
            .expect("user_version");
        tx.commit().expect("commit");
    }
    conn.execute(
        "INSERT INTO sync_state (scope, cursor) VALUES ('inbox', 'c-9')",
        [],
    )
    .expect("seed");
    thelemail_store::migrations::migrate(&conn).expect("migrate");
    let cursor: Option<String> = conn
        .query_row(
            "SELECT cursor FROM sync_state WHERE scope = 'inbox'",
            [],
            |r| r.get(0),
        )
        .expect("cursor");
    assert_eq!(cursor.as_deref(), Some("c-9"));
    let columns: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('messages')")
        .expect("prepare")
        .query_map([], |r| r.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    assert!(columns.iter().any(|c| c == "bimi_domain"));
}

#[test]
fn cached_images_never_reach_disk_in_plaintext() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("offline.db");
    let key = generate_db_key();
    let conn = open_account_db(&path, &key, ACCOUNT).expect("open");
    store_avatar(
        &conn,
        AvatarKind::Person,
        "ada@acme.test",
        &photo(PNG, "/p"),
        1,
    )
    .expect("store");
    let logo = CachedAvatar {
        source: String::new(),
        format: "svg".into(),
        bytes: Some(SVG.to_vec()),
        checked_at: 1,
    };
    store_avatar(&conn, AvatarKind::Bimi, "acme.test", &logo, 1).expect("store");
    for entry in std::fs::read_dir(dir.path()).expect("dir") {
        let bytes = std::fs::read(entry.expect("entry").path()).expect("read");
        let text = String::from_utf8_lossy(&bytes);
        for needle in [
            "AVATAR-PIXELS",
            "Acme logo mark",
            "ada@acme.test",
            "acme.test",
        ] {
            assert!(!text.contains(needle), "{needle:?} found in plaintext");
        }
        assert!(!bytes.windows(4).any(|w| w == b"\x89PNG"));
    }
    drop(conn);
    for entry in std::fs::read_dir(dir.path()).expect("dir") {
        let bytes = std::fs::read(entry.expect("entry").path()).expect("read");
        assert!(!String::from_utf8_lossy(&bytes).contains("AVATAR-PIXELS"));
    }
}
