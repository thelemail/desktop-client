use std::time::Duration;

use thelemail_api::Net;
use tokio::sync::broadcast;

use super::*;
use crate::CoreEvent;
use crate::stub::{ACCOUNT, Stub, error, json, no_content, refreshed, with_headers};

struct Rig {
    stub: Stub,
    net: Net,
    auth: Auth,
    events: broadcast::Receiver<CoreEvent>,
}

impl Rig {
    async fn new(
        route: impl Fn(&crate::stub::Seen, usize) -> String + Send + Sync + 'static,
    ) -> Self {
        let stub = Stub::start(route).await;
        let net = Net::new(stub.config()).expect("net");
        let (tx, events) = broadcast::channel(16);
        Self {
            stub,
            net,
            auth: Auth::new(tx),
            events,
        }
    }

    fn api(&self) -> Api<'_> {
        Api::new(&self.net, &self.auth)
    }

    fn signed_in(self, token: &str) -> Self {
        self.auth
            .adopt(ACCOUNT, token, Duration::from_secs(900), None);
        self
    }
}

fn bearer(seen: &crate::stub::Seen) -> &str {
    seen.header("authorization").unwrap_or_default()
}

#[tokio::test]
async fn every_call_carries_the_desktop_binding_and_a_stable_device() {
    let rig = Rig::new(|_, _| json("200 OK", "{}")).await.signed_in("t1");
    let call = || Call::get("/v1/me").header("X-Client", "web");
    rig.api().empty(ACCOUNT, call()).await.unwrap();
    rig.api().empty(ACCOUNT, call()).await.unwrap();
    let seen = rig.stub.requests();
    assert_eq!(seen.len(), 2);
    for s in &seen {
        assert_eq!(s.header("x-client"), Some("desktop"));
        assert_eq!(s.header("x-account-id"), Some(ACCOUNT));
        assert_eq!(bearer(s), "Bearer t1");
    }
    let device = seen[0].header("x-device-id").unwrap();
    assert_eq!(uuid::Uuid::parse_str(device).map(|_| ()), Ok(()));
    assert_eq!(seen[1].header("x-device-id"), Some(device));
}

#[tokio::test]
async fn an_account_without_a_token_refreshes_before_its_first_call() {
    let rig = Rig::new(|s, _| match s.path.as_str() {
        "/v1/auth/refresh" => refreshed("fresh"),
        _ => json("200 OK", "{}"),
    })
    .await;
    rig.api().empty(ACCOUNT, Call::get("/v1/me")).await.unwrap();
    let seen = rig.stub.requests();
    assert_eq!(seen[0].path, "/v1/auth/refresh");
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].header("authorization"), None);
    assert_eq!(seen[0].header("x-refresh-rotation"), Some("1"));
    assert_eq!(seen[0].header("x-account-id"), Some(ACCOUNT));
    assert_eq!(bearer(&seen[1]), "Bearer fresh");
}

#[tokio::test]
async fn a_token_inside_the_skew_is_renewed_first() {
    let rig = Rig::new(|s, _| match s.path.as_str() {
        "/v1/auth/refresh" => refreshed("next"),
        _ => json("200 OK", "{}"),
    })
    .await;
    rig.auth
        .adopt(ACCOUNT, "old", REFRESH_SKEW - Duration::from_secs(1), None);
    rig.api().empty(ACCOUNT, Call::get("/v1/me")).await.unwrap();
    assert_eq!(rig.stub.hits("/v1/auth/refresh"), 1);
    assert_eq!(bearer(&rig.stub.requests()[1]), "Bearer next");
}

#[tokio::test]
async fn concurrent_unauthorized_calls_share_one_refresh() {
    let rig = Rig::new(|s, _| match s.path.as_str() {
        "/v1/auth/refresh" => refreshed("new"),
        _ if bearer(s) == "Bearer new" => json("200 OK", "{}"),
        _ => error("401 Unauthorized", "invalid_credentials"),
    })
    .await
    .signed_in("old");
    let api = rig.api();
    let calls = (0..6).map(|i| api.empty(ACCOUNT, Call::get(format!("/v1/item/{i}"))));
    let results = futures_util::future::join_all(calls).await;
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    assert_eq!(rig.stub.hits("/v1/auth/refresh"), 1);
}

#[tokio::test]
async fn a_refused_refresh_loses_the_session_and_stops_calling() {
    let mut rig = Rig::new(|s, _| match s.path.as_str() {
        "/v1/auth/refresh" => error("401 Unauthorized", "invalid_credentials"),
        _ => error("401 Unauthorized", "invalid_credentials"),
    })
    .await
    .signed_in("old");
    let err = rig
        .api()
        .empty(ACCOUNT, Call::get("/v1/me"))
        .await
        .unwrap_err();
    assert_eq!(err, ApiError::SessionLost);
    assert!(rig.auth.is_lost(ACCOUNT));
    assert!(matches!(
        rig.events.try_recv(),
        Ok(CoreEvent::SessionLost { ref account_id }) if account_id == ACCOUNT
    ));
    let before = rig.stub.requests().len();
    let again = rig
        .api()
        .empty(ACCOUNT, Call::get("/v1/me"))
        .await
        .unwrap_err();
    assert_eq!(again, ApiError::SessionLost);
    assert_eq!(rig.stub.requests().len(), before);
    rig.auth
        .adopt(ACCOUNT, "back", Duration::from_secs(900), None);
    assert!(!rig.auth.is_lost(ACCOUNT));
}

#[tokio::test]
async fn a_network_failure_during_refresh_is_not_a_lost_session() {
    let rig = Rig::new(|s, _| match s.path.as_str() {
        "/v1/auth/refresh" => error("503 Service Unavailable", "service_unavailable"),
        _ => error("401 Unauthorized", "invalid_credentials"),
    })
    .await
    .signed_in("old");
    let err = rig
        .api()
        .empty(ACCOUNT, Call::get("/v1/me"))
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Unauthorized(_)), "{err:?}");
    assert!(!rig.auth.is_lost(ACCOUNT));
}

#[tokio::test]
async fn a_credential_check_is_never_refreshed_and_replayed() {
    let rig = Rig::new(|s, _| match s.path.as_str() {
        "/v1/auth/refresh" => refreshed("new"),
        _ => error("401 Unauthorized", "invalid_credentials"),
    })
    .await
    .signed_in("old");
    let call = Call::post("/v1/2fa/step-up/opaque/confirm")
        .json(&serde_json::json!({"ke3": "x"}))
        .credential_check();
    let err = rig.api().empty(ACCOUNT, call).await.unwrap_err();
    assert_eq!(err.code(), Some(&ErrorCode::InvalidCredentials));
    assert_eq!(rig.stub.hits("/v1/2fa/step-up/opaque/confirm"), 1);
    assert_eq!(rig.stub.hits("/v1/auth/refresh"), 0);
}

#[tokio::test]
async fn a_call_still_refused_after_refresh_is_not_retried_again() {
    let rig = Rig::new(|s, _| match s.path.as_str() {
        "/v1/auth/refresh" => refreshed("new"),
        _ => error("401 Unauthorized", "invalid_credentials"),
    })
    .await
    .signed_in("old");
    let err = rig
        .api()
        .empty(ACCOUNT, Call::get("/v1/me"))
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Unauthorized(_)));
    assert_eq!(rig.stub.hits("/v1/me"), 2);
    assert_eq!(rig.stub.hits("/v1/auth/refresh"), 1);
}

#[tokio::test]
async fn a_step_up_grant_is_minted_once_and_bound_to_the_request() {
    let rig = Rig::new(|s, _| match s.path.as_str() {
        "/v1/auth/step-up/grants" => json("200 OK", r#"{"grant":"g-1"}"#),
        _ if s.header("x-step-up-grant") == Some("g-1") => no_content(),
        _ => error("403 Forbidden", "step_up_grant_required"),
    })
    .await
    .signed_in("t");
    let body = serde_json::json!({"enabled": true});
    rig.api()
        .empty(ACCOUNT, Call::put("/v1/account/thing?x=1").json(&body))
        .await
        .unwrap();
    let seen = rig.stub.requests();
    let grant = seen
        .iter()
        .find(|s| s.path == "/v1/auth/step-up/grants")
        .unwrap();
    let asked: serde_json::Value = serde_json::from_str(&grant.body).unwrap();
    use base64::Engine;
    use sha2::Digest;
    let digest = base64::engine::general_purpose::STANDARD
        .encode(sha2::Sha256::digest(serde_json::to_string(&body).unwrap()));
    assert_eq!(
        asked,
        serde_json::json!({"method": "PUT", "path": "/v1/account/thing", "bodyDigest": digest})
    );
}

#[tokio::test]
async fn a_refused_granted_retry_does_not_ask_twice() {
    let rig = Rig::new(|s, _| match s.path.as_str() {
        "/v1/auth/step-up/grants" => json("200 OK", r#"{"grant":"g"}"#),
        _ => error("403 Forbidden", "step_up_grant_required"),
    })
    .await
    .signed_in("t");
    let err = rig
        .api()
        .empty(ACCOUNT, Call::delete("/v1/thing"))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(&ErrorCode::StepUpGrantRequired));
    assert_eq!(rig.stub.hits("/v1/auth/step-up/grants"), 1);
    assert_eq!(rig.stub.hits("/v1/thing"), 2);
}

#[tokio::test]
async fn lifecycle_refusals_ask_for_a_profile_reload() {
    let rig = Rig::new(|_, _| error("403 Forbidden", "read_only"))
        .await
        .signed_in("t");
    let mut lifecycle = rig.auth.take_lifecycle().unwrap();
    let err = rig
        .api()
        .empty(ACCOUNT, Call::post("/v1/x"))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(&ErrorCode::ReadOnly));
    assert_eq!(lifecycle.try_recv().ok().as_deref(), Some(ACCOUNT));
}

#[tokio::test]
async fn rate_limits_carry_the_retry_after_header() {
    let rig = Rig::new(|_, _| {
        with_headers(
            "429 Too Many Requests",
            "Retry-After: 9",
            r#"{"error":{"code":"rate_limited","message":"slow"}}"#,
        )
    })
    .await
    .signed_in("t");
    let err = rig
        .api()
        .empty(ACCOUNT, Call::get("/v1/x"))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        ApiError::RateLimited {
            retry_after: Some(9),
            ..
        }
    ));
}

#[tokio::test]
async fn csrf_goes_only_where_asked_once_a_refresh_supplied_it() {
    let rig = Rig::new(|s, _| match s.path.as_str() {
        "/v1/auth/refresh" => refreshed("t"),
        _ => no_content(),
    })
    .await;
    rig.api().empty(ACCOUNT, Call::get("/v1/a")).await.unwrap();
    rig.api()
        .empty(ACCOUNT, Call::post("/v1/auth/logout").anonymous().csrf())
        .await
        .unwrap();
    let seen = rig.stub.requests();
    assert_eq!(seen[1].header("x-csrf-token"), None);
    assert_eq!(seen[2].header("x-csrf-token"), Some("csrf-t"));
    assert_eq!(seen[2].header("authorization"), None);
}

#[tokio::test]
async fn paths_must_stay_on_the_api_origin() {
    let rig = Rig::new(|_, _| no_content()).await.signed_in("t");
    for path in ["v1/me", "//evil.test/v1/me", "https://evil.test/v1/me"] {
        let err = rig.api().empty(ACCOUNT, Call::get(path)).await.unwrap_err();
        assert_eq!(err, ApiError::Request, "{path}");
    }
    let err = rig
        .api()
        .empty("not-an-id", Call::get("/v1/me"))
        .await
        .unwrap_err();
    assert_eq!(err, ApiError::Request);
    assert!(rig.stub.requests().is_empty());
}

#[tokio::test]
async fn a_public_call_names_no_account_and_sends_no_token() {
    let rig = Rig::new(|_, _| json("200 OK", r#"{"challengeId":"c"}"#))
        .await
        .signed_in("t");
    let v: serde_json::Value = rig
        .api()
        .public(Call::post("/v1/auth/login/init").json(&serde_json::json!({"email": "a"})))
        .await
        .unwrap();
    assert_eq!(v["challengeId"], "c");
    let seen = &rig.stub.requests()[0];
    assert_eq!(seen.header("x-account-id"), None);
    assert_eq!(seen.header("authorization"), None);
    assert_eq!(seen.header("x-client"), Some("desktop"));
    assert!(seen.header("x-device-id").is_some());
}

#[tokio::test]
async fn signing_in_again_wakes_whoever_waits_on_a_lost_session() {
    let rig = Rig::new(|_, _| error("401 Unauthorized", "invalid_credentials")).await;
    let revived = rig.auth.revived(ACCOUNT);
    assert_eq!(
        rig.api().empty(ACCOUNT, Call::get("/v1/me")).await,
        Err(ApiError::SessionLost)
    );
    let waiter = tokio::spawn(async move { revived.notified().await });
    tokio::task::yield_now().await;
    rig.auth.adopt(ACCOUNT, "t", Duration::from_secs(900), None);
    tokio::time::timeout(Duration::from_secs(2), waiter)
        .await
        .expect("woken in time")
        .unwrap();
}
