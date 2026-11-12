mod support;

use std::cell::Cell;

use serde_json::{Value, json};
use support::*;
use thelemail_keystore::*;
use thelemail_trust::Family;

thread_local! {
    static ACCOUNT_ID: Cell<Option<&'static str>> = const { Cell::new(None) };
}

fn acct() -> &'static str {
    ACCOUNT_ID.with(|cell| match cell.get() {
        Some(id) => id,
        None => {
            let id: &'static str = Box::leak(new_account_id().into_boxed_str());
            cell.set(Some(id));
            id
        }
    })
}

struct Account {
    ks: Keystore,
    pass: Pass,
    first: Key,
}

fn generate(pass: &Pass, emails: &[&str]) -> Key {
    support::generate(pass, emails[0])
}

fn account() -> Account {
    let pass = Pass::generate();
    let first = generate(&pass, &[EMAIL]);
    let ks = Keystore::new();
    open_with_passphrase(&ks, acct(), &pass, &first, 0);
    Account { ks, pass, first }
}

fn device_with(pass: &Pass, key: &Key) -> Keystore {
    let ks = Keystore::new();
    open_with_passphrase(&ks, acct(), pass, key, 0);
    ks
}

fn fingerprint_of(armored: &str) -> String {
    trust(Family::Keys, json!({"op": "inspect", "armored": armored}))["fingerprint"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn opens_with(pass: &Pass, encrypted: &str) -> String {
    testkit(json!({
        "op": "signTransition",
        "accountId": acct(),
        "encryptedPrivateKey": encrypted,
        "passphrase": pass.0,
        "newKeyFingerprint": "00".repeat(32),
    }))["previousKeyFingerprint"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn endorsement_verifies(new_fp: &str, previous: &Key, signature: &[u8]) -> bool {
    use base64::Engine as _;
    trust(
        Family::Trust,
        json!({
            "op": "keyTransition",
            "accountId": acct(),
            "newKeyFingerprint": new_fp,
            "previousKeyFingerprint": previous.fingerprint_hex,
            "signature": b64().encode(signature),
            "previousPublicKey": previous.public_key_armored,
        }),
    )
    .get("failure")
    .is_none()
}

fn input(version: i64, key: &Key) -> AccountKeyInput {
    AccountKeyInput {
        key_version: version,
        fingerprint_hex: key.fingerprint_hex.clone(),
        encrypted_private_key_armored: key.encrypted_private_key_armored.clone(),
        is_current: false,
        transitions: Vec::new(),
    }
}

fn current(version: i64, key: &Key) -> AccountKeyInput {
    AccountKeyInput {
        is_current: true,
        ..input(version, key)
    }
}

fn endorsement(pass: &Pass, by: &Key, new_fp: &str) -> AccountKeyTransitionInput {
    endorsement_for(acct(), pass, by, new_fp)
}

fn history(pass: &Pass, keys: &[&Key]) -> Vec<AccountKeyInput> {
    keys.iter()
        .enumerate()
        .map(|(i, key)| AccountKeyInput {
            is_current: i + 1 == keys.len(),
            transitions: keys[..i]
                .iter()
                .rev()
                .take(16)
                .map(|by| endorsement(pass, by, &key.fingerprint_hex))
                .collect(),
            ..input(i as i64 + 1, key)
        })
        .collect()
}

fn load(ks: &Keystore, keys: Vec<AccountKeyInput>) -> Value {
    let current_version = keys.len() as i64;
    load_with(ks, current_version, keys)
}

fn load_with(ks: &Keystore, current_version: i64, keys: Vec<AccountKeyInput>) -> Value {
    serde_json::to_value(ks.load_account_keys(LoadAccountKeysArgs {
        account_id: acct().to_owned(),
        current_version,
        keys,
    }))
    .expect("json")
}

fn prepare_with(ks: &Keystore, expected_key_version: i64, emails: &[&str]) -> Value {
    serde_json::to_value(ks.prepare_key_rotation(
        PrepareKeyRotationArgs {
            account_id: acct().to_owned(),
            emails: emails.iter().map(|e| (*e).to_owned()).collect(),
            expected_key_version,
        },
        0,
    ))
    .expect("json")
}

fn prepare(ks: &Keystore) -> Value {
    let (_, version) = current_public(ks);
    prepare_with(ks, version.unwrap_or(1), &[EMAIL, "nora.b@thelemail.test"])
}

fn commit(ks: &Keystore, fingerprint: &str) -> Value {
    serde_json::to_value(ks.commit_key_rotation(CommitKeyRotationArgs {
        account_id: acct().to_owned(),
        fingerprint_hex: fingerprint.to_owned(),
    }))
    .expect("json")
}

fn rotate(ks: &Keystore) -> Value {
    let prepared = prepare(ks);
    assert_eq!(prepared["ok"], true, "{prepared}");
    let fp = prepared["fingerprintHex"].as_str().unwrap().to_owned();
    assert_eq!(commit(ks, &fp)["ok"], true);
    prepared
}

fn signature_bytes(value: &Value) -> Vec<u8> {
    value
        .as_array()
        .expect("signature bytes")
        .iter()
        .map(|b| u8::try_from(b.as_u64().expect("byte")).expect("byte"))
        .collect()
}

fn endorsers(prepared: &Value) -> Vec<String> {
    prepared["transitions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["previousKeyFingerprintHex"].as_str().unwrap().to_owned())
        .collect()
}

fn encrypt_to(ks: &Keystore, public_key_armored: &str, plaintext: &[u8]) -> Vec<u8> {
    match ks.encrypt(EncryptArgs {
        account_id: acct().to_owned(),
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
        account_id: acct().to_owned(),
        ciphertext_armored: None,
        ciphertext_binary: Some(ciphertext.to_vec()),
        binary: false,
        verification_keys_armored: None,
        key_fingerprint_hex: hint.map(str::to_owned),
    }))
    .expect("json")
}

fn current_public(ks: &Keystore) -> (String, Option<i64>) {
    match ks.get_public_key(acct()) {
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
    let loaded = load(&a.ks, vec![current(1, &a.first)]);
    assert_eq!(loaded["keyVersion"], 1);
    assert_eq!(loaded["currentChanged"], false);

    let old_mail = encrypt_to(
        &a.ks,
        &a.first.public_key_armored,
        b"written to the first key",
    );

    let mut events = a.ks.subscribe();
    let prepared = prepare(&a.ks);
    assert_eq!(prepared["ok"], true, "{prepared}");
    assert_eq!(prepared["keyVersion"], 2);
    let new_fp = prepared["fingerprintHex"].as_str().unwrap().to_owned();
    assert_eq!(new_fp.len(), 64);
    assert_ne!(new_fp, first_fp);

    let transitions = prepared["transitions"].as_array().unwrap();
    assert_eq!(transitions.len(), 1);
    assert_eq!(
        transitions[0]["previousKeyFingerprintHex"],
        first_fp.as_str()
    );
    let signature = signature_bytes(&transitions[0]["signature"]);
    assert!(
        endorsement_verifies(&new_fp, &a.first, &signature),
        "the endorsement verifies against the retired key"
    );

    let new_public = prepared["publicKeyArmored"].as_str().unwrap();
    assert_eq!(
        opens_with(&a.pass, prepared["encryptedPrivateKey"].as_str().unwrap()),
        new_fp,
        "the new key is locked under the same passphrase"
    );
    assert!(new_public.contains("BEGIN PGP PUBLIC KEY BLOCK"));

    let committed = commit(&a.ks, &new_fp);
    assert_eq!(committed["ok"], true);
    assert_eq!(committed["keyVersion"], 2);
    match events.try_recv().expect("keys event") {
        Broadcast::AccountKeysChanged {
            account_id,
            fingerprints,
            current_fingerprint_hex,
            key_version,
            current_changed,
            changed,
        } => {
            assert_eq!(account_id, acct());
            assert!(current_changed);
            assert!(changed);
            assert_eq!(current_fingerprint_hex, new_fp);
            assert_eq!(key_version, 2);
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
    assert_eq!(fingerprint_of(&public), new_fp);

    for hint in [None, Some(first_fp.as_str()), Some(new_fp.as_str())] {
        let out = decrypt(&a.ks, &old_mail, hint);
        assert_eq!(
            out["plaintext"], "written to the first key",
            "hint {hint:?}"
        );
    }
    let new_mail = encrypt_to(&a.ks, &public, b"written to the new key");
    assert_eq!(
        decrypt(&a.ks, &new_mail, None)["plaintext"],
        "written to the new key"
    );
    assert_eq!(
        a.ks.decrypt_bytes(acct(), &old_mail, None).unwrap(),
        b"written to the first key"
    );

    let second = prepare(&a.ks);
    assert_eq!(endorsers(&second), vec![new_fp.clone(), first_fp.clone()]);
    assert_eq!(second["keyVersion"], 3);
}

#[test]
fn a_rotation_waits_until_the_key_history_is_known() {
    let a = account();
    assert_eq!(prepare(&a.ks)["code"], "key_history_incomplete");

    let older = generate(&a.pass, &[EMAIL]);
    let oldest = generate(&a.pass, &[EMAIL]);
    let stranger = generate(&Pass::generate(), &[EMAIL]);
    let mut locked_out = history(&a.pass, &[&oldest, &older, &a.first]);
    locked_out[0].encrypted_private_key_armored = stranger.encrypted_private_key_armored.clone();
    let loaded = load(&a.ks, locked_out);
    assert_eq!(loaded["keyVersion"], 3);
    assert_eq!(loaded["failed"], serde_json::json!([1]));
    assert_eq!(prepare(&a.ks)["code"], "key_history_incomplete");

    load(&a.ks, history(&a.pass, &[&oldest, &older, &a.first]));
    let prepared = prepare(&a.ks);
    assert_eq!(prepared["ok"], true, "{prepared}");
    assert_eq!(prepared["transitions"].as_array().unwrap().len(), 3);
}

#[test]
fn loading_keys_adopts_an_endorsed_newer_key_and_never_moves_back() {
    let a = account();
    let newer = generate(&a.pass, &[EMAIL]);

    let loaded = load(&a.ks, history(&a.pass, &[&a.first, &newer]));
    assert_eq!(loaded["currentChanged"], true);
    assert_eq!(
        loaded["currentFingerprintHex"],
        newer.fingerprint_hex.as_str()
    );
    assert_eq!(loaded["keyVersion"], 2);
    assert_eq!(
        loaded["loaded"],
        serde_json::json!([a.first.fingerprint_hex, newer.fingerprint_hex])
    );
    assert_eq!(loaded["unverified"], serde_json::json!([]));

    let again = load(&a.ks, vec![current(1, &a.first)]);
    assert_eq!(again["ok"], true, "{again}");
    assert_eq!(again["currentChanged"], false);
    assert_eq!(
        again["currentFingerprintHex"],
        newer.fingerprint_hex.as_str()
    );
    assert_eq!(again["keyVersion"], 2);
}

#[test]
fn keys_that_fail_to_open_are_reported_by_their_own_version() {
    let a = account();
    let stranger = generate(&Pass::generate(), &[EMAIL]);
    let locked = generate(&a.pass, &[EMAIL]);
    let second = generate(&a.pass, &[EMAIL]);
    let third = generate(&a.pass, &[EMAIL]);
    let fourth = generate(&a.pass, &[EMAIL]);
    let mut keys = history(&a.pass, &[&a.first, &locked, &second, &third, &fourth]);
    keys[1].encrypted_private_key_armored = stranger.encrypted_private_key_armored.clone();
    keys[3].encrypted_private_key_armored = fourth.encrypted_private_key_armored.clone();
    let loaded = load(&a.ks, keys);
    assert_eq!(loaded["failed"], serde_json::json!([2, 4]));
    assert_eq!(loaded["unverified"], serde_json::json!([3, 5]));
    assert_eq!(loaded["keyVersion"], 1);
    assert_eq!(
        loaded["currentFingerprintHex"],
        a.first.fingerprint_hex.as_str()
    );
}

#[test]
fn a_key_list_without_the_current_key_adopts_nothing() {
    let a = account();
    let other = generate(&a.pass, &[EMAIL]);
    let mut events = a.ks.subscribe();
    let ignored = load(&a.ks, vec![current(1, &other)]);
    assert_eq!(ignored["ok"], true, "{ignored}");
    assert_eq!(ignored["unverified"], serde_json::json!([1]));
    assert_eq!(ignored["loaded"], serde_json::json!([]));
    assert_eq!(ignored["keyVersion"], 0);
    assert_eq!(ignored["currentChanged"], false);
    assert_eq!(
        ignored["currentFingerprintHex"],
        a.first.fingerprint_hex.as_str()
    );
    while let Ok(event) = events.try_recv() {
        let event = serde_json::to_value(event).unwrap();
        assert_eq!(event["currentChanged"], false, "{event}");
        assert_eq!(
            event["fingerprints"],
            serde_json::json!([a.first.fingerprint_hex])
        );
    }

    load(&a.ks, vec![current(1, &a.first)]);
    let mut events = a.ks.subscribe();
    let swapped = load(&a.ks, vec![current(1, &other)]);
    assert_eq!(swapped["code"], "key_list_invalid");
    assert!(events.try_recv().is_err());
    let (public, version) = current_public(&a.ks);
    assert_eq!(version, Some(1));
    assert_eq!(fingerprint_of(&public), a.first.fingerprint_hex);
}

#[test]
fn an_alias_grant_wrapped_to_the_retired_key_still_unwraps() {
    let a = account();
    load(&a.ks, vec![current(1, &a.first)]);
    let created = serde_json::to_value(a.ks.create_alias_key(
        CreateAliasKeyArgs {
            account_id: acct().to_owned(),
            email: "team@thelemail.test".to_owned(),
            display_name: "Team".to_owned(),
            recipients: vec![CreateAliasKeyRecipient {
                account_id: acct().to_owned(),
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

    rotate(&a.ks);

    for hint in [Some(a.first.fingerprint_hex.clone()), None] {
        a.ks.unload_alias_keys(acct());
        let loaded = serde_json::to_value(a.ks.load_alias_keys(LoadAliasKeysArgs {
            account_id: acct().to_owned(),
            grants: vec![AliasKeyGrantInput {
                alias_id: "alias-1".to_owned(),
                key_version: 1,
                alias_key_fingerprint_hex: alias_fp.clone(),
                wrapped_private_key_armored: wrapped.clone(),
                member_key_fingerprint_hex: hint.clone(),
            }],
        }))
        .unwrap();
        assert_eq!(
            loaded["loaded"],
            serde_json::json!([alias_fp]),
            "hint {hint:?}"
        );
        assert_eq!(loaded["failed"], serde_json::json!([]));
    }

    let alias_mail = encrypt_to(&a.ks, &alias_public, b"to the team");
    assert_eq!(
        decrypt(&a.ks, &alias_mail, Some(&alias_fp))["plaintext"],
        "to the team"
    );
}

#[test]
fn an_alias_grant_for_a_key_loaded_later_opens_once_that_key_arrives() {
    let a = account();
    let retired = generate(&a.pass, &[EMAIL]);
    let created = serde_json::to_value(a.ks.create_alias_key(
        CreateAliasKeyArgs {
            account_id: acct().to_owned(),
            email: "team@thelemail.test".to_owned(),
            display_name: "Team".to_owned(),
            recipients: vec![CreateAliasKeyRecipient {
                account_id: acct().to_owned(),
                public_key_armored: retired.public_key_armored.clone(),
            }],
        },
        0,
    ))
    .unwrap();
    assert_eq!(created["ok"], true, "{created}");
    let alias_fp = created["keyFingerprintHex"].as_str().unwrap().to_owned();
    let alias_public = created["publicKeyArmored"].as_str().unwrap().to_owned();
    let grant = || AliasKeyGrantInput {
        alias_id: "alias-1".to_owned(),
        key_version: 1,
        alias_key_fingerprint_hex: alias_fp.clone(),
        wrapped_private_key_armored: created["grants"][0]["wrappedPrivateKeyArmored"]
            .as_str()
            .unwrap()
            .to_owned(),
        member_key_fingerprint_hex: Some(retired.fingerprint_hex.clone()),
    };

    let early = serde_json::to_value(a.ks.load_alias_keys(LoadAliasKeysArgs {
        account_id: acct().to_owned(),
        grants: vec![grant()],
    }))
    .unwrap();
    assert_eq!(early["loaded"], serde_json::json!([]));
    assert_eq!(early["failed"][0]["aliasId"], "alias-1");
    let alias_mail = encrypt_to(&a.ks, &alias_public, b"to the team");
    assert_eq!(decrypt(&a.ks, &alias_mail, None)["code"], "no_matching_key");

    load(&a.ks, history(&a.pass, &[&retired, &a.first]));
    assert_eq!(
        decrypt(&a.ks, &alias_mail, Some(&alias_fp))["plaintext"],
        "to the team"
    );

    a.ks.unload_alias_keys(acct());
    let fresh = Account {
        ks: device_with(&a.pass, &a.first),
        pass: a.pass,
        first: a.first,
    };
    fresh.ks.load_alias_keys(LoadAliasKeysArgs {
        account_id: acct().to_owned(),
        grants: vec![grant()],
    });
    fresh.ks.unload_alias_keys(acct());
    load(&fresh.ks, history(&fresh.pass, &[&retired, &fresh.first]));
    assert_eq!(
        decrypt(&fresh.ks, &alias_mail, None)["code"],
        "no_matching_key",
        "unloading alias keys also drops the grants waiting for a key"
    );
}

#[test]
fn a_saved_session_without_a_key_version_restores_unversioned() {
    let a = account();
    let (_, version) = current_public(&a.ks);
    assert_eq!(version, None);
    let saved = a.ks.saved_keys(acct()).expect("saved keys");
    assert_eq!(saved.key_version, 0);
    assert!(saved.history_keys.is_empty());
}

#[test]
fn a_persisted_vault_carries_the_key_history() {
    let a = account();
    load(&a.ks, vec![current(1, &a.first)]);
    let old_mail = encrypt_to(&a.ks, &a.first.public_key_armored, b"before the rotation");
    rotate(&a.ks);

    let server_half = [0x5cu8; 32];
    let persisted = a.ks.persist(acct(), &server_half).expect("persist");
    let encoded = serde_json::to_vec(&persisted.keys).unwrap();
    let decoded: SavedKeys = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded.key_version, 2);
    assert_eq!(decoded.history_keys.len(), 1);
    assert_eq!(decoded.history_keys[0].key_version, 1);
    a.ks.lock(acct());

    let restored = Keystore::new();
    assert!(restored.restore(
        acct(),
        persisted.email,
        &decoded,
        &persisted.wrapped,
        persisted.wrap_version,
        &server_half,
    ));
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
    let stranger = generate(&Pass::generate(), &["someone@example.com"]);
    let mail = encrypt_to(&a.ks, &stranger.public_key_armored, b"not for us");
    assert_eq!(decrypt(&a.ks, &mail, None)["code"], "no_matching_key");
    assert_eq!(
        decrypt(&a.ks, &mail, Some(&a.first.fingerprint_hex))["code"],
        "no_matching_key"
    );
    assert_eq!(
        a.ks.decrypt_bytes(acct(), &mail, None),
        Err("no_matching_key")
    );
    assert_eq!(
        a.ks.decrypt_bytes(acct(), b"not a pgp message", None),
        Err("invalid_ciphertext")
    );
}

#[test]
fn a_commit_needs_the_prepared_key_at_the_next_version() {
    let a = account();
    load(&a.ks, vec![current(1, &a.first)]);
    assert_eq!(
        commit(&a.ks, &a.first.fingerprint_hex)["code"],
        "no_pending_rotation"
    );

    let prepared = prepare(&a.ks);
    let fp = prepared["fingerprintHex"].as_str().unwrap().to_owned();
    assert_eq!(
        commit(&a.ks, &a.first.fingerprint_hex)["code"],
        "no_pending_rotation"
    );

    a.ks.abandon_key_rotation(acct());
    assert_eq!(commit(&a.ks, &fp)["code"], "no_pending_rotation");
    let (_, version) = current_public(&a.ks);
    assert_eq!(version, Some(1));
}

#[test]
fn a_rotation_overtaken_by_another_device_is_not_committed() {
    let a = account();
    load(&a.ks, vec![current(1, &a.first)]);
    let prepared = prepare(&a.ks);
    let fp = prepared["fingerprintHex"].as_str().unwrap().to_owned();

    let elsewhere = generate(&a.pass, &[EMAIL]);
    let loaded = load(&a.ks, history(&a.pass, &[&a.first, &elsewhere]));
    assert_eq!(loaded["keyVersion"], 2);

    assert_eq!(commit(&a.ks, &fp)["code"], "no_pending_rotation");
    let (public, version) = current_public(&a.ks);
    assert_eq!(version, Some(2));
    assert_eq!(fingerprint_of(&public), elsewhere.fingerprint_hex);
}

#[test]
fn a_rotation_is_refused_for_a_stale_version_or_no_addresses() {
    let a = account();
    load(&a.ks, vec![current(1, &a.first)]);
    assert_eq!(
        prepare_with(&a.ks, 2, &[EMAIL])["code"],
        "stale_key_version"
    );
    assert_eq!(
        prepare_with(&a.ks, 0, &[EMAIL])["code"],
        "stale_key_version"
    );
    assert_eq!(prepare_with(&a.ks, 1, &[])["code"], "no_emails");
    assert_eq!(prepare_with(&a.ks, 1, &["  "])["code"], "no_emails");
    assert_eq!(prepare_with(&a.ks, 1, &[EMAIL])["ok"], true);
}

#[test]
fn only_the_sixteen_newest_keys_endorse_newest_first() {
    let a = account();
    let older: Vec<Key> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..17)
            .map(|_| scope.spawn(|| generate(&a.pass, &[EMAIL])))
            .collect();
        workers.into_iter().map(|w| w.join().unwrap()).collect()
    });
    let mut keys = vec![current(18, &a.first)];
    keys.extend(
        older
            .iter()
            .enumerate()
            .map(|(i, k)| input(i as i64 + 1, k)),
    );
    let loaded = load(&a.ks, keys);
    assert_eq!(loaded["currentChanged"], false);
    assert_eq!(loaded["keyVersion"], 18);
    assert_eq!(loaded["failed"], serde_json::json!([]));

    let prepared = prepare(&a.ks);
    assert_eq!(prepared["ok"], true, "{prepared}");
    assert_eq!(prepared["keyVersion"], 19);
    let mut expected = vec![a.first.fingerprint_hex.clone()];
    expected.extend(
        older
            .iter()
            .rev()
            .take(15)
            .map(|k| k.fingerprint_hex.clone()),
    );
    assert_eq!(endorsers(&prepared), expected);

    let new_fp = prepared["fingerprintHex"].as_str().unwrap().to_owned();
    let signers = std::iter::once(&a.first).chain(older.iter().rev());
    for (transition, key) in prepared["transitions"]
        .as_array()
        .unwrap()
        .iter()
        .zip(signers)
    {
        assert!(
            endorsement_verifies(&new_fp, key, &signature_bytes(&transition["signature"])),
            "each endorsement verifies against its own key"
        );
    }
}

#[test]
fn the_web_keystore_payloads_deserialize() {
    let load: LoadAccountKeysArgs = serde_json::from_value(serde_json::json!({
        "accountId": acct(),
        "currentVersion": 2,
        "keys": [{
            "keyVersion": 2,
            "fingerprintHex": "ab",
            "encryptedPrivateKeyArmored": "armored",
            "isCurrent": true,
            "transitions": [{ "previousKeyFingerprintHex": "cd", "signature": [1, 2, 255] }]
        }]
    }))
    .expect("loadAccountKeys args");
    assert_eq!(load.current_version, 2);
    assert_eq!(load.keys[0].key_version, 2);
    assert_eq!(load.keys[0].fingerprint_hex, "ab");
    assert_eq!(load.keys[0].encrypted_private_key_armored, "armored");
    assert!(load.keys[0].is_current);
    assert_eq!(
        load.keys[0].transitions[0].previous_key_fingerprint_hex,
        "cd"
    );
    assert_eq!(load.keys[0].transitions[0].signature, vec![1, 2, 255]);

    let prepare: PrepareKeyRotationArgs = serde_json::from_value(serde_json::json!({
        "accountId": acct(),
        "emails": [EMAIL],
        "expectedKeyVersion": 3,
        "serverClockOffsetMs": 120
    }))
    .expect("prepareKeyRotation args");
    assert_eq!(prepare.expected_key_version, 3);

    let commit: CommitKeyRotationArgs = serde_json::from_value(serde_json::json!({
        "accountId": acct(),
        "fingerprintHex": "AB"
    }))
    .expect("commitKeyRotation args");
    assert_eq!(commit.fingerprint_hex, "AB");

    let list: AccountScopedArgs =
        serde_json::from_value(serde_json::json!({ "accountId": acct() }))
            .expect("listAccountKeys args");
    assert_eq!(list.account_id, acct());

    let grant: AliasKeyGrantInput = serde_json::from_value(serde_json::json!({
        "aliasId": "alias-1",
        "keyVersion": 1,
        "aliasKeyFingerprintHex": "cd",
        "wrappedPrivateKeyArmored": "wrapped",
        "isCurrent": true,
        "memberKeyFingerprintHex": "ef"
    }))
    .expect("alias grant");
    assert_eq!(grant.member_key_fingerprint_hex.as_deref(), Some("ef"));
}

#[test]
fn the_responses_use_the_web_keystore_field_names() {
    let a = account();
    let loaded = load(&a.ks, vec![current(1, &a.first)]);
    for field in [
        "ok",
        "loaded",
        "failed",
        "unverified",
        "currentFingerprintHex",
        "keyVersion",
        "currentChanged",
    ] {
        assert!(loaded.get(field).is_some(), "loadAccountKeys lacks {field}");
    }

    let listed = serde_json::to_value(a.ks.list_account_keys(acct())).unwrap();
    assert_eq!(
        listed,
        serde_json::json!({
            "ok": true,
            "keys": [{ "fingerprintHex": a.first.fingerprint_hex, "keyVersion": 1, "isCurrent": true }],
            "keyVersion": 1
        })
    );

    let prepared = prepare(&a.ks);
    for field in [
        "ok",
        "publicKeyArmored",
        "encryptedPrivateKey",
        "fingerprintHex",
        "keyVersion",
        "transitions",
    ] {
        assert!(
            prepared.get(field).is_some(),
            "prepareKeyRotation lacks {field}"
        );
    }
    assert!(prepared["transitions"][0]["signature"].is_array());

    let mut events = a.ks.subscribe();
    let fp = prepared["fingerprintHex"].as_str().unwrap().to_owned();
    assert_eq!(
        commit(&a.ks, &fp),
        serde_json::json!({ "ok": true, "keyVersion": 2 })
    );
    let event = serde_json::to_value(events.try_recv().unwrap()).unwrap();
    assert_eq!(
        event,
        serde_json::json!({
            "type": "accountKeysChanged",
            "accountId": acct(),
            "fingerprints": [fp, a.first.fingerprint_hex],
            "currentFingerprintHex": fp,
            "keyVersion": 2,
            "currentChanged": true
        })
    );

    let listed = serde_json::to_value(a.ks.list_account_keys(acct())).unwrap();
    assert_eq!(listed["keys"][0]["isCurrent"], true);
    assert_eq!(listed["keys"][1]["keyVersion"], 1);
    assert_eq!(listed["keys"][1]["isCurrent"], false);
    assert_eq!(
        serde_json::to_value(Keystore::new().list_account_keys(acct())).unwrap(),
        serde_json::json!({ "ok": false, "code": "locked" })
    );
}

#[test]
fn a_key_adopted_after_a_lost_commit_clears_the_pending_rotation() {
    let a = account();
    load(&a.ks, vec![current(1, &a.first)]);
    let prepared = prepare(&a.ks);
    let fp = prepared["fingerprintHex"].as_str().unwrap().to_owned();
    let served = AccountKeyInput {
        key_version: 2,
        fingerprint_hex: fp.clone(),
        encrypted_private_key_armored: prepared["encryptedPrivateKey"].as_str().unwrap().to_owned(),
        is_current: true,
        transitions: prepared["transitions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| AccountKeyTransitionInput {
                previous_key_fingerprint_hex: t["previousKeyFingerprintHex"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
                signature: signature_bytes(&t["signature"]),
            })
            .collect(),
    };

    let loaded = load(&a.ks, vec![input(1, &a.first), served]);
    assert_eq!(loaded["currentChanged"], true);
    assert_eq!(loaded["currentFingerprintHex"], fp.as_str());
    assert_eq!(commit(&a.ks, &fp)["code"], "no_pending_rotation");
}

#[test]
fn an_srp_vault_cannot_rotate() {
    let r = registered();
    let device = Keystore::new();
    let unlocked = sign_in_as(
        &device,
        &r.server,
        &r.account,
        PASSWORD,
        &r.account.encrypted_private_key,
        Vec::new(),
        AuthScheme::SrpV1,
    );
    assert_eq!(unlocked["ok"], true, "{unlocked}");
    assert_eq!(load(&device, vec![])["code"], "not_opaque");
    assert_eq!(prepare(&device)["code"], "not_opaque");
}

fn copy(transitions: &[AccountKeyTransitionInput]) -> Vec<AccountKeyTransitionInput> {
    transitions
        .iter()
        .map(|t| AccountKeyTransitionInput {
            previous_key_fingerprint_hex: t.previous_key_fingerprint_hex.clone(),
            signature: t.signature.clone(),
        })
        .collect()
}

fn current_fingerprint(ks: &Keystore) -> (String, Option<i64>) {
    let (public, version) = current_public(ks);
    (fingerprint_of(&public), version)
}

fn assert_refused(ks: &Keystore, current_version: i64, keys: Vec<AccountKeyInput>, why: &str) {
    let before = current_fingerprint(ks);
    let listed = serde_json::to_value(ks.list_account_keys(acct())).unwrap();
    let mut events = ks.subscribe();
    assert_eq!(
        load_with(ks, current_version, keys),
        serde_json::json!({ "ok": false, "code": "key_list_invalid" }),
        "{why}"
    );
    assert!(events.try_recv().is_err(), "{why} broadcast");
    assert_eq!(current_fingerprint(ks), before, "{why}");
    assert_eq!(
        serde_json::to_value(ks.list_account_keys(acct())).unwrap(),
        listed,
        "{why}"
    );
}

#[test]
fn a_fresh_sign_in_never_promotes_a_replayed_retired_key() {
    let a = account();
    let second = generate(&a.pass, &[EMAIL]);
    let third = generate(&a.pass, &[EMAIL]);
    let real = history(&a.pass, &[&a.first, &second, &third]);

    let device = device_with(&a.pass, &third);
    let mut replayed = vec![input(1, &second), input(2, &third), current(3, &a.first)];
    replayed[1].transitions = copy(&real[2].transitions);
    let loaded = load(&device, replayed);
    assert_eq!(loaded["ok"], true, "{loaded}");
    assert_eq!(loaded["unverified"], serde_json::json!([1, 3]));
    assert_eq!(loaded["currentChanged"], false);
    assert_eq!(
        loaded["currentFingerprintHex"],
        third.fingerprint_hex.as_str()
    );
    assert_eq!(current_fingerprint(&device).0, third.fingerprint_hex);
}

#[test]
fn a_device_never_takes_a_known_key_at_another_version() {
    let a = account();
    let second = generate(&a.pass, &[EMAIL]);
    let third = generate(&a.pass, &[EMAIL]);
    let filler = generate(&a.pass, &[EMAIL]);
    let loaded = load(&a.ks, history(&a.pass, &[&a.first, &second, &third]));
    assert_eq!(loaded["keyVersion"], 3);

    let mut replayed = history(&a.pass, &[&filler, &second, &third, &a.first]);
    replayed[3].transitions = vec![endorsement(&a.pass, &third, &a.first.fingerprint_hex)];
    assert_refused(&a.ks, 4, replayed, "v1 replayed at version 4");
    assert_refused(
        &a.ks,
        4,
        vec![current(4, &a.first)],
        "v1 alone at version 4",
    );
    assert_refused(
        &a.ks,
        1,
        vec![current(1, &third)],
        "the current key moved to version 1",
    );
    assert_eq!(current_fingerprint(&a.ks), (third.fingerprint_hex, Some(3)));
}

#[test]
fn an_unknown_key_without_an_endorsement_from_the_key_below_is_unverified() {
    let a = account();
    let second = generate(&a.pass, &[EMAIL]);
    let third = generate(&a.pass, &[EMAIL]);
    let elsewhere = generate(&a.pass, &[EMAIL]);
    load(&a.ks, history(&a.pass, &[&a.first, &second]));

    let variants = [
        ("no endorsement", Vec::new()),
        (
            "endorsed only by v1",
            vec![endorsement(&a.pass, &a.first, &third.fingerprint_hex)],
        ),
        (
            "endorsement for another key",
            vec![AccountKeyTransitionInput {
                previous_key_fingerprint_hex: second.fingerprint_hex.clone(),
                ..endorsement(&a.pass, &second, &elsewhere.fingerprint_hex)
            }],
        ),
        (
            "endorsement for another account",
            vec![endorsement_for(
                "another-account",
                &a.pass,
                &second,
                &third.fingerprint_hex,
            )],
        ),
    ];
    for (why, transitions) in variants {
        let mut keys = history(&a.pass, &[&a.first, &second, &third]);
        keys[2].transitions = transitions;
        let loaded = load(&a.ks, keys);
        assert_eq!(loaded["ok"], true, "{why}: {loaded}");
        assert_eq!(loaded["unverified"], serde_json::json!([3]), "{why}");
        assert_eq!(loaded["currentChanged"], false, "{why}");
        assert_eq!(loaded["keyVersion"], 2, "{why}");
        assert_eq!(
            current_fingerprint(&a.ks),
            (second.fingerprint_hex.clone(), Some(2)),
            "{why}"
        );
    }

    let mail = encrypt_to(&a.ks, &third.public_key_armored, b"to an unverified key");
    assert_eq!(decrypt(&a.ks, &mail, None)["code"], "no_matching_key");
}

#[test]
fn a_device_offline_across_two_rotations_follows_the_chain() {
    let a = account();
    let second = generate(&a.pass, &[EMAIL]);
    let third = generate(&a.pass, &[EMAIL]);
    let fourth = generate(&a.pass, &[EMAIL]);
    load(&a.ks, history(&a.pass, &[&a.first, &second]));

    let mut events = a.ks.subscribe();
    let mut keys = history(&a.pass, &[&a.first, &second, &third, &fourth]);
    keys[3].transitions = vec![endorsement(&a.pass, &third, &fourth.fingerprint_hex)];
    let loaded = load(&a.ks, keys);
    assert_eq!(loaded["currentChanged"], true);
    assert_eq!(loaded["keyVersion"], 4);
    assert_eq!(loaded["unverified"], serde_json::json!([]));
    assert_eq!(
        loaded["loaded"],
        serde_json::json!([
            a.first.fingerprint_hex,
            second.fingerprint_hex,
            third.fingerprint_hex,
            fourth.fingerprint_hex
        ])
    );
    match events.try_recv().expect("keys event") {
        Broadcast::AccountKeysChanged {
            fingerprints,
            key_version,
            current_changed,
            ..
        } => {
            assert_eq!(key_version, 4);
            assert!(current_changed);
            assert_eq!(
                fingerprints,
                vec![
                    fourth.fingerprint_hex.clone(),
                    third.fingerprint_hex.clone(),
                    second.fingerprint_hex.clone(),
                    a.first.fingerprint_hex.clone()
                ]
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(events.try_recv().is_err());

    let mail = encrypt_to(&a.ks, &third.public_key_armored, b"sent while offline");
    assert_eq!(
        decrypt(&a.ks, &mail, None)["plaintext"],
        "sent while offline"
    );
    assert_eq!(prepare(&a.ks)["keyVersion"], 5);
}

#[test]
fn a_fresh_sign_in_climbs_the_chain_from_the_key_it_was_given() {
    let a = account();
    let second = generate(&a.pass, &[EMAIL]);
    let third = generate(&a.pass, &[EMAIL]);
    let keys = || history(&a.pass, &[&a.first, &second, &third]);

    let loaded = load(&a.ks, keys());
    assert_eq!(loaded["currentChanged"], true);
    assert_eq!(loaded["keyVersion"], 3);
    assert_eq!(
        loaded["currentFingerprintHex"],
        third.fingerprint_hex.as_str()
    );

    let device = device_with(&a.pass, &second);
    assert_eq!(current_public(&device).1, None);
    let loaded = load(&device, keys());
    assert_eq!(loaded["keyVersion"], 3);
    assert_eq!(
        current_fingerprint(&device),
        (third.fingerprint_hex.clone(), Some(3))
    );
    let listed = serde_json::to_value(device.list_account_keys(acct())).unwrap();
    assert_eq!(
        listed["keys"][1]["fingerprintHex"],
        second.fingerprint_hex.as_str()
    );
    assert_eq!(listed["keys"][1]["keyVersion"], 2);

    let device = device_with(&a.pass, &third);
    let loaded = load(&device, keys());
    assert_eq!(loaded["currentChanged"], false);
    assert_eq!(loaded["keyVersion"], 3);
}

#[test]
fn malformed_key_lists_are_refused_without_changing_anything() {
    let a = account();
    let second = generate(&a.pass, &[EMAIL]);
    let third = generate(&a.pass, &[EMAIL]);
    load(&a.ks, history(&a.pass, &[&a.first, &second]));
    let full = || history(&a.pass, &[&a.first, &second, &third]);

    let mut gap = full();
    gap.remove(1);
    assert_refused(&a.ks, 2, gap, "a gap at version 2");

    let mut two_current = full();
    two_current[1].is_current = true;
    assert_refused(&a.ks, 3, two_current, "two current keys");

    let mut none_current = full();
    none_current[2].is_current = false;
    assert_refused(&a.ks, 3, none_current, "no current key");

    let mut repeated = full();
    repeated[1].key_version = 3;
    assert_refused(&a.ks, 3, repeated, "a repeated version");

    assert_refused(&a.ks, 4, full(), "more versions than keys");
    assert_refused(&a.ks, 2, full(), "fewer versions than keys");
    assert_refused(&a.ks, 0, Vec::new(), "an empty list");

    let mut stale_current = full();
    stale_current[2].is_current = false;
    stale_current[0].is_current = true;
    assert_refused(&a.ks, 3, stale_current, "the current key is not the newest");

    let mut duplicate = full();
    duplicate[2].fingerprint_hex = second.fingerprint_hex.clone();
    assert_refused(&a.ks, 3, duplicate, "a duplicate fingerprint");

    let mut not_hex = full();
    not_hex[2].fingerprint_hex = "zz".repeat(20);
    assert_refused(&a.ks, 3, not_hex, "a malformed fingerprint");

    let mut self_endorsed = full();
    self_endorsed[2].transitions[0].previous_key_fingerprint_hex = third.fingerprint_hex.clone();
    assert_refused(&a.ks, 3, self_endorsed, "a key endorsing itself");

    let mut flooded = full();
    let endorsement = endorsement(&a.pass, &second, &third.fingerprint_hex);
    flooded[2].transitions = (0..17)
        .map(|_| AccountKeyTransitionInput {
            previous_key_fingerprint_hex: endorsement.previous_key_fingerprint_hex.clone(),
            signature: endorsement.signature.clone(),
        })
        .collect();
    assert_refused(&a.ks, 3, flooded, "more than sixteen endorsements");

    let loaded = load(&a.ks, full());
    assert_eq!(loaded["keyVersion"], 3);
}

#[test]
fn every_accepted_key_list_is_announced_once() {
    let a = account();
    let mut events = a.ks.subscribe();
    load(&a.ks, vec![current(1, &a.first)]);
    assert!(matches!(
        events.try_recv().expect("keys event"),
        Broadcast::AccountKeysChanged {
            current_changed: false,
            changed: true,
            key_version: 1,
            ..
        }
    ));
    let loaded = load(&a.ks, vec![current(1, &a.first)]);
    assert_eq!(loaded["currentChanged"], false);
    assert!(matches!(
        events.try_recv().expect("keys event"),
        Broadcast::AccountKeysChanged {
            current_changed: false,
            changed: false,
            key_version: 1,
            ..
        }
    ));
    assert!(events.try_recv().is_err());
}

#[test]
fn a_key_list_that_changes_nothing_is_not_marked_as_a_change() {
    let a = account();
    let second = generate(&a.pass, &[EMAIL]);
    let keys = || history(&a.pass, &[&a.first, &second]);
    load(&a.ks, keys());
    let mut events = a.ks.subscribe();
    for _ in 0..3 {
        assert_eq!(load(&a.ks, keys())["ok"], true);
        assert!(matches!(
            events.try_recv().expect("keys event"),
            Broadcast::AccountKeysChanged { changed: false, .. }
        ));
    }
    let event = serde_json::to_value(Broadcast::AccountKeysChanged {
        account_id: acct().to_owned(),
        fingerprints: Vec::new(),
        current_fingerprint_hex: String::new(),
        key_version: 2,
        current_changed: false,
        changed: true,
    })
    .unwrap();
    assert!(event.get("changed").is_none(), "{event}");
}

struct Registry {
    ks: Keystore,
    server: OpaqueServer,
    account: Registered,
    keys: Vec<Key>,
    transitions: Vec<Vec<AccountKeyTransitionInput>>,
}

fn registered() -> Registry {
    let server = OpaqueServer::start();
    let ks = Keystore::new();
    let account = register(&ks, &server, acct());
    let first = Key {
        fingerprint_hex: fingerprint_of(&account.public_key),
        encrypted_private_key_armored: account.encrypted_private_key.clone(),
        public_key_armored: account.public_key.clone(),
    };
    Registry {
        ks,
        server,
        account,
        keys: vec![first],
        transitions: vec![Vec::new()],
    }
}

impl Registry {
    fn prepare_next(&mut self) -> Key {
        let prepared = prepare(&self.ks);
        assert_eq!(prepared["ok"], true, "{prepared}");
        let key = Key {
            fingerprint_hex: prepared["fingerprintHex"].as_str().unwrap().to_owned(),
            encrypted_private_key_armored: prepared["encryptedPrivateKey"]
                .as_str()
                .unwrap()
                .to_owned(),
            public_key_armored: prepared["publicKeyArmored"].as_str().unwrap().to_owned(),
        };
        self.transitions.push(
            prepared["transitions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| AccountKeyTransitionInput {
                    previous_key_fingerprint_hex: t["previousKeyFingerprintHex"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                    signature: signature_bytes(&t["signature"]),
                })
                .collect(),
        );
        self.keys.push(key.clone());
        key
    }

    fn rotate(&mut self) -> Key {
        let key = self.prepare_next();
        assert_eq!(commit(&self.ks, &key.fingerprint_hex)["ok"], true);
        key
    }

    fn chain(&self, upto: usize) -> Vec<AccountKeyInput> {
        (0..upto)
            .map(|i| AccountKeyInput {
                is_current: i + 1 == upto,
                transitions: copy(&self.transitions[i]),
                ..input(i as i64 + 1, &self.keys[i])
            })
            .collect()
    }
}

fn signed_in_as(
    r: &Registry,
    key: &Key,
    stored: Vec<StoredAccountKey>,
    scheme: AuthScheme,
) -> Keystore {
    let device = Keystore::new();
    let unlocked = sign_in_as(
        &device,
        &r.server,
        &r.account,
        PASSWORD,
        &key.encrypted_private_key_armored,
        stored,
        scheme,
    );
    assert_eq!(unlocked["ok"], true, "sign-in unlocks: {unlocked}");
    device
}

fn signed_in(r: &Registry, key: &Key, stored: Vec<StoredAccountKey>) -> Keystore {
    signed_in_as(r, key, stored, AuthScheme::OpaqueV1)
}

fn stored_versions(ks: &Keystore) -> Vec<(i64, String, bool)> {
    ks.stored_account_keys(acct())
        .expect("stored keys")
        .into_iter()
        .map(|k| {
            (
                k.key_version,
                k.fingerprint_hex,
                k.encrypted_private_key_armored.is_some(),
            )
        })
        .collect()
}

#[test]
fn a_sign_in_keeps_the_stored_newest_key_current_when_the_server_replays_a_retired_one() {
    let mut r = registered();
    let first = r.keys[0].clone();
    let second = r.rotate();
    let third = r.rotate();
    let before = stored_versions(&r.ks);
    assert_eq!(
        before,
        vec![
            (3, third.fingerprint_hex.clone(), true),
            (2, second.fingerprint_hex.clone(), true),
            (1, first.fingerprint_hex.clone(), true),
        ]
    );

    let device = signed_in(&r, &first, r.ks.stored_account_keys(acct()).unwrap());
    assert_eq!(
        current_fingerprint(&device),
        (third.fingerprint_hex.clone(), Some(3))
    );

    let loaded = load(&device, vec![current(1, &first)]);
    assert_eq!(loaded["ok"], true, "{loaded}");
    assert_eq!(loaded["currentChanged"], false);
    assert_eq!(loaded["keyVersion"], 3);
    assert_eq!(
        current_fingerprint(&device),
        (third.fingerprint_hex.clone(), Some(3))
    );
    assert_eq!(stored_versions(&device), before);

    let mut replayed = vec![input(1, &second), input(2, &third), current(3, &first)];
    replayed[1].transitions = copy(&r.transitions[2]);
    assert_refused(&device, 3, replayed, "v1 relabelled above the stored v3");

    let mail = encrypt_to(&device, &second.public_key_armored, b"kept across sign-in");
    assert_eq!(
        decrypt(&device, &mail, None)["plaintext"],
        "kept across sign-in"
    );
}

#[test]
fn a_sign_in_with_a_newer_key_climbs_the_chain_from_the_stored_one() {
    let mut r = registered();
    let second = r.rotate();
    let stored = r.ks.stored_account_keys(acct()).unwrap();
    let third = r.prepare_next();
    r.ks.abandon_key_rotation(acct());

    let device = signed_in(&r, &third, stored);
    assert_eq!(
        current_fingerprint(&device),
        (second.fingerprint_hex.clone(), Some(2))
    );

    let mut unendorsed = r.chain(3);
    unendorsed[2].transitions = Vec::new();
    let loaded = load(&device, unendorsed);
    assert_eq!(loaded["unverified"], serde_json::json!([3]));
    assert_eq!(current_fingerprint(&device).0, second.fingerprint_hex);

    let loaded = load(&device, r.chain(3));
    assert_eq!(loaded["currentChanged"], true);
    assert_eq!(
        current_fingerprint(&device),
        (third.fingerprint_hex.clone(), Some(3))
    );
}

#[test]
fn a_sign_in_without_stored_keys_starts_from_the_key_it_was_given() {
    let r = registered();
    let first = r.keys[0].clone();
    let device = signed_in(&r, &first, Vec::new());
    assert_eq!(current_public(&device).1, None);
    assert!(device.stored_account_keys(acct()).unwrap().is_empty());
    let loaded = load(&device, vec![current(1, &first)]);
    assert_eq!(loaded["keyVersion"], 1);
    assert_eq!(
        stored_versions(&device),
        vec![(1, first.fingerprint_hex.clone(), true)]
    );
}

#[test]
fn a_version_learned_from_one_list_is_not_renumbered_by_a_later_list() {
    let a = account();
    let second = generate(&a.pass, &[EMAIL]);
    let third = generate(&a.pass, &[EMAIL]);
    let real = || history(&a.pass, &[&a.first, &second, &third]);

    let device = device_with(&a.pass, &third);
    let loaded = load(&device, vec![input(1, &a.first), current(2, &third)]);
    assert_eq!(loaded["keyVersion"], 2, "{loaded}");

    assert_refused(&device, 3, real(), "a learned version renumbered");
    assert_eq!(
        current_fingerprint(&device),
        (third.fingerprint_hex.clone(), Some(2))
    );
    assert_refused(
        &device,
        3,
        vec![input(1, &third), input(2, &second), current(3, &a.first)],
        "the order of known keys reversed",
    );
}

#[test]
fn a_version_this_device_rotated_to_never_moves() {
    let a = account();
    load(&a.ks, vec![current(1, &a.first)]);
    let prepared = rotate(&a.ks);
    let rotated = prepared["fingerprintHex"].as_str().unwrap().to_owned();
    let rotated_input = AccountKeyInput {
        key_version: 3,
        fingerprint_hex: rotated.clone(),
        encrypted_private_key_armored: prepared["encryptedPrivateKey"].as_str().unwrap().to_owned(),
        is_current: true,
        transitions: Vec::new(),
    };
    let filler = generate(&a.pass, &[EMAIL]);
    assert_refused(
        &a.ks,
        3,
        vec![input(1, &a.first), input(2, &filler), rotated_input],
        "the rotated key moved to version 3",
    );
    assert_eq!(current_fingerprint(&a.ks), (rotated, Some(2)));
}

fn scheme_of(ks: &Keystore) -> Option<AuthScheme> {
    ks.status()
        .accounts
        .into_iter()
        .find(|a| a.account_id == acct())
        .map(|a| a.auth_scheme)
}

fn finalized(ks: &Keystore, stored: Vec<StoredAccountKey>) -> Value {
    serde_json::to_value(ks.migration_finalized(acct(), stored)).unwrap()
}

#[test]
fn a_staged_sign_in_stays_on_the_password_scheme_until_finalized() {
    let mut r = registered();
    let first = r.keys[0].clone();
    let second = r.rotate();
    let stored = r.ks.stored_account_keys(acct()).unwrap();

    let device = signed_in_as(&r, &first, stored.clone(), AuthScheme::SrpV1);
    assert_eq!(scheme_of(&device), Some(AuthScheme::SrpV1));
    assert!(device.stored_account_keys(acct()).is_none());

    assert_eq!(
        finalized(&device, stored),
        serde_json::json!({ "ok": true })
    );
    assert_eq!(scheme_of(&device), Some(AuthScheme::OpaqueV1));
    assert_eq!(
        current_fingerprint(&device),
        (second.fingerprint_hex.clone(), Some(2))
    );
}

#[test]
fn finalizing_an_opaque_vault_again_is_refused() {
    let r = registered();
    let first = r.keys[0].clone();
    let device = signed_in(&r, &first, Vec::new());

    assert_eq!(
        finalized(&device, Vec::new()),
        serde_json::json!({ "ok": false, "code": "not_staged" })
    );
}

#[test]
fn finalizing_a_locked_account_is_refused() {
    let ks = Keystore::new();

    assert_eq!(
        finalized(&ks, Vec::new()),
        serde_json::json!({ "ok": false, "code": "locked" })
    );
}
