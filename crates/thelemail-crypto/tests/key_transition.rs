use base64::Engine as _;
use serde::Deserialize;
use thelemail_crypto::keytransition::{self, TransitionError};
use thelemail_crypto::openpgp::UnlockedKey;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/the168");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Vectors {
    schema: String,
    keys: Vec<VectorKey>,
    statements: Vec<StatementCase>,
    transitions: Vec<TransitionCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorKey {
    name: String,
    fingerprint: String,
    public_key_armored: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatementCase {
    name: String,
    valid: bool,
    #[serde(default)]
    expected: Option<Expected>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    account_id: String,
    key_fingerprint: String,
    #[serde(default)]
    key_transitions: Vec<ExpectedTransition>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedTransition {
    previous_key_fingerprint: String,
    signature: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransitionCase {
    name: String,
    account_id: String,
    new_key_fingerprint: String,
    previous_key_fingerprint: String,
    payload: String,
    signature: String,
    previous_public_key_armored: String,
    valid: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct OpenpgpJsFixture {
    transitions: Vec<TransitionCase>,
}

fn b64(value: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .expect("base64")
}

fn vectors() -> Vectors {
    let raw = std::fs::read_to_string(format!("{FIXTURES}/the168-vectors.json")).expect("vectors");
    serde_json::from_str(&raw).expect("parse vectors")
}

fn fixture_key(name: &str) -> UnlockedKey {
    let armored =
        std::fs::read_to_string(format!("{FIXTURES}/keys/{name}.asc")).expect("fixture key");
    UnlockedKey::open_unlocked(&armored).expect("open fixture key")
}

fn outcome(case: &TransitionCase) -> Result<(), TransitionError> {
    keytransition::verify(
        case.payload.as_bytes(),
        &b64(&case.signature),
        &case.previous_public_key_armored,
    )
}

#[test]
fn the_payload_matches_the_golden_bytes() {
    let v = vectors();
    assert_eq!(v.schema, "thelemail-the168-vectors/v1");
    for case in &v.transitions {
        let rebuilt = keytransition::payload(
            &case.account_id,
            &case.new_key_fingerprint,
            &case.previous_key_fingerprint,
        )
        .expect("payload");
        assert_eq!(rebuilt, case.payload, "{}", case.name);
    }
}

#[test]
fn every_golden_transition_gets_the_expected_verdict() {
    let v = vectors();
    assert!(v.transitions.iter().any(|c| c.valid));
    assert!(v.transitions.iter().any(|c| !c.valid));
    for case in &v.transitions {
        match (case.valid, outcome(case)) {
            (true, Ok(())) => {}
            (false, Err(err)) => assert_eq!(
                Some(err.code()),
                case.error.as_deref(),
                "{} failed for the wrong reason",
                case.name
            ),
            (true, Err(err)) => panic!("{} should verify, got {err:?}", case.name),
            (false, Ok(())) => panic!("{} should be rejected", case.name),
        }
    }
}

#[test]
fn endorsements_inside_valid_golden_statements_verify() {
    let v = vectors();
    let mut checked = 0;
    for statement in v.statements.iter().filter(|s| s.valid) {
        let expected = statement.expected.as_ref().expect("expected fields");
        for element in &expected.key_transitions {
            let key = v
                .keys
                .iter()
                .find(|k| k.fingerprint == element.previous_key_fingerprint)
                .unwrap_or_else(|| panic!("{}: unknown previous key", statement.name));
            let message = keytransition::payload(
                &expected.account_id,
                &expected.key_fingerprint,
                &element.previous_key_fingerprint,
            )
            .expect("payload");
            keytransition::verify(
                message.as_bytes(),
                &b64(&element.signature),
                &key.public_key_armored,
            )
            .unwrap_or_else(|err| panic!("{} via {}: {err:?}", statement.name, key.name));
            checked += 1;
        }
    }
    assert!(
        checked >= 5,
        "only {checked} statement endorsements were checked"
    );
}

#[test]
fn endorsements_made_by_openpgp_js_verify() {
    let raw = std::fs::read_to_string(format!("{FIXTURES}/openpgpjs-endorsements.json"))
        .expect("openpgp.js fixture");
    let fixture: OpenpgpJsFixture = serde_json::from_str(&raw).expect("parse fixture");
    assert_eq!(fixture.transitions.len(), 2);
    for case in &fixture.transitions {
        let rebuilt = keytransition::payload(
            &case.account_id,
            &case.new_key_fingerprint,
            &case.previous_key_fingerprint,
        )
        .expect("payload");
        assert_eq!(rebuilt, case.payload, "{}", case.name);
        outcome(case).unwrap_or_else(|err| panic!("{}: {err:?}", case.name));
    }
}

#[test]
fn an_endorsement_made_here_verifies_for_v4_and_v6_previous_keys() {
    let v = vectors();
    let account_id = "4b1f2c3d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";
    let new_fp = v
        .keys
        .iter()
        .find(|k| k.name == "current")
        .expect("current key")
        .fingerprint
        .clone();

    for name in ["previous-v4", "previous-v6-a", "previous-v6-b"] {
        let previous = fixture_key(name);
        let signature = keytransition::endorse(&previous, account_id, &new_fp).expect(name);
        let message =
            keytransition::payload(account_id, &new_fp, &previous.fingerprint_hex()).unwrap();
        let public = previous.public_key_armored().unwrap();

        keytransition::verify(message.as_bytes(), &signature, &public).expect(name);
        assert!(signature.len() <= keytransition::MAX_SIGNATURE_BYTES);
        assert_eq!(signature[0] & 0x3f, 2, "{name}: a single signature packet");

        let other =
            keytransition::payload(account_id, &previous.fingerprint_hex(), &new_fp).unwrap();
        assert_eq!(
            keytransition::verify(
                other
                    .replace(&new_fp, &previous.fingerprint_hex())
                    .as_bytes(),
                &signature,
                &public
            ),
            Err(TransitionError::Signature),
            "{name}: a different payload must not verify"
        );
    }
}

#[test]
fn an_endorsement_checked_against_another_key_names_the_signer() {
    let v = vectors();
    let account_id = "4b1f2c3d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";
    let new_fp = v
        .keys
        .iter()
        .find(|k| k.name == "current")
        .unwrap()
        .fingerprint
        .clone();
    let signer = fixture_key("previous-v6-a");
    let other = fixture_key("previous-v6-b");

    let signature = keytransition::endorse(&signer, account_id, &new_fp).unwrap();
    let claimed = keytransition::payload(account_id, &new_fp, &other.fingerprint_hex()).unwrap();
    assert_eq!(
        keytransition::verify(
            claimed.as_bytes(),
            &signature,
            &other.public_key_armored().unwrap()
        ),
        Err(TransitionError::Signer)
    );
}

#[test]
fn the_payload_refuses_values_it_cannot_encode_canonically() {
    let fp = "8143e5cf230140b74ad27243820483b821da5821f597d74a661d04e9ef55c3b8";
    assert_eq!(
        keytransition::payload("", fp, fp),
        Err(TransitionError::Malformed)
    );
    assert_eq!(
        keytransition::payload("a\"b", fp, fp),
        Err(TransitionError::Malformed)
    );
    assert_eq!(
        keytransition::payload("acct", &fp[..63], fp),
        Err(TransitionError::Malformed)
    );
    let upper = keytransition::payload("acct", &fp.to_uppercase(), fp).unwrap();
    assert!(upper.contains(fp) && !upper.contains(&fp.to_uppercase()));
}
