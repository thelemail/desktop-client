use serde_json::json;
use thelemail_keystore::*;

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
fn signs_in_against_the_live_backend_and_unlocks_the_vault() {
    let email = std::env::var("THELEMAIL_TEST_EMAIL").expect("THELEMAIL_TEST_EMAIL");
    let password = std::env::var("THELEMAIL_TEST_PASSWORD").expect("THELEMAIL_TEST_PASSWORD");

    let client = reqwest::blocking::Client::builder()
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
        &client,
        "/v1/auth/login/init",
        json!({ "email": email, "ke1": start.ke1 }),
    );
    let account_id = init["accountId"].as_str().expect("accountId").to_owned();
    let ke2_std = init["ke2"].as_str().expect("ke2").to_owned();
    let challenge_id = init["challengeId"]
        .as_str()
        .expect("challengeId")
        .to_owned();

    let finished = rt.block_on(ks.opaque_finish_auth(OpaqueFinishAuthArgs {
        operation_id: start.operation_id.clone(),
        account_id: account_id.clone(),
        ke2: ke2_std,
        recovery: false,
    }));
    let ke3_std = match &finished {
        OpaqueFinishAuthResponse::Ok { ke3, .. } => ke3.clone(),
        OpaqueFinishAuthResponse::Err { code, .. } => panic!("finish auth failed: {code}"),
    };
    let complete = post(
        &client,
        "/v1/auth/login/complete",
        json!({ "challengeId": challenge_id, "ke3": ke3_std, "enrollPersistentSession": false }),
    );
    let grant = complete
        .get("grant")
        .cloned()
        .unwrap_or_else(|| complete.clone());

    let unlocked = ks.opaque_complete_login_unlock(OpaqueCompleteLoginUnlockArgs {
        operation_id: start.operation_id,
        account_id: account_id.clone(),
        encrypted_private_key: grant["encryptedPrivateKey"]
            .as_str()
            .expect("encryptedPrivateKey")
            .to_owned(),
        wrapped_master_key: grant["wrappedMasterKey"]
            .as_str()
            .expect("wrappedMasterKey")
            .to_owned(),
        master_key_id: grant["masterKeyId"]
            .as_str()
            .expect("masterKeyId")
            .to_owned(),
        opaque_params_version: grant["opaqueParamsVersion"].as_i64().unwrap_or(1),
        server_auth_scheme: AuthScheme::OpaqueV1,
    });

    match unlocked {
        OpaqueCompleteLoginUnlockResponse::Ok { .. } => {}
        OpaqueCompleteLoginUnlockResponse::Err { code, .. } => panic!("unlock failed: {code}"),
    }

    let status = ks.status();
    assert_eq!(status.accounts.len(), 1);
    assert!(status.accounts[0].unlocked);
}
