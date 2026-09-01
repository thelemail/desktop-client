use serde::Deserialize;
use thelemail_crypto::openpgp::{UnlockedKey, inspect_wire_shape};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

#[derive(Deserialize)]
struct KeyMeta {
    passphrase: String,
    fingerprint: String,
    messages: std::collections::BTreeMap<String, MessageMeta>,
}

#[derive(Deserialize)]
struct MessageMeta {
    plaintext: String,
}

fn meta() -> KeyMeta {
    let raw = std::fs::read_to_string(format!("{FIXTURES}/keys/meta.json")).expect("read meta");
    serde_json::from_str(&raw).expect("parse meta")
}

fn unlocked() -> (UnlockedKey, String) {
    let m = meta();
    let armored =
        std::fs::read_to_string(format!("{FIXTURES}/keys/account.enc.asc")).expect("read key");
    let key = UnlockedKey::open(&armored, &m.passphrase).expect("unlock key");
    (key, m.passphrase)
}

#[test]
fn unlocks_a_key_generated_by_openpgp_js() {
    let m = meta();
    let (key, _) = unlocked();
    assert_eq!(key.fingerprint_hex(), m.fingerprint);
}

#[test]
fn rejects_a_wrong_passphrase() {
    let armored =
        std::fs::read_to_string(format!("{FIXTURES}/keys/account.enc.asc")).expect("read key");
    assert!(UnlockedKey::open(&armored, "not-the-passphrase").is_err());
}

#[test]
fn decrypts_messages_produced_by_openpgp_js() {
    let m = meta();
    let (key, _pw) = unlocked();
    assert!(!m.messages.is_empty());
    for (name, msg) in &m.messages {
        let ct = std::fs::read(format!("{FIXTURES}/messages/{name}.js.pgp"))
            .unwrap_or_else(|_| panic!("read {name}"));
        let got = key.decrypt(&ct).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            String::from_utf8_lossy(&got),
            msg.plaintext,
            "plaintext mismatch for {name}"
        );
    }
}

#[derive(Deserialize)]
struct GoMessageMeta {
    #[serde(rename = "plaintextLen")]
    plaintext_len: usize,
    #[serde(rename = "pkeskVersion")]
    pkesk_version: u8,
}

fn go_meta() -> std::collections::BTreeMap<String, GoMessageMeta> {
    let raw =
        std::fs::read_to_string(format!("{FIXTURES}/messages/go-meta.json")).expect("read go meta");
    serde_json::from_str(&raw).expect("parse go meta")
}

#[test]
fn decrypts_messages_produced_by_the_server_encryptor() {
    let (key, _pw) = unlocked();
    let metas = go_meta();
    assert!(!metas.is_empty());
    for (name, meta) in &metas {
        let ct = std::fs::read(format!("{FIXTURES}/messages/{name}.pgp"))
            .unwrap_or_else(|_| panic!("read {name}"));
        let got = key.decrypt(&ct).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            got.len(),
            meta.plaintext_len,
            "plaintext length mismatch for {name}"
        );
    }
}

#[test]
fn stored_ciphertext_is_pkesk_v3_and_seipd_v1() {
    for (name, meta) in &go_meta() {
        let ct = std::fs::read(format!("{FIXTURES}/messages/{name}.pgp"))
            .unwrap_or_else(|_| panic!("read {name}"));
        let shape = inspect_wire_shape(&ct).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(shape.pkesk_version, 3, "{name} pkesk version");
        assert_eq!(shape.seipd_version, 1, "{name} seipd version");
        assert_eq!(
            shape.pkesk_version, meta.pkesk_version,
            "{name} agrees with generator"
        );
    }
}

#[test]
fn rejects_truncated_and_tampered_ciphertext() {
    let (key, _pw) = unlocked();
    let ct = std::fs::read(format!("{FIXTURES}/messages/body-plain-go.pgp")).expect("read");

    assert!(key.decrypt(&ct[..ct.len() / 2]).is_err());

    let mut flipped = ct.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0xff;
    assert!(key.decrypt(&flipped).is_err());

    assert!(key.decrypt(b"not an openpgp message at all").is_err());
}
