use std::collections::HashMap;
use thelemail_api::{ApiConfig, ApiRequest, Net};

fn net() -> Net {
    unsafe {
        std::env::set_var("THELEMAIL_DESKTOP_API_BASE_URL", "http://localhost:8180");
        std::env::set_var(
            "THELEMAIL_DESKTOP_SUBMISSION_BASE_URL",
            "http://localhost:8181",
        );
        std::env::set_var("THELEMAIL_DESKTOP_BLOB_ORIGIN", "http://127.0.0.1:9002");
        std::env::set_var("THELEMAIL_DESKTOP_WEB_ORIGIN", "http://localhost:5175");
    }
    Net::new(ApiConfig::from_env().expect("config")).expect("net")
}

fn get(url: &str) -> ApiRequest {
    ApiRequest {
        url: url.to_owned(),
        method: "GET".to_owned(),
        headers: HashMap::new(),
        body: None,
    }
}

fn post(url: &str) -> ApiRequest {
    ApiRequest {
        url: url.to_owned(),
        method: "POST".to_owned(),
        headers: HashMap::new(),
        body: None,
    }
}

#[tokio::test]
#[ignore = "requires a local backend"]
async fn reaches_an_unguarded_endpoint() {
    let resp = net()
        .request(get("http://localhost:8180/v1/auth/opaque-parameters"))
        .await
        .expect("request");
    assert_eq!(resp.status, 200);
}

#[tokio::test]
#[ignore = "requires a local backend"]
async fn passes_the_same_origin_guard() {
    let resp = net()
        .request(post("http://localhost:8180/v1/auth/refresh"))
        .await
        .expect("request");
    assert_ne!(
        resp.status, 403,
        "RequireSameOrigin rejected the request: the synthesized Origin is not allow-listed"
    );
}

#[tokio::test]
#[ignore = "requires a local backend"]
async fn refuses_a_host_outside_the_allow_list() {
    let result = net()
        .request(post("http://example.com/v1/auth/refresh"))
        .await;
    assert!(result.is_err(), "transport must refuse unlisted hosts");
}

#[tokio::test]
#[ignore = "requires a local backend"]
async fn never_returns_set_cookie_to_the_webview() {
    let resp = net()
        .request(post("http://localhost:8180/v1/auth/refresh"))
        .await
        .expect("request");
    assert!(
        !resp
            .headers
            .keys()
            .any(|k| k.eq_ignore_ascii_case("set-cookie")),
        "the refresh cookie must never reach the webview"
    );
}

#[tokio::test]
async fn the_generic_command_refuses_the_submission_origin() {
    let result = net()
        .request(post("http://localhost:8181/v1/messages"))
        .await;
    assert!(
        result.is_err(),
        "api_request must not reach the submission origin: that is an outbound mail channel"
    );
}

#[tokio::test]
async fn the_submission_command_refuses_the_api_origin() {
    let result = net()
        .submit(post("http://localhost:8180/v1/messages"))
        .await;
    assert!(
        result.is_err(),
        "the submission command must not be usable as a general API client"
    );
}

#[tokio::test]
async fn neither_command_reaches_an_unrelated_host() {
    assert!(net().request(post("https://evil.example/x")).await.is_err());
    assert!(net().submit(post("https://evil.example/x")).await.is_err());
    assert!(net().blob_get("https://evil.example/x").await.is_err());
}
