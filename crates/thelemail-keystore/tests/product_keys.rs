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
    let ks = Keystore::new();
    let (account_id, fixture) = open_product_keys_account(&ks);
    V2Account {
        ks,
        account_id,
        fixture,
    }
}

impl V2Account {
    fn load_key_list(&self) {
        let list: AccountKeyList =
            serde_json::from_value(self.fixture["accountKeys"].clone()).expect("key list");
        let loaded = serde_json::to_value(
            self.ks
                .load_account_keys(LoadAccountKeysArgs::from_list(&self.account_id, list)),
        )
        .unwrap();
        assert_eq!(loaded["ok"], true, "{loaded}");
    }

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
            .and_then(|read| match read.refused.clone() {
                Some(reason) => Err(reason),
                None => Ok(read),
            })
    }
}

#[test]
fn a_rotated_account_reads_once_its_key_list_is_loaded() {
    let a = v2_account();
    assert!(a.read(&a.fixture["productKeys"]).is_err());
    a.load_key_list();
    let read = a.read(&a.fixture["productKeys"]).expect("read");
    assert_eq!(read.state_revision, Some(1));
    assert_eq!(
        read.authority_key_id.as_deref(),
        a.fixture["productKeys"]["authorityKey"]["keyId"].as_str()
    );
}

#[test]
fn a_forged_head_mac_is_refused() {
    let a = v2_account();
    a.load_key_list();
    let mut forged = a.fixture["productKeys"].clone();
    forged["head"]["mac"] = json!(b64().encode([0u8; 32]));
    assert!(a.read(&forged).is_err());
}

#[test]
fn an_authority_key_the_head_does_not_name_is_refused() {
    let a = v2_account();
    a.load_key_list();
    let other = v2_account();
    let mut swapped = a.fixture["productKeys"].clone();
    swapped["authorityKey"] = other.fixture["productKeys"]["authorityKey"].clone();
    assert!(a.read(&swapped).is_err());
}

#[test]
fn a_locked_account_is_an_error_not_a_refusal() {
    let a = v2_account();
    a.load_key_list();
    a.ks.lock(&a.account_id);
    let bundles = a.fixture["productBundles"].as_array().unwrap();
    assert!(
        a.ks.read_product_keys(
            &a.account_id,
            &a.fixture["productKeys"],
            &a.fixture["rootDescriptor"],
            bundles,
        )
        .is_err()
    );
}
