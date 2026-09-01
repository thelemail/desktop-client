use base64::Engine as _;
use serde::Deserialize;
use std::process::Command;
use thelemail_crypto::opaque::{finish_login, start_login};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");
const HARNESS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/js/opaque-server.mjs"
);

#[derive(Deserialize)]
struct Setup {
    #[serde(rename = "exportKey")]
    export_key: String,
    password: String,
    #[serde(rename = "clientIdentity")]
    client_identity: String,
    #[serde(rename = "serverIdentity")]
    server_identity: String,
}

#[derive(Deserialize)]
struct LoginResponse {
    ke2: String,
    #[serde(rename = "serverLoginState")]
    server_login_state: String,
}

#[derive(Deserialize)]
struct VerifyResponse {
    #[serde(rename = "sessionKey")]
    session_key: String,
}

fn b64url() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
}

fn setup() -> Setup {
    let raw = std::fs::read_to_string(format!("{FIXTURES}/opaque/setup.json")).expect("read setup");
    serde_json::from_str(&raw).expect("parse setup")
}

fn node(args: &[&str]) -> String {
    let out = Command::new("node")
        .arg(HARNESS)
        .args(args)
        .output()
        .expect("run opaque harness");
    assert!(
        out.status.success(),
        "harness failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("harness stdout")
}

#[test]
fn rust_client_authenticates_against_the_web_clients_opaque_server() {
    let s = setup();

    let start = start_login(&s.password).expect("start login");
    let ke1 = b64url().encode(&start.ke1);

    let login: LoginResponse =
        serde_json::from_str(&node(&["login", &ke1])).expect("parse login response");
    let ke2 = b64url().decode(&login.ke2).expect("decode ke2");

    let finished = finish_login(
        start.state,
        &s.password,
        &ke2,
        &s.client_identity,
        &s.server_identity,
    )
    .expect("finish login");

    assert_eq!(
        b64url().encode(&finished.export_key),
        s.export_key,
        "export key must match the one @serenity-kit/opaque derived at registration"
    );

    let payload = serde_json::json!({
        "serverLoginState": login.server_login_state,
        "ke3": b64url().encode(&finished.ke3),
    })
    .to_string();
    let verify: VerifyResponse =
        serde_json::from_str(&node(&["verify", &payload])).expect("parse verify response");

    assert_eq!(
        b64url().encode(&finished.session_key),
        verify.session_key,
        "session key must match the server's"
    );
}

#[test]
fn a_wrong_password_does_not_authenticate() {
    let s = setup();
    let start = start_login("not the password").expect("start login");
    let ke1 = b64url().encode(&start.ke1);
    let login: LoginResponse =
        serde_json::from_str(&node(&["login", &ke1])).expect("parse login response");
    let ke2 = b64url().decode(&login.ke2).expect("decode ke2");

    let result = finish_login(
        start.state,
        "not the password",
        &ke2,
        &s.client_identity,
        &s.server_identity,
    );
    assert!(result.is_err(), "a wrong password must fail the login");
}
