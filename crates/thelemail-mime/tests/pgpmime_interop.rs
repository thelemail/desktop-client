use serde::Deserialize;
use std::collections::BTreeMap;
use thelemail_mime::{extract_pgp_armor, is_pgp_encrypted_mime};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/mime");

#[derive(Deserialize)]
struct Expected {
    #[serde(rename = "isPgpEncrypted")]
    is_pgp_encrypted: bool,
    armor: Option<String>,
}

fn cases() -> BTreeMap<String, Expected> {
    let raw = std::fs::read_to_string(format!("{FIXTURES}/meta.json")).expect("read meta");
    serde_json::from_str(&raw).expect("parse meta")
}

#[test]
fn agrees_with_the_web_client_on_every_case() {
    let cases = cases();
    assert!(!cases.is_empty());
    for (name, expected) in &cases {
        let mime = std::fs::read_to_string(format!("{FIXTURES}/{name}.eml"))
            .unwrap_or_else(|_| panic!("read {name}"));

        assert_eq!(
            is_pgp_encrypted_mime(&mime),
            expected.is_pgp_encrypted,
            "isPgpEncryptedMime disagrees for {name}"
        );
        assert_eq!(
            extract_pgp_armor(&mime),
            expected.armor,
            "extractPgpArmor disagrees for {name}"
        );
    }
}

#[test]
fn hostile_input_does_not_panic() {
    for hostile in [
        "",
        "\r\n\r\n",
        "Content-Type: multipart/encrypted; boundary=\"x\"",
        "Content-Type: multipart/encrypted; boundary=\"x\"\r\n\r\n--x",
        "Content-Type: multipart/encrypted; boundary=\"\"\r\n\r\n--\r\n",
        "\u{feff}Content-Type: multipart/encrypted; boundary=\"é\"\r\n\r\n--é\r\n\r\n",
    ] {
        let _ = is_pgp_encrypted_mime(hostile);
        let _ = extract_pgp_armor(hostile);
    }
}
