use std::time::Duration;

use serde_json::{Value, json};
use thelemail_state::Resource;

use super::settings::{MailSidebar, Privacy, RecentKind};
use super::*;
use crate::stub::{ACCOUNT, Stub, error, json as reply, no_content};

const ME: &str = r#"{"accountId":"0c0c0c0c-0c0c-4c0c-8c0c-0c0c0c0c0c0c","email":"ada@thelemail.test","fullName":"Ada","status":"active","createdAt":"t","keyFingerprint":"ab","keyVersion":1}"#;

async fn core_with(stub: &Stub) -> Arc<Core> {
    let core = Arc::new(Core::with_net(
        thelemail_api::Net::new(stub.config()).expect("net"),
    ));
    core.auth()
        .adopt(ACCOUNT, "t", Duration::from_secs(900), None);
    core
}

fn drain(events: &mut tokio::sync::broadcast::Receiver<CoreEvent>) -> Vec<CoreEvent> {
    std::iter::from_fn(|| events.try_recv().ok()).collect()
}

#[tokio::test]
async fn a_profile_load_is_announced_and_a_failed_reload_keeps_it() {
    let stub = Stub::start(|_, hit| {
        if hit == 1 {
            reply("200 OK", ME)
        } else {
            error("503 Service Unavailable", "service_unavailable")
        }
    })
    .await;
    let core = core_with(&stub).await;
    let mut events = core.subscribe();
    assert_eq!(core.account().profile(ACCOUNT), Resource::NotLoaded);
    load_profile(&core, ACCOUNT).await;
    assert_eq!(
        core.account()
            .profile(ACCOUNT)
            .data()
            .map(|m| m.email.as_str()),
        Some("ada@thelemail.test")
    );
    assert!(matches!(
        drain(&mut events).as_slice(),
        [CoreEvent::ProfileChanged { .. }]
    ));
    load_profile(&core, ACCOUNT).await;
    let profile = core.account().profile(ACCOUNT);
    assert!(profile.data().is_some());
    assert!(matches!(
        profile.error(),
        Some(ApiError::Server { status: 503, .. })
    ));
    core.account().forget(ACCOUNT);
    assert_eq!(core.account().profile(ACCOUNT), Resource::NotLoaded);
}

#[tokio::test]
async fn a_section_write_merges_into_what_the_server_holds() {
    let stub = Stub::start(|s, _| match (s.method.as_str(), s.path.as_str()) {
        ("GET", "/v1/account/settings") => reply(
            "200 OK",
            r#"{"sections":{"privacy":{"stripTrackingParams":true,"legacyFlag":7}}}"#,
        ),
        _ => reply("200 OK", r#"{"section":"privacy","value":{}}"#),
    })
    .await;
    let core = core_with(&stub).await;
    assert_eq!(
        persist_privacy(&core, ACCOUNT, Privacy::default()).await,
        Err(ApiError::Request),
        "writing before the settings are loaded must refuse"
    );
    load_settings(&core, ACCOUNT).await;
    persist_privacy(
        &core,
        ACCOUNT,
        Privacy {
            strip_tracking_params: false,
            share_spam_headers: Some(true),
        },
    )
    .await
    .unwrap();
    let put = stub
        .requests()
        .into_iter()
        .find(|s| s.method == "PUT")
        .unwrap();
    assert_eq!(put.path, "/v1/account/settings/privacy");
    let body: Value = serde_json::from_str(&put.body).unwrap();
    assert_eq!(
        body,
        json!({"stripTrackingParams": false, "shareSpamHeaders": true, "legacyFlag": 7})
    );
    let settings = core.account().settings(ACCOUNT).data().cloned().unwrap();
    assert!(!settings.privacy.strip_tracking_params);
}

#[tokio::test]
async fn sidebar_edits_show_at_once_and_reach_the_server_as_one_write() {
    let stub = Stub::start(|s, _| match s.method.as_str() {
        "GET" => reply("200 OK", r#"{"sections":{}}"#),
        _ => no_content(),
    })
    .await;
    let core = core_with(&stub).await;
    load_settings(&core, ACCOUNT).await;
    for show_more in [true, false, true] {
        persist_mail_sidebar(
            &core,
            ACCOUNT,
            MailSidebar {
                show_more,
                ..MailSidebar::default()
            },
        );
    }
    record_recents(&core, ACCOUNT, RecentKind::Move, &["f1".into()]);
    let shown = core.account().settings(ACCOUNT).data().cloned().unwrap();
    assert!(shown.mail_sidebar.show_more);
    assert_eq!(shown.mail_recents.moves, ["f1"]);
    load_settings(&core, ACCOUNT).await;
    let kept = core.account().settings(ACCOUNT).data().cloned().unwrap();
    assert!(
        kept.mail_sidebar.show_more,
        "a reload must not undo an edit that is still waiting"
    );
    let puts_seen = || stub.requests().iter().filter(|s| s.method == "PUT").count();
    for _ in 0..60 {
        if puts_seen() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(DEBOUNCE).await;
    let puts: Vec<_> = stub
        .requests()
        .into_iter()
        .filter(|s| s.method == "PUT")
        .collect();
    assert_eq!(puts.len(), 2, "{puts:?}");
    let sidebar = puts
        .iter()
        .find(|s| s.path.ends_with("/mail_sidebar"))
        .unwrap();
    let body: Value = serde_json::from_str(&sidebar.body).unwrap();
    assert_eq!(body["showMore"], json!(true));
}

#[tokio::test]
async fn billing_joins_the_subscription_with_its_plan() {
    let stub = Stub::start(|s, _| match s.path.as_str() {
        "/v1/billing/plans" => reply(
            "200 OK",
            r#"{"plans":[{"code":"free_family","workspaceType":"family","displayName":"Free family","pricingModel":"flat","currency":"eur","yearlyAmountCents":0,"monthlyAmountCents":0,"minSeats":1,"maxSeats":6,"storageBytesPerMailbox":5368709120,"customDomainsLimit":0,"auditRetentionDays":0,"prioritySupport":"none"}]}"#,
        ),
        _ => reply(
            "200 OK",
            r#"{"status":"active","entitled":true,"planCode":"free_family","cancelAtPeriodEnd":false}"#,
        ),
    })
    .await;
    let core = core_with(&stub).await;
    let mut events = core.subscribe();
    load_billing(&core, ACCOUNT).await;
    load_billing(&core, ACCOUNT).await;
    assert_eq!(
        stub.hits("/v1/billing/plans"),
        1,
        "the catalog is fetched once"
    );
    let plans = stub
        .requests()
        .into_iter()
        .find(|s| s.path == "/v1/billing/plans")
        .unwrap();
    assert_eq!(plans.header("authorization"), None);
    let billing = core.account().billing(ACCOUNT).data().cloned().unwrap();
    assert!(billing.is_free() && billing.is_free_family());
    assert!(billing.can_add_members() && !billing.can_add_domains());
    assert!(!billing.needs_payment);
    assert_eq!(billing.plan.unwrap().max_seats, Some(6));
    assert!(matches!(
        drain(&mut events).as_slice(),
        [
            CoreEvent::BillingChanged { .. },
            CoreEvent::BillingChanged { .. }
        ]
    ));
}

#[tokio::test]
async fn a_lifecycle_refusal_reloads_the_profile_and_billing() {
    let stub = Stub::start(|s, _| match s.path.as_str() {
        "/v1/me" => reply("200 OK", ME),
        "/v1/billing/plans" => reply("200 OK", r#"{"plans":[]}"#),
        "/v1/billing/subscription" => reply(
            "200 OK",
            r#"{"status":"past_due","entitled":false,"cancelAtPeriodEnd":false}"#,
        ),
        _ => error("403 Forbidden", "read_only"),
    })
    .await;
    let core = core_with(&stub).await;
    core.start();
    let mut events = core.subscribe();
    let err = core
        .api()
        .empty(ACCOUNT, crate::api::Call::post("/v1/messages"))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(&crate::api::ErrorCode::ReadOnly));
    let mut seen = (false, false);
    while seen != (true, true) {
        match tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("events in time")
            .unwrap()
        {
            CoreEvent::ProfileChanged { .. } => seen.0 = true,
            CoreEvent::BillingChanged { .. } => seen.1 = true,
            _ => {}
        }
    }
    assert!(
        core.account()
            .billing(ACCOUNT)
            .data()
            .unwrap()
            .needs_payment
    );
}
