mod support;

use base64::Engine as _;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::*;
use thelemail_keystore::*;

const READ_VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/productkeys/product-keys-read-v1.json"
);
const READ_VECTORS_DIGEST: &str =
    "ba71af45ec0ee3e1c18e1d35ce075f1d45b6a079afface8912edb13e8ec0a754";

#[test]
fn read_vectors_from_trust_pass_through_the_c_abi() {
    let raw = std::fs::read(READ_VECTORS).expect("read vectors");
    assert_eq!(hex::encode(Sha256::digest(&raw)), READ_VECTORS_DIGEST);
    let file: Value = serde_json::from_slice(&raw).expect("parse vectors");
    let cases = file["cases"].as_array().expect("cases");
    assert!(!cases.is_empty());
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let out = testkit(json!({"op": "readProductKeysVector", "case": case}));
        match case.get("error").and_then(Value::as_str) {
            Some(code) => assert_eq!(out["error"], code, "{name}"),
            None => assert_eq!(out["result"], case["result"], "{name}"),
        }
    }
}

struct V2Account {
    ks: Keystore,
    account_id: String,
    fixture: Value,
}

fn v2_account() -> V2Account {
    let account_id = new_account_id();
    let opened = testkit(json!({
        "op": "openSession",
        "accountId": account_id,
        "masterKey": true,
        "email": EMAIL,
    }));
    let fixture = testkit(json!({
        "op": "productKeysAccount",
        "accountId": account_id,
        "encryptedPrivateKey": opened["encryptedPrivateKey"],
    }));
    V2Account {
        ks: Keystore::new(),
        account_id,
        fixture,
    }
}

impl V2Account {
    fn read(&self, product_keys: &Value) -> Result<ProductKeysRead, String> {
        let bundles = self.fixture["productBundles"].as_array().unwrap();
        self.ks
            .read_product_keys(
                &self.account_id,
                product_keys,
                &self.fixture["rootDescriptor"],
                bundles,
            )
            .map_err(|err| err.message().to_owned())
    }
}

#[test]
fn a_v2_account_reads_through_the_unlocked_session() {
    let a = v2_account();
    let read = a.read(&a.fixture["productKeys"]).expect("read");
    assert_eq!(read.state_revision, 1);
    assert_eq!(
        read.authority_key_id,
        a.fixture["productKeys"]["authorityKey"]["keyId"]
    );
}

#[test]
fn a_forged_head_mac_is_refused() {
    let a = v2_account();
    let mut forged = a.fixture["productKeys"].clone();
    forged["head"]["mac"] = json!(b64().encode([0u8; 32]));
    assert!(a.read(&forged).is_err());
}

#[test]
fn an_authority_key_the_head_does_not_name_is_refused() {
    let a = v2_account();
    let other = v2_account();
    let mut swapped = a.fixture["productKeys"].clone();
    swapped["authorityKey"] = other.fixture["productKeys"]["authorityKey"].clone();
    assert!(a.read(&swapped).is_err());
}

#[test]
fn a_locked_account_cannot_read() {
    let a = v2_account();
    trust(
        thelemail_trust::Family::Session,
        json!({"op": "lock", "accountId": a.account_id}),
    );
    assert!(a.read(&a.fixture["productKeys"]).is_err());
}
