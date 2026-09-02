use serde_json::json;

fn api() -> String {
    std::env::var("THELEMAIL_TEST_API").unwrap_or_else(|_| "http://localhost:8180".to_owned())
}

fn post(
    client: &reqwest::blocking::Client,
    path: &str,
    body: serde_json::Value,
) -> serde_json::Value {
    let resp = client
        .post(format!("{}{path}", api()))
        .header("Origin", "http://localhost:5175")
        .json(&body)
        .send()
        .expect("request");
    let status = resp.status();
    let text = resp.text().expect("body");
    assert!(status.is_success(), "{path} -> {status}: {text}");
    serde_json::from_str(&text).unwrap_or(serde_json::Value::Null)
}

#[test]
#[ignore = "requires a local backend and the dev test account"]
fn backfills_a_real_mailbox_and_searches_it_offline() {
    use thelemail_api::{ApiConfig, Net};
    use thelemail_keystore::*;

    let email = std::env::var("THELEMAIL_TEST_EMAIL").expect("THELEMAIL_TEST_EMAIL");
    let password = std::env::var("THELEMAIL_TEST_PASSWORD").expect("THELEMAIL_TEST_PASSWORD");

    let http = reqwest::blocking::Client::builder()
        .cookie_store(true)
        .build()
        .expect("client");
    let ks = Keystore::new();
    let rt = tokio::runtime::Runtime::new().expect("runtime");

    let start = rt
        .block_on(ks.opaque_start_auth(OpaqueStartAuthArgs {
            password: password.clone(),
            email: Some(email.clone()),
            recovery: false,
        }))
        .expect("start auth");

    let init = post(
        &http,
        "/v1/auth/login/init",
        json!({ "email": email, "ke1": start.ke1 }),
    );
    let account_id = init["accountId"].as_str().expect("accountId").to_owned();

    let finished = rt.block_on(ks.opaque_finish_auth(OpaqueFinishAuthArgs {
        operation_id: start.operation_id.clone(),
        account_id: account_id.clone(),
        ke2: init["ke2"].as_str().expect("ke2").to_owned(),
        recovery: false,
    }));
    let ke3 = match &finished {
        OpaqueFinishAuthResponse::Ok { ke3, .. } => ke3.clone(),
        OpaqueFinishAuthResponse::Err { code, .. } => panic!("finish auth: {code}"),
    };

    let complete = post(
        &http,
        "/v1/auth/login/complete",
        json!({
            "challengeId": init["challengeId"].as_str().expect("challengeId"),
            "ke3": ke3,
            "enrollPersistentSession": false
        }),
    );
    let grant = complete.get("grant").cloned().unwrap_or(complete.clone());
    let access_token = grant["accessToken"]
        .as_str()
        .expect("accessToken")
        .to_owned();

    let unlocked = ks.opaque_complete_login_unlock(OpaqueCompleteLoginUnlockArgs {
        operation_id: start.operation_id,
        account_id: account_id.clone(),
        encrypted_private_key: grant["encryptedPrivateKey"]
            .as_str()
            .expect("key")
            .to_owned(),
        wrapped_master_key: grant["wrappedMasterKey"].as_str().expect("wmk").to_owned(),
        master_key_id: grant["masterKeyId"].as_str().expect("mkid").to_owned(),
        opaque_params_version: grant["opaqueParamsVersion"].as_i64().unwrap_or(1),
        server_auth_scheme: AuthScheme::OpaqueV1,
    });
    assert!(
        matches!(unlocked, OpaqueCompleteLoginUnlockResponse::Ok { .. }),
        "vault must unlock"
    );

    unsafe {
        std::env::set_var("THELEMAIL_DESKTOP_API_BASE_URL", api());
        std::env::set_var(
            "THELEMAIL_DESKTOP_SUBMISSION_BASE_URL",
            "http://localhost:8181",
        );
        std::env::set_var("THELEMAIL_DESKTOP_BLOB_ORIGIN", "http://127.0.0.1:9002");
        std::env::set_var("THELEMAIL_DESKTOP_WEB_ORIGIN", "http://localhost:5175");
    }
    let net = Net::new(ApiConfig::from_env().expect("config")).expect("net");

    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("offline.db");
    let key = thelemail_store::generate_db_key();
    let conn = thelemail_store::open_account_db(&db_path, &key, &account_id).expect("open mirror");

    let url = format!(
        "{}v1/messages?mailbox=inbox&sort=oldest&limit=100",
        net.config().api_base
    );
    let mut headers = std::collections::HashMap::new();
    headers.insert("Authorization".to_owned(), format!("Bearer {access_token}"));
    headers.insert("X-Account-Id".to_owned(), account_id.clone());

    let resp = rt
        .block_on(net.request(thelemail_api::ApiRequest {
            url,
            method: "GET".to_owned(),
            headers,
            body: None,
        }))
        .expect("list messages");
    assert_eq!(resp.status, 200, "listing the mailbox must succeed");

    let page: thelemail_desktop_lib::mirror::MessageListResponse =
        serde_json::from_slice(&resp.body.expect("body")).expect("parse page");
    assert!(
        !page.items.is_empty(),
        "the test mailbox has no messages to mirror"
    );

    thelemail_desktop_lib::mirror::apply_page(
        &conn,
        &ks,
        &account_id,
        "inbox",
        &page.items,
        &page.next_cursor,
        i64::MAX,
    )
    .expect("apply page");

    let stored: i64 = conn
        .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
        .expect("count");
    assert_eq!(
        stored as usize,
        page.items.len(),
        "every row must be mirrored"
    );

    let decrypted: i64 = conn
        .query_row(
            "SELECT count(*) FROM messages WHERE preview_state = 'ok' AND subject <> ''",
            [],
            |r| r.get(0),
        )
        .expect("count decrypted");
    assert!(
        decrypted > 0,
        "no preview decrypted: the mirror stored rows it could not read"
    );

    let subject: String = conn
        .query_row(
            "SELECT subject FROM messages WHERE preview_state = 'ok' AND subject <> '' LIMIT 1",
            [],
            |r| r.get(0),
        )
        .expect("a subject");
    let term = subject
        .split_whitespace()
        .find(|w| w.chars().all(|c| c.is_alphanumeric()) && w.len() > 3)
        .expect("a searchable word in the subject")
        .to_lowercase();

    let hits = thelemail_store::search::search_messages(&conn, &term, None).expect("search");
    assert!(
        !hits.is_empty(),
        "searching {term:?} found nothing, but it came from a mirrored subject"
    );

    let token = access_token.clone();
    let mirror = thelemail_desktop_lib::mirror::Mirror::default();
    mirror
        .adopt_connection(&account_id, conn)
        .expect("adopt connection");

    let fetched = rt
        .block_on(thelemail_desktop_lib::mirror::prefetch_bodies(
            &net,
            &ks,
            &mirror,
            &account_id,
            &token,
            |_, _| {},
        ))
        .expect("prefetch bodies");
    assert!(fetched > 0, "no bodies were prefetched");

    let indexed_rows: Vec<(String, String, String)> = mirror
        .with_conn(&account_id, |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT m.id, sd.body, m.subject || ' ' || m.snippet \
                     FROM search_docs sd JOIN messages m ON m.rowid = sd.rowid \
                     WHERE sd.body <> ''",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(|e| e.to_string())?);
            }
            Ok(out)
        })
        .expect("read index");

    assert!(
        !indexed_rows.is_empty(),
        "prefetch stored no searchable body text"
    );
    println!(
        "prefetched {fetched} bodies, {} indexed",
        indexed_rows.len()
    );

    let probe = indexed_rows.iter().find_map(|(id, body, preview)| {
        let preview = preview.to_lowercase();
        body.split_whitespace()
            .map(|w| {
                w.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase()
            })
            .find(|w| {
                w.len() > 5
                    && w.chars().all(|c| c.is_ascii_alphabetic())
                    && !preview.contains(w.as_str())
            })
            .map(|word| (id.clone(), word))
    });

    let (message_id, word) = probe.expect(
        "no mirrored message has a body word outside its own subject and snippet: \
         the body-search assertion would be vacuous",
    );
    println!("  probing {word:?}, expected in message {message_id}");

    let hits = mirror
        .search(&account_id, &word, None)
        .expect("search body");
    assert!(
        hits.iter().any(|h| h.id == message_id),
        "searching {word:?} did not return {message_id}, but that word appears only in its body"
    );
    let hit = hits.iter().find(|h| h.id == message_id).expect("hit");
    assert!(
        !hit.excerpt.is_empty(),
        "the excerpt is generated from the body column and must not be empty"
    );

    let conn = mirror
        .take_connection(&account_id)
        .expect("reclaim connection");
    drop(conn);
    let raw = std::fs::read(&db_path).expect("read db");
    assert!(!raw.starts_with(b"SQLite format 3"), "mirror is plaintext");
    assert!(
        !String::from_utf8_lossy(&raw).contains(&subject),
        "the subject is readable in the mirror file"
    );
}
