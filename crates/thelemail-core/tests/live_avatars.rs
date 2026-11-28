use std::time::Duration;

use serde_json::json;
use thelemail_api::ApiConfig;
use thelemail_core::avatars::Lookup;
use thelemail_core::{Core, CoreEvent};
use thelemail_keystore::{OpaqueFinishAuthArgs, OpaqueFinishAuthResponse, OpaqueStartAuthArgs};

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

fn sign_in(core: &Core, rt: &tokio::runtime::Runtime) -> (String, String) {
    let email = std::env::var("THELEMAIL_TEST_EMAIL").expect("THELEMAIL_TEST_EMAIL");
    let password = std::env::var("THELEMAIL_TEST_PASSWORD").expect("THELEMAIL_TEST_PASSWORD");
    let http = reqwest::blocking::Client::builder()
        .cookie_store(true)
        .build()
        .expect("client");
    let ks = core.keystore();
    let start = rt
        .block_on(ks.opaque_start_auth(OpaqueStartAuthArgs {
            password,
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
    let ke3 = match rt.block_on(ks.opaque_finish_auth(OpaqueFinishAuthArgs {
        operation_id: start.operation_id,
        account_id: account_id.clone(),
        ke2: init["ke2"].as_str().expect("ke2").to_owned(),
        recovery: false,
    })) {
        OpaqueFinishAuthResponse::Ok { ke3, .. } => ke3,
        OpaqueFinishAuthResponse::Err { code, .. } => panic!("finish auth: {code}"),
    };
    let complete = post(
        &http,
        "/v1/auth/login/complete",
        json!({
            "challengeId": init["challengeId"],
            "ke3": ke3,
            "enrollPersistentSession": false
        }),
    );
    let grant = complete.get("grant").cloned().unwrap_or(complete);
    let token = grant["accessToken"]
        .as_str()
        .expect("accessToken")
        .to_owned();
    (account_id, token)
}

#[test]
#[ignore = "requires a local backend, a dev account and THELEMAIL_TEST_AVATAR_OF"]
fn a_real_avatar_is_resolved_scaled_and_cached() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let _enter = rt.enter();
    let core = Core::new(ApiConfig {
        api_base: url::Url::parse(&api()).expect("url"),
        submission_base: url::Url::parse("http://localhost:8181").expect("url"),
        web_origin: "http://localhost:5175".to_owned(),
    })
    .expect("core");
    core.start();
    let (account_id, token) = sign_in(&core, &rt);
    let conn = rusqlite::Connection::open_in_memory().expect("db");
    thelemail_store::migrations::migrate(&conn).expect("migrate");
    core.mirror()
        .adopt_connection(&account_id, conn)
        .expect("adopt");
    core.auth().adopt(
        &account_id,
        &token,
        std::time::Duration::from_secs(900),
        None,
    );

    let wanted = std::env::var("THELEMAIL_TEST_AVATAR_OF").expect("THELEMAIL_TEST_AVATAR_OF");
    let bimi = std::env::var("THELEMAIL_TEST_BIMI_DOMAIN").ok();
    let mut events = core.subscribe();
    let settled = rt.block_on(async {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let photo = core.avatars().person(&account_id, &wanted);
            let logo = bimi.as_deref().map(|d| core.avatars().bimi(&account_id, d));
            let done = !matches!(photo, Lookup::Pending) && !matches!(logo, Some(Lookup::Pending));
            if done || tokio::time::Instant::now() > deadline {
                return (photo, logo);
            }
            let _ = tokio::time::timeout(Duration::from_millis(500), async {
                while let Ok(event) = events.recv().await {
                    if matches!(event, CoreEvent::AvatarsChanged { .. }) {
                        break;
                    }
                }
            })
            .await;
        }
    });
    let (photo, logo) = settled;
    let image = photo.image().expect("the account has a photo");
    let decoded = image::load_from_memory_with_format(&image.png, image::ImageFormat::Png)
        .expect("stored as PNG");
    assert!(decoded.width() <= thelemail_core::avatars::EDGE);
    assert_eq!(decoded.width(), decoded.height());
    if let Some(logo) = logo {
        eprintln!("bimi lookup: {:?}", logo.image().map(|i| i.png.len()));
    }
    let conn = core.mirror().take_connection(&account_id).expect("conn");
    let rows: i64 = conn
        .query_row(
            "SELECT count(*) FROM avatar_cache WHERE kind = 'person' AND bytes IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(rows, 1);
}
