use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use thelemail_api::ApiConfig;
use thelemail_core::account::settings::{Composing, ReplyDefault};
use thelemail_core::api::Call;
use thelemail_core::{Core, CoreEvent, account, realtime};
use thelemail_keystore::{OpaqueFinishAuthArgs, OpaqueFinishAuthResponse, OpaqueStartAuthArgs};

fn core() -> Arc<Core> {
    let api =
        std::env::var("THELEMAIL_TEST_API").unwrap_or_else(|_| "http://localhost:8180".to_owned());
    let core = Core::new(ApiConfig {
        api_base: url::Url::parse(&api).expect("url"),
        submission_base: url::Url::parse("http://localhost:8181").expect("url"),
        web_origin: "http://localhost:5175".to_owned(),
    })
    .expect("core");
    core.start();
    core
}

async fn sign_in(core: &Core) -> String {
    let email = std::env::var("THELEMAIL_TEST_EMAIL").expect("THELEMAIL_TEST_EMAIL");
    let password = std::env::var("THELEMAIL_TEST_PASSWORD").expect("THELEMAIL_TEST_PASSWORD");
    let ks = core.keystore();
    let start = ks
        .opaque_start_auth(OpaqueStartAuthArgs {
            password,
            email: Some(email.clone()),
            recovery: false,
        })
        .await
        .expect("start auth");
    let init: Value = core
        .api()
        .public(
            Call::post("/v1/auth/login/init").json(&json!({ "email": email, "ke1": start.ke1 })),
        )
        .await
        .expect("login init");
    let account_id = init["accountId"].as_str().expect("accountId").to_owned();
    let ke3 = match ks
        .opaque_finish_auth(OpaqueFinishAuthArgs {
            operation_id: start.operation_id,
            account_id: account_id.clone(),
            ke2: init["ke2"].as_str().expect("ke2").to_owned(),
            recovery: false,
        })
        .await
    {
        OpaqueFinishAuthResponse::Ok { ke3, .. } => ke3,
        OpaqueFinishAuthResponse::Err { code, .. } => panic!("finish auth: {code}"),
    };
    let complete: Value = core
        .api()
        .public(Call::post("/v1/auth/login/complete").json(&json!({
            "challengeId": init["challengeId"],
            "ke3": ke3,
            "enrollPersistentSession": false
        })))
        .await
        .expect("login complete");
    let grant = complete.get("grant").cloned().unwrap_or(complete);
    core.auth().adopt(
        &account_id,
        grant["accessToken"].as_str().expect("accessToken"),
        Duration::from_secs(grant["expiresInSeconds"].as_u64().unwrap_or(60)),
        grant["csrfToken"].as_str(),
    );
    account_id
}

async fn until(
    events: &mut tokio::sync::broadcast::Receiver<CoreEvent>,
    what: impl Fn(&CoreEvent) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if what(&events.recv().await.expect("events")) {
                return;
            }
        }
    })
    .await
    .expect("the event in time");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a local backend and a dev account"]
async fn resources_refresh_and_hints_against_the_local_stack() {
    let desktop = core();
    let account_id = sign_in(&desktop).await;

    account::load_profile(&desktop, &account_id).await;
    account::load_settings(&desktop, &account_id).await;
    account::load_billing(&desktop, &account_id).await;
    let profile = desktop.account().profile(&account_id);
    assert_eq!(
        profile.data().map(|m| m.account_id.as_str()),
        Some(account_id.as_str()),
        "{profile:?}"
    );
    let settings = desktop.account().settings(&account_id);
    assert!(settings.is_loaded(), "{settings:?}");
    let billing = desktop.account().billing(&account_id);
    assert!(billing.is_loaded(), "{billing:?}");
    assert!(desktop.account().catalog().is_loaded());

    desktop
        .auth()
        .adopt(&account_id, "not-a-token", Duration::from_secs(900), None);
    let mut events = desktop.subscribe();
    account::load_profile(&desktop, &account_id).await;
    let reloaded = desktop.account().profile(&account_id);
    assert!(
        reloaded.error().is_none(),
        "a rejected token must refresh through the saved cookie: {reloaded:?}"
    );
    assert!(!desktop.auth().is_lost(&account_id));
    until(&mut events, |e| {
        matches!(e, CoreEvent::ProfileChanged { .. })
    })
    .await;

    realtime::open(&desktop, &account_id).unwrap();
    until(&mut events, |e| {
        matches!(e, CoreEvent::RealtimeState { open: true, .. })
    })
    .await;

    let web = core();
    assert_eq!(sign_in(&web).await, account_id);
    account::load_settings(&web, &account_id).await;
    let before = web
        .account()
        .settings(&account_id)
        .data()
        .map(|s| s.composing)
        .expect("settings on the second device");
    let flipped = Composing {
        reply_default: match before.reply_default {
            ReplyDefault::Reply => ReplyDefault::All,
            ReplyDefault::All => ReplyDefault::Reply,
        },
        ..before
    };
    account::persist_composing(&web, &account_id, flipped)
        .await
        .expect("write from the second device");
    until(&mut events, |e| {
        matches!(e, CoreEvent::Hint(h) if h.kind.is(realtime::Entity::Settings, realtime::Action::Updated))
    })
    .await;
    until(&mut events, |e| {
        matches!(e, CoreEvent::SettingsChanged { .. })
    })
    .await;
    let seen = desktop
        .account()
        .settings(&account_id)
        .data()
        .map(|s| s.composing.reply_default);
    assert_eq!(seen, Some(flipped.reply_default));

    account::persist_composing(&web, &account_id, before)
        .await
        .expect("restore the setting");
    desktop.realtime().close(&account_id);
}
