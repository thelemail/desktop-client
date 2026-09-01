use rusqlite::Connection;
use serde::Serialize;

use crate::db::StoreError;

const MAX_TOKENS: usize = 16;
const DEFAULT_LIMIT: usize = 100;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedQuery {
    pub fts: Option<String>,
    pub from: Vec<String>,
    pub mailbox: Option<String>,
    pub unread: Option<bool>,
    pub starred: Option<bool>,
    pub has_attachment: bool,
}

pub fn parse_query(input: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery::default();
    let mut free = String::new();

    for token in split_respecting_quotes(input) {
        match token.split_once(':') {
            Some(("from", value)) if !value.is_empty() => parsed.from.push(value.to_lowercase()),
            Some(("in", value)) if !value.is_empty() => {
                parsed.mailbox = Some(value.to_lowercase());
            }
            Some(("is", "unread")) => parsed.unread = Some(true),
            Some(("is", "read")) => parsed.unread = Some(false),
            Some(("is", "starred")) => parsed.starred = Some(true),
            Some(("has", "attachment")) => parsed.has_attachment = true,
            _ => {
                free.push(' ');
                free.push_str(&token);
            }
        }
    }

    parsed.fts = build_match(&free);
    parsed
}

fn split_respecting_quotes(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in input.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn build_match(free: &str) -> Option<String> {
    let mut terms: Vec<String> = Vec::new();
    let mut rest = String::new();

    let mut chars = free.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            let mut phrase = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                phrase.push(c);
            }
            let cleaned = sanitize_tokens(&phrase);
            if !cleaned.is_empty() {
                terms.push(format!("\"{}\"", cleaned.join(" ")));
            }
        } else {
            rest.push(ch);
        }
    }

    for token in sanitize_tokens(&rest) {
        terms.push(format!("\"{token}\"*"));
    }

    terms.truncate(MAX_TOKENS);
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

fn sanitize_tokens(input: &str) -> Vec<String> {
    input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub id: String,
    pub subject: String,
    pub sender_display: String,
    pub sender_address: String,
    pub snippet: String,
    pub excerpt: String,
    pub stored_at: String,
    pub mailbox_state: String,
    pub read: bool,
    pub starred: bool,
    pub attachment_count: i64,
    pub thread_root_id: Option<String>,
}

pub fn search_messages(
    conn: &Connection,
    query: &str,
    limit: Option<usize>,
) -> Result<Vec<SearchHit>, StoreError> {
    let parsed = parse_query(query);
    let limit = limit.unwrap_or(DEFAULT_LIMIT) as i64;

    let mut sql = String::from(
        "SELECT m.id, m.subject, m.sender_display, m.sender_address, m.snippet, \
                m.stored_at, m.mailbox_state, m.read, m.starred, m.attachment_count, m.thread_root_id, ",
    );

    if parsed.fts.is_some() {
        sql.push_str(
            "snippet(message_fts, 4, '<mark>', '</mark>', '…', 24) AS excerpt \
             FROM message_fts JOIN messages m ON m.rowid = message_fts.rowid \
             WHERE message_fts MATCH ?1 AND m.deleted = 0",
        );
    } else {
        sql.push_str("'' AS excerpt FROM messages m WHERE m.deleted = 0");
    }

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(fts) = &parsed.fts {
        params.push(Box::new(fts.clone()));
    }

    if let Some(mailbox) = &parsed.mailbox {
        params.push(Box::new(mailbox.clone()));
        sql.push_str(&format!(" AND m.mailbox_state = ?{}", params.len()));
    }
    if let Some(unread) = parsed.unread {
        sql.push_str(if unread {
            " AND m.read = 0"
        } else {
            " AND m.read = 1"
        });
    }
    if parsed.starred == Some(true) {
        sql.push_str(" AND m.starred = 1");
    }
    if parsed.has_attachment {
        sql.push_str(" AND m.attachment_count > 0");
    }
    for sender in &parsed.from {
        params.push(Box::new(format!("%{sender}%")));
        sql.push_str(&format!(
            " AND (lower(m.sender_address) LIKE ?{n} OR lower(m.sender_display) LIKE ?{n})",
            n = params.len()
        ));
    }

    if parsed.fts.is_some() {
        sql.push_str(" ORDER BY bm25(message_fts, 10.0, 6.0, 3.0, 2.0, 1.0), m.stored_at DESC");
    } else {
        sql.push_str(" ORDER BY m.stored_at DESC");
    }

    params.push(Box::new(limit));
    sql.push_str(&format!(" LIMIT ?{}", params.len()));

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(refs.as_slice(), |row| {
        Ok(SearchHit {
            id: row.get(0)?,
            subject: row.get(1)?,
            sender_display: row.get(2)?,
            sender_address: row.get(3)?,
            snippet: row.get(4)?,
            stored_at: row.get(5)?,
            mailbox_state: row.get(6)?,
            read: row.get::<_, i64>(7)? != 0,
            starred: row.get::<_, i64>(8)? != 0,
            attachment_count: row.get(9)?,
            thread_root_id: row.get(10)?,
            excerpt: row.get(11)?,
        })
    })?;

    let mut out = Vec::new();
    for hit in rows {
        out.push(hit?);
    }
    Ok(out)
}
