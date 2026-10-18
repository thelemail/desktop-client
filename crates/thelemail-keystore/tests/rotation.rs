use base64::Engine as _;
use serde_json::Value;
use thelemail_crypto::amk::{Amk, derive_pgp_passphrase};
use thelemail_crypto::keytransition;
use thelemail_crypto::openpgp::{GeneratedKey, UnlockedKey, generate_account_key_for};
use thelemail_keystore::*;

const ACCOUNT: &str = "58dd106c-1b2a-4c3d-8e4f-5a6b7c8d9e0f";
const EMAIL: &str = "nora@thelemail.test";

struct Account {
    ks: Keystore,
    amk: Amk,
    first: GeneratedKey,
}

fn generate(amk: &Amk, emails: &[&str]) -> GeneratedKey {
    let passphrase = derive_pgp_passphrase(amk);
    let emails: Vec<String> = emails.iter().map(|e| (*e).to_owned()).collect();
    generate_account_key_for(&emails, passphrase.expose(), 0).expect("generate")
}

fn persisted_for(amk: &Amk, key: &GeneratedKey) -> PersistedVault {
    let passphrase = derive_pgp_passphrase(amk);
    let unlocked =
        UnlockedKey::open(&key.encrypted_private_key_armored, passphrase.expose()).expect("open");
    serde_json::from_value(serde_json::json!({
        "email": EMAIL,
        "authScheme": "opaque_v1",
        "secretKeyArmored": unlocked.secret_key_armored().expect("armor"),
        "amk": base64::engine::general_purpose::STANDARD.encode(amk.expose()),
    }))
    .expect("persisted vault")
}

fn account() -> Account {
    let amk = Amk::generate();
    let first = generate(&amk, &[EMAIL]);
    let ks = Keystore::new();
    assert!(ks.adopt_persisted(ACCOUNT, persisted_for(&amk, &first)));
    Account { ks, amk, first }
}

fn input(version: i64, key: &GeneratedKey) -> AccountKeyInput {
    AccountKeyInput {
        key_version: version,
        key_fingerprint: key.fingerprint_hex.clone(),
        encrypted_private_key: key.encrypted_private_key_armored.clone(),
    }
}

fn load(ks: &Keystore, keys: Vec<AccountKeyInput>) -> Value {
    serde_json::to_value(ks.load_account_keys(LoadAccountKeysArgs {
        account_id: ACCOUNT.to_owned(),
        keys,
    }))
    .expect("json")
}

fn prepare(ks: &Keystore) -> Value {
    serde_json::to_value(ks.prepare_key_rotation(
        PrepareKeyRotationArgs {
            account_id: ACCOUNT.to_owned(),
            emails: vec![EMAIL.to_owned(), "nora.b@thelemail.test".to_owned()],
        },
        0,
    ))
    .expect("json")
}

fn commit(ks: &Keystore, fingerprint: &str, version: i64) -> Value {
    serde_json::to_value(ks.commit_key_rotation(CommitKeyRotationArgs {
        account_id: ACCOUNT.to_owned(),
        key_fingerprint_hex: fingerprint.to_owned(),
        key_version: version,
    }))
    .expect("json")
}

fn rotate(ks: &Keystore, version: i64) -> Value {
    let prepared = prepare(ks);
    assert_eq!(prepared["ok"], true, "{prepared}");
    let fp = prepared["keyFingerprintHex"].as_str().unwrap().to_owned();
    assert_eq!(commit(ks, &fp, version)["ok"], true);
    prepared
}

fn encrypt_to(ks: &Keystore, public_key_armored: &str, plaintext: &[u8]) -> Vec<u8> {
    match ks.encrypt(EncryptArgs {
        account_id: ACCOUNT.to_owned(),
        recipient_public_key_armored: public_key_armored.to_owned(),
        plaintext: plaintext.to_vec(),
        sign_with_vault_key: false,
        alias_id: None,
    }) {
        EncryptResponse::Ok { ciphertext, .. } => ciphertext,
        EncryptResponse::Err { code, .. } => panic!("encrypt failed: {code}"),
    }
}

fn decrypt(ks: &Keystore, ciphertext: &[u8], hint: Option<&str>) -> Value {
    serde_json::to_value(ks.decrypt(DecryptArgs {
        account_id: ACCOUNT.to_owned(),
        ciphertext_armored: None,
        ciphertext_binary: Some(ciphertext.to_vec()),
        binary: false,
        verification_keys_armored: None,
        key_fingerprint_hex: hint.map(str::to_owned),
    }))
    .expect("json")
}

fn current_public(ks: &Keystore) -> (String, Option<i64>) {
    match ks.get_public_key(ACCOUNT) {
        GetPublicKeyResponse::Ok {
            public_key_armored,
            key_version,
            ..
        } => (public_key_armored, key_version),
        GetPublicKeyResponse::Err { code, .. } => panic!("locked: {code}"),
    }
}

#[test]
fn a_rotation_endorses_with_the_retired_key_and_keeps_old_mail_readable() {
    let a = account();
    let first_fp = a.first.fingerprint_hex.clone();
    let loaded = load(&a.ks, vec![input(1, &a.first)]);
    assert_eq!(loaded["keyVersion"], 1);
    assert_eq!(loaded["currentChanged"], false);

    let old_mail = encrypt_to(&a.ks, &a.first.public_key_armored, b"written to the first key");

    let mut events = a.ks.subscribe();
    let prepared = prepare(&a.ks);
    assert_eq!(prepared["ok"], true, "{prepared}");
    assert_eq!(prepared["expectedKeyVersion"], 1);
    let new_fp = prepared["keyFingerprintHex"].as_str().unwrap().to_owned();
    assert_eq!(new_fp.len(), 64);
    assert_ne!(new_fp, first_fp);

    let transitions = prepared["transitions"].as_array().unwrap();
    assert_eq!(transitions.len(), 1);
    assert_eq!(transitions[0]["previousKeyFingerprint"], first_fp.as_str());
    let signature = base64::engine::general_purpose::STANDARD
        .decode(transitions[0]["signature"].as_str().unwrap())
        .unwrap();
    let message = keytransition::payload(ACCOUNT, &new_fp, &first_fp).unwrap();
    keytransition::verify(message.as_bytes(), &signature, &a.first.public_key_armored)
        .expect("the endorsement verifies against the retired key");

    let new_public = prepared["publicKeyArmored"].as_str().unwrap();
    let passphrase = derive_pgp_passphrase(&a.amk);
    let reopened = UnlockedKey::open(
        prepared["encryptedPrivateKey"].as_str().unwrap(),
        passphrase.expose(),
    )
    .expect("the new key is locked under the same passphrase");
    assert_eq!(reopened.fingerprint_hex(), new_fp);
    assert!(new_public.contains("BEGIN PGP PUBLIC KEY BLOCK"));

    assert_eq!(commit(&a.ks, &new_fp, 2)["ok"], true);
    match events.try_recv().expect("keys event") {
        Broadcast::AccountKeysChanged {
            account_id,
            fingerprints,
            current_changed,
        } => {
            assert_eq!(account_id, ACCOUNT);
            assert!(current_changed);
            assert_eq!(fingerprints, vec![new_fp.clone(), first_fp.clone()]);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(
        events.try_recv().expect("vault event"),
        Broadcast::VaultChanged { .. }
    ));

    let (public, version) = current_public(&a.ks);
    assert_eq!(version, Some(2));
    assert_eq!(
        thelemail_crypto::openpgp::public_key_fingerprint_hex(&public).unwrap(),
        new_fp
    );

    for hint in [None, Some(first_fp.as_str()), Some(new_fp.as_str())] {
        let out = decrypt(&a.ks, &old_mail, hint);
        assert_eq!(out["plaintext"], "written to the first key", "hint {hint:?}");
    }
    let new_mail = encrypt_to(&a.ks, &public, b"written to the new key");
    assert_eq!(
        decrypt(&a.ks, &new_mail, None)["plaintext"],
        "written to the new key"
    );
    assert_eq!(
        a.ks.decrypt_bytes(ACCOUNT, &old_mail, None).unwrap(),
        b"written to the first key"
    );

    let second = prepare(&a.ks);
    let endorsers: Vec<&str> = second["transitions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["previousKeyFingerprint"].as_str().unwrap())
        .collect();
    assert_eq!(endorsers, vec![new_fp.as_str(), first_fp.as_str()]);
    assert_eq!(second["expectedKeyVersion"], 2);
}

#[test]
fn a_rotation_waits_until_the_key_history_is_known() {
    let a = account();
    assert_eq!(prepare(&a.ks)["code"], "key_history_incomplete");

    let loaded = load(&a.ks, vec![input(3, &a.first)]);
    assert_eq!(loaded["keyVersion"], 3);
    assert_eq!(prepare(&a.ks)["code"], "key_history_incomplete");

    let older = generate(&a.amk, &[EMAIL]);
    let oldest = generate(&a.amk, &[EMAIL]);
    load(&a.ks, vec![input(2, &older), input(1, &oldest)]);
    let prepared = prepare(&a.ks);
    assert_eq!(prepared["ok"], true, "{prepared}");
    assert_eq!(prepared["transitions"].as_array().unwrap().len(), 3);
}

#[test]
fn loading_keys_adopts_a_newer_key_and_never_moves_back() {
    let a = account();
    let newer = generate(&a.amk, &[EMAIL]);

    let loaded = load(&a.ks, vec![input(1, &a.first), input(2, &newer)]);
    assert_eq!(loaded["currentChanged"], true);
    assert_eq!(loaded["currentFingerprintHex"], newer.fingerprint_hex.as_str());
    assert_eq!(loaded["keyVersion"], 2);
    assert_eq!(loaded["added"], serde_json::json!([newer.fingerprint_hex]));

    let again = load(&a.ks, vec![input(1, &a.first)]);
    assert_eq!(again["currentChanged"], false);
    assert_eq!(again["currentFingerprintHex"], newer.fingerprint_hex.as_str());
    assert_eq!(again["keyVersion"], 2);

    let foreign = generate(&Amk::generate(), &[EMAIL]);
    let mut mislabelled = input(4, &newer);
    mislabelled.key_fingerprint = a.first.fingerprint_hex.clone();
    let rejected = load(&a.ks, vec![input(3, &foreign), mislabelled]);
    assert_eq!(rejected["failed"], serde_json::json!([3]));
    assert_eq!(rejected["currentFingerprintHex"], newer.fingerprint_hex.as_str());
    assert_eq!(rejected["keyVersion"], 2);
}

#[test]
fn a_key_list_without_the_current_key_never_replaces_it() {
    let a = account();
    let other = generate(&a.amk, &[EMAIL]);
    let loaded = load(&a.ks, vec![input(2, &other)]);
    assert_eq!(loaded["currentChanged"], false);
    assert_eq!(loaded["currentFingerprintHex"], a.first.fingerprint_hex.as_str());
    assert_eq!(loaded["added"], serde_json::json!([other.fingerprint_hex]));

    let mail = encrypt_to(&a.ks, &other.public_key_armored, b"history only");
    assert_eq!(decrypt(&a.ks, &mail, None)["plaintext"], "history only");
}

#[test]
fn an_alias_grant_wrapped_to_the_retired_key_still_unwraps() {
    let a = account();
    load(&a.ks, vec![input(1, &a.first)]);
    let created = serde_json::to_value(a.ks.create_alias_key(
        CreateAliasKeyArgs {
            account_id: ACCOUNT.to_owned(),
            email: "team@thelemail.test".to_owned(),
            display_name: "Team".to_owned(),
            recipients: vec![CreateAliasKeyRecipient {
                account_id: ACCOUNT.to_owned(),
                public_key_armored: a.first.public_key_armored.clone(),
            }],
        },
        0,
    ))
    .unwrap();
    assert_eq!(created["ok"], true, "{created}");
    let alias_fp = created["keyFingerprintHex"].as_str().unwrap().to_owned();
    let alias_public = created["publicKeyArmored"].as_str().unwrap().to_owned();
    let wrapped = created["grants"][0]["wrappedPrivateKeyArmored"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        created["grants"][0]["memberKeyFingerprintHex"],
        a.first.fingerprint_hex.as_str()
    );

    rotate(&a.ks, 2);

    for hint in [Some(a.first.fingerprint_hex.clone()), None] {
        a.ks.unload_alias_keys(ACCOUNT);
        let loaded = serde_json::to_value(a.ks.load_alias_keys(LoadAliasKeysArgs {
            account_id: ACCOUNT.to_owned(),
            grants: vec![AliasKeyGrantInput {
                alias_id: "alias-1".to_owned(),
                key_version: 1,
                alias_key_fingerprint_hex: alias_fp.clone(),
                wrapped_private_key_armored: wrapped.clone(),
                member_key_fingerprint_hex: hint.clone(),
            }],
        }))
        .unwrap();
        assert_eq!(loaded["loaded"], serde_json::json!([alias_fp]), "hint {hint:?}");
        assert_eq!(loaded["failed"], serde_json::json!([]));
    }

    let alias_mail = encrypt_to(&a.ks, &alias_public, b"to the team");
    assert_eq!(
        decrypt(&a.ks, &alias_mail, Some(&alias_fp))["plaintext"],
        "to the team"
    );
}

#[test]
fn a_persisted_vault_written_before_rotation_still_restores() {
    let amk = Amk::generate();
    let key = generate(&amk, &[EMAIL]);
    let persisted = persisted_for(&amk, &key);
    let legacy = serde_json::json!({
        "email": persisted.email,
        "authScheme": "opaque_v1",
        "secretKeyArmored": persisted.secret_key_armored,
        "amk": persisted.amk,
    });
    let restored: PersistedVault = serde_json::from_value(legacy).expect("legacy vault parses");
    assert_eq!(restored.key_version, 0);
    assert!(restored.history.is_empty());

    let ks = Keystore::new();
    assert!(ks.adopt_persisted(ACCOUNT, restored));
    let (_, version) = current_public(&ks);
    assert_eq!(version, None);
}

#[test]
fn a_persisted_vault_carries_the_key_history() {
    let a = account();
    load(&a.ks, vec![input(1, &a.first)]);
    let old_mail = encrypt_to(&a.ks, &a.first.public_key_armored, b"before the rotation");
    rotate(&a.ks, 2);

    let persisted = a.ks.persistable(ACCOUNT).expect("persistable");
    let encoded = serde_json::to_vec(&persisted).unwrap();
    let decoded: PersistedVault = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded.key_version, 2);
    assert_eq!(decoded.history.len(), 1);
    assert_eq!(decoded.history[0].key_version, 1);

    let restored = Keystore::new();
    assert!(restored.adopt_persisted(ACCOUNT, decoded));
    let (_, version) = current_public(&restored);
    assert_eq!(version, Some(2));
    assert_eq!(
        decrypt(&restored, &old_mail, None)["plaintext"],
        "before the rotation"
    );
}

#[test]
fn mail_for_a_key_this_vault_never_held_reports_no_matching_key() {
    let a = account();
    let stranger = generate(&Amk::generate(), &["someone@example.com"]);
    let mail = encrypt_to(&a.ks, &stranger.public_key_armored, b"not for us");
    assert_eq!(decrypt(&a.ks, &mail, None)["code"], "no_matching_key");
    assert_eq!(
        decrypt(&a.ks, &mail, Some(&a.first.fingerprint_hex))["code"],
        "no_matching_key"
    );
    assert_eq!(a.ks.decrypt_bytes(ACCOUNT, &mail, None), Err("no_matching_key"));
    assert_eq!(
        a.ks.decrypt_bytes(ACCOUNT, b"not a pgp message", None),
        Err("invalid_ciphertext")
    );
}

#[test]
fn a_commit_needs_the_prepared_key_and_a_higher_version() {
    let a = account();
    load(&a.ks, vec![input(1, &a.first)]);
    assert_eq!(commit(&a.ks, &a.first.fingerprint_hex, 2)["code"], "no_pending_rotation");

    let prepared = prepare(&a.ks);
    let fp = prepared["keyFingerprintHex"].as_str().unwrap().to_owned();
    assert_eq!(commit(&a.ks, &a.first.fingerprint_hex, 2)["code"], "invalid");
    assert_eq!(commit(&a.ks, &fp, 1)["code"], "invalid");

    a.ks.abandon_key_rotation(ACCOUNT);
    assert_eq!(commit(&a.ks, &fp, 2)["code"], "no_pending_rotation");
    let (_, version) = current_public(&a.ks);
    assert_eq!(version, Some(1));
}

#[test]
fn a_key_adopted_after_a_lost_commit_clears_the_pending_rotation() {
    let a = account();
    load(&a.ks, vec![input(1, &a.first)]);
    let prepared = prepare(&a.ks);
    let fp = prepared["keyFingerprintHex"].as_str().unwrap().to_owned();
    let served = AccountKeyInput {
        key_version: 2,
        key_fingerprint: fp.clone(),
        encrypted_private_key: prepared["encryptedPrivateKey"].as_str().unwrap().to_owned(),
    };

    let loaded = load(&a.ks, vec![input(1, &a.first), served]);
    assert_eq!(loaded["currentChanged"], true);
    assert_eq!(loaded["currentFingerprintHex"], fp.as_str());
    assert_eq!(commit(&a.ks, &fp, 2)["code"], "no_pending_rotation");
}

#[test]
fn an_srp_vault_cannot_rotate() {
    let amk = Amk::generate();
    let key = generate(&amk, &[EMAIL]);
    let mut persisted = persisted_for(&amk, &key);
    persisted.auth_scheme = AuthScheme::SrpV1;
    let ks = Keystore::new();
    assert!(ks.adopt_persisted(ACCOUNT, persisted));
    assert_eq!(load(&ks, vec![])["code"], "not_opaque");
    assert_eq!(prepare(&ks)["code"], "not_opaque");
}
