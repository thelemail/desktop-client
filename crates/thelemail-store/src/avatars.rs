use rusqlite::{Connection, OptionalExtension, params};

use crate::db::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AvatarKind {
    Person,
    Bimi,
    Account,
}

impl AvatarKind {
    pub const ALL: [Self; 3] = [Self::Person, Self::Bimi, Self::Account];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Bimi => "bimi",
            Self::Account => "account",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedAvatar {
    pub source: String,
    pub format: String,
    pub bytes: Option<Vec<u8>>,
    pub checked_at: i64,
}

pub fn cached_avatar(
    conn: &Connection,
    kind: AvatarKind,
    key: &str,
    now: i64,
) -> Result<Option<CachedAvatar>, StoreError> {
    let found = conn
        .query_row(
            "SELECT source, format, bytes, checked_at FROM avatar_cache WHERE kind = ?1 AND key = ?2",
            params![kind.as_str(), key],
            |row| {
                Ok(CachedAvatar {
                    source: row.get(0)?,
                    format: row.get(1)?,
                    bytes: row.get(2)?,
                    checked_at: row.get(3)?,
                })
            },
        )
        .optional()?;
    if found.is_some() {
        conn.execute(
            "UPDATE avatar_cache SET last_access = ?3 WHERE kind = ?1 AND key = ?2",
            params![kind.as_str(), key, now],
        )?;
    }
    Ok(found)
}

pub fn all_avatars(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<(AvatarKind, String, CachedAvatar)>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT kind, key, source, format, bytes, checked_at FROM avatar_cache \
         ORDER BY last_access DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit as i64], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            CachedAvatar {
                source: row.get(2)?,
                format: row.get(3)?,
                bytes: row.get(4)?,
                checked_at: row.get(5)?,
            },
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (kind, key, avatar) = row?;
        if let Some(kind) = AvatarKind::parse(&kind) {
            out.push((kind, key, avatar));
        }
    }
    Ok(out)
}

pub fn store_avatar(
    conn: &Connection,
    kind: AvatarKind,
    key: &str,
    avatar: &CachedAvatar,
    now: i64,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO avatar_cache (kind, key, source, format, bytes, checked_at, last_access) \
         VALUES (?1,?2,?3,?4,?5,?6,?7) \
         ON CONFLICT(kind, key) DO UPDATE SET source=excluded.source, format=excluded.format, \
           bytes=excluded.bytes, checked_at=excluded.checked_at, last_access=excluded.last_access",
        params![
            kind.as_str(),
            key,
            avatar.source,
            avatar.format,
            avatar.bytes,
            avatar.checked_at,
            now
        ],
    )?;
    Ok(())
}

pub fn prune_avatars(conn: &Connection, keep: usize) -> Result<usize, StoreError> {
    Ok(conn.execute(
        "DELETE FROM avatar_cache WHERE rowid IN ( \
           SELECT rowid FROM avatar_cache ORDER BY last_access DESC LIMIT -1 OFFSET ?1)",
        [keep as i64],
    )?)
}

pub fn clear_avatars(conn: &Connection) -> Result<(), StoreError> {
    conn.execute("DELETE FROM avatar_cache", [])?;
    Ok(())
}
