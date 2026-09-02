use serde::Deserialize;
use thelemail_crypto::amk::{
    Amk, WRAPPED_MASTER_KEY_LEN, derive_master_key_id, derive_pgp_passphrase, unwrap_master_key,
    wrap_master_key,
};

#[derive(Deserialize)]
struct Fixture {
    vectors: Vec<Vector>,
}

#[derive(Deserialize)]
struct Vector {
    #[serde(rename = "exportKey")]
    export_key: String,
    amk: String,
    recovery: bool,
    wrapped: String,
    #[serde(rename = "masterKeyId")]
    master_key_id: String,
    #[serde(rename = "pgpPassphrase")]
    pgp_passphrase: String,
}

fn load() -> Vec<Vector> {
    let raw = include_str!("../../../fixtures/amk/vectors.json");
    let fixture: Fixture = serde_json::from_str(raw).expect("parse amk fixture");
    assert!(!fixture.vectors.is_empty(), "fixture must not be empty");
    fixture.vectors
}

fn amk_of(hex_str: &str) -> Amk {
    let bytes = hex::decode(hex_str).expect("hex amk");
    Amk::from_bytes(bytes.try_into().expect("32-byte amk"))
}

#[test]
fn unwraps_blobs_produced_by_the_web_client() {
    for v in load() {
        let export_key = hex::decode(&v.export_key).expect("hex export key");
        let wrapped = hex::decode(&v.wrapped).expect("hex wrapped");
        assert_eq!(wrapped.len(), WRAPPED_MASTER_KEY_LEN);

        let got = unwrap_master_key(&export_key, &wrapped, v.recovery).expect("unwrap");
        assert_eq!(hex::encode(got.expose()), v.amk);
    }
}

#[test]
fn derivations_match_the_web_client() {
    for v in load() {
        let amk = amk_of(&v.amk);
        assert_eq!(hex::encode(derive_master_key_id(&amk)), v.master_key_id);
        assert_eq!(derive_pgp_passphrase(&amk).expose(), v.pgp_passphrase);
    }
}

#[test]
fn rust_wrapped_blobs_unwrap_in_rust() {
    for v in load() {
        let export_key = hex::decode(&v.export_key).expect("hex export key");
        let amk = amk_of(&v.amk);
        let wrapped = wrap_master_key(&export_key, &amk, v.recovery);
        let got = unwrap_master_key(&export_key, &wrapped, v.recovery).expect("unwrap");
        assert_eq!(hex::encode(got.expose()), v.amk);
    }
}

#[test]
fn wrap_context_is_bound_by_aad() {
    for v in load() {
        let export_key = hex::decode(&v.export_key).expect("hex export key");
        let wrapped = hex::decode(&v.wrapped).expect("hex wrapped");
        assert!(unwrap_master_key(&export_key, &wrapped, !v.recovery).is_err());
    }
}

#[test]
fn rejects_tampered_and_malformed_blobs() {
    let v = &load()[0];
    let export_key = hex::decode(&v.export_key).expect("hex export key");
    let wrapped = hex::decode(&v.wrapped).expect("hex wrapped");

    let mut bad_version = wrapped.clone();
    bad_version[0] = 0x02;
    assert!(unwrap_master_key(&export_key, &bad_version, v.recovery).is_err());

    let mut flipped = wrapped.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0x01;
    assert!(unwrap_master_key(&export_key, &flipped, v.recovery).is_err());

    assert!(unwrap_master_key(&export_key, &wrapped[..wrapped.len() - 1], v.recovery).is_err());
}

#[test]
fn a_device_wrap_needs_both_halves() {
    use thelemail_crypto::amk::{device_unwrap, device_wrap};

    let local = [7u8; 32];
    let server = [9u8; 32];
    let secret = b"the vault payload";

    let wrapped = device_wrap(&local, &server, secret);
    assert_ne!(
        &wrapped[1..],
        &secret[..],
        "the payload must not be stored in the clear"
    );
    assert_eq!(
        device_unwrap(&local, &server, &wrapped).expect("unwrap"),
        secret
    );

    assert!(
        device_unwrap(&[8u8; 32], &server, &wrapped).is_err(),
        "the keychain half alone must not open it"
    );
    assert!(
        device_unwrap(&local, &[1u8; 32], &wrapped).is_err(),
        "a disk copy without the server half must not open it"
    );

    let mut tampered = wrapped.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xff;
    assert!(
        device_unwrap(&local, &server, &tampered).is_err(),
        "tampering must be detected"
    );
}
