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

const VARIANTS: [&str; 2] = ["", "-v6"];

fn meta_for(suffix: &str) -> KeyMeta {
    let raw =
        std::fs::read_to_string(format!("{FIXTURES}/keys/meta{suffix}.json")).expect("read meta");
    serde_json::from_str(&raw).expect("parse meta")
}

fn unlocked_for(suffix: &str) -> (UnlockedKey, String) {
    let m = meta_for(suffix);
    let armored = std::fs::read_to_string(format!("{FIXTURES}/keys/account{suffix}.enc.asc"))
        .expect("read key");
    let key = UnlockedKey::open(&armored, &m.passphrase).expect("unlock key");
    (key, m.passphrase)
}

fn unlocked() -> (UnlockedKey, String) {
    unlocked_for("")
}

#[test]
fn unlocks_keys_generated_by_openpgp_js() {
    for suffix in VARIANTS {
        let m = meta_for(suffix);
        let (key, _) = unlocked_for(suffix);
        assert_eq!(key.fingerprint_hex(), m.fingerprint, "fixture{suffix}");
    }
}

#[test]
fn the_v6_fixture_is_what_the_web_client_generates() {
    let m = meta_for("-v6");
    assert_eq!(m.fingerprint.len(), 64);
    let (key, _) = unlocked_for("-v6");
    assert_eq!(key.fingerprint_bytes().len(), 32);
}

#[test]
fn rejects_a_wrong_passphrase() {
    let armored =
        std::fs::read_to_string(format!("{FIXTURES}/keys/account.enc.asc")).expect("read key");
    assert!(UnlockedKey::open(&armored, "not-the-passphrase").is_err());
}

#[test]
fn decrypts_messages_produced_by_openpgp_js() {
    for suffix in VARIANTS {
        let m = meta_for(suffix);
        let (key, _pw) = unlocked_for(suffix);
        assert!(!m.messages.is_empty());
        for (name, msg) in &m.messages {
            let ct = std::fs::read(format!("{FIXTURES}/messages/{name}{suffix}.js.pgp"))
                .unwrap_or_else(|_| panic!("read {name}{suffix}"));
            let got = key
                .decrypt(&ct)
                .unwrap_or_else(|e| panic!("{name}{suffix}: {e}"));
            assert_eq!(
                String::from_utf8_lossy(&got),
                msg.plaintext,
                "plaintext mismatch for {name}{suffix}"
            );
        }
    }
}

#[test]
fn openpgp_js_encrypts_to_v6_keys_with_seipd_v2() {
    for name in meta_for("-v6").messages.keys() {
        let ct = std::fs::read(format!("{FIXTURES}/messages/{name}-v6.js.pgp"))
            .unwrap_or_else(|_| panic!("read {name}"));
        let shape = inspect_wire_shape(&ct).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(shape.pkesk_version, 6, "{name} pkesk version");
        assert_eq!(shape.seipd_version, 2, "{name} seipd version");
    }
}

#[derive(Deserialize)]
struct GoMessageMeta {
    #[serde(rename = "plaintextLen")]
    plaintext_len: usize,
    #[serde(rename = "pkeskVersion")]
    pkesk_version: u8,
}

fn go_meta_for(suffix: &str) -> std::collections::BTreeMap<String, GoMessageMeta> {
    let raw = std::fs::read_to_string(format!("{FIXTURES}/messages/go-meta{suffix}.json"))
        .expect("read go meta");
    serde_json::from_str(&raw).expect("parse go meta")
}

#[test]
fn decrypts_messages_produced_by_the_server_encryptor() {
    for suffix in VARIANTS {
        let (key, _pw) = unlocked_for(suffix);
        let metas = go_meta_for(suffix);
        assert!(!metas.is_empty());
        for (name, meta) in &metas {
            let ct = std::fs::read(format!("{FIXTURES}/messages/{name}{suffix}.pgp"))
                .unwrap_or_else(|_| panic!("read {name}{suffix}"));
            let got = key
                .decrypt(&ct)
                .unwrap_or_else(|e| panic!("{name}{suffix}: {e}"));
            assert_eq!(
                got.len(),
                meta.plaintext_len,
                "plaintext length mismatch for {name}{suffix}"
            );
        }
    }
}

#[test]
fn server_ciphertext_shape_follows_the_recipient_key() {
    for (suffix, pkesk, seipd) in [("", 3, 1), ("-v6", 6, 2)] {
        for (name, meta) in &go_meta_for(suffix) {
            let ct = std::fs::read(format!("{FIXTURES}/messages/{name}{suffix}.pgp"))
                .unwrap_or_else(|_| panic!("read {name}{suffix}"));
            let shape = inspect_wire_shape(&ct).unwrap_or_else(|e| panic!("{name}{suffix}: {e}"));
            assert_eq!(shape.pkesk_version, pkesk, "{name}{suffix} pkesk version");
            assert_eq!(shape.seipd_version, seipd, "{name}{suffix} seipd version");
            assert_eq!(
                shape.pkesk_version, meta.pkesk_version,
                "{name}{suffix} agrees with generator"
            );
        }
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
