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
        fingerprint_hex: key.fingerprint_hex.clone(),
        encrypted_private_key_armored: key.encrypted_private_key_armored.clone(),
        is_current: false,
        transitions: Vec::new(),
    }
}

fn current(version: i64, key: &GeneratedKey) -> AccountKeyInput {
    AccountKeyInput {
        is_current: true,
        ..input(version, key)
    }
}

fn endorsement(amk: &Amk, by: &GeneratedKey, new_fp: &str) -> AccountKeyTransitionInput {
    endorsement_for(ACCOUNT, amk, by, new_fp)
}

fn endorsement_for(
    account_id: &str,
    amk: &Amk,
    by: &GeneratedKey,
    new_fp: &str,
) -> AccountKeyTransitionInput {
    let passphrase = derive_pgp_passphrase(amk);
    let signer =
        UnlockedKey::open(&by.encrypted_private_key_armored, passphrase.expose()).expect("open");
    AccountKeyTransitionInput {
        previous_key_fingerprint_hex: by.fingerprint_hex.clone(),
        signature: keytransition::endorse(&signer, account_id, new_fp).expect("endorse"),
    }
}

fn history(amk: &Amk, keys: &[&GeneratedKey]) -> Vec<AccountKeyInput> {
    keys.iter()
        .enumerate()
        .map(|(i, key)| AccountKeyInput {
            is_current: i + 1 == keys.len(),
            transitions: keys[..i]
                .iter()
                .rev()
                .take(16)
                .map(|by| endorsement(amk, by, &key.fingerprint_hex))
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
        account_id: ACCOUNT.to_owned(),
        current_version,
        keys,
    }))
    .expect("json")
}

fn prepare_with(ks: &Keystore, expected_key_version: i64, emails: &[&str]) -> Value {
    serde_json::to_value(ks.prepare_key_rotation(
        PrepareKeyRotationArgs {
            account_id: ACCOUNT.to_owned(),
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
        account_id: ACCOUNT.to_owned(),
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
            assert_eq!(account_id, ACCOUNT);
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
    assert_eq!(
        thelemail_crypto::openpgp::public_key_fingerprint_hex(&public).unwrap(),
        new_fp
    );

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
        a.ks.decrypt_bytes(ACCOUNT, &old_mail, None).unwrap(),
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

    let older = generate(&a.amk, &[EMAIL]);
    let oldest = generate(&a.amk, &[EMAIL]);
    let stranger = generate(&Amk::generate(), &[EMAIL]);
    let mut locked_out = history(&a.amk, &[&oldest, &older, &a.first]);
    locked_out[0].encrypted_private_key_armored = stranger.encrypted_private_key_armored.clone();
    let loaded = load(&a.ks, locked_out);
    assert_eq!(loaded["keyVersion"], 3);
    assert_eq!(loaded["failed"], serde_json::json!([1]));
    assert_eq!(prepare(&a.ks)["code"], "key_history_incomplete");

    load(&a.ks, history(&a.amk, &[&oldest, &older, &a.first]));
    let prepared = prepare(&a.ks);
    assert_eq!(prepared["ok"], true, "{prepared}");
    assert_eq!(prepared["transitions"].as_array().unwrap().len(), 3);
}

#[test]
fn loading_keys_adopts_an_endorsed_newer_key_and_never_moves_back() {
    let a = account();
    let newer = generate(&a.amk, &[EMAIL]);

    let loaded = load(&a.ks, history(&a.amk, &[&a.first, &newer]));
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
    let stranger = generate(&Amk::generate(), &[EMAIL]);
    let locked = generate(&a.amk, &[EMAIL]);
    let second = generate(&a.amk, &[EMAIL]);
    let third = generate(&a.amk, &[EMAIL]);
    let fourth = generate(&a.amk, &[EMAIL]);
    let mut keys = history(&a.amk, &[&a.first, &locked, &second, &third, &fourth]);
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
fn a_key_list_without_the_current_key_is_refused() {
    let a = account();
    let other = generate(&a.amk, &[EMAIL]);
    let mut events = a.ks.subscribe();
    let refused = load(&a.ks, vec![current(1, &other)]);
    assert_eq!(
        refused,
        serde_json::json!({ "ok": false, "code": "key_list_invalid" })
    );
    assert!(events.try_recv().is_err());

    load(&a.ks, vec![current(1, &a.first)]);
    let mut events = a.ks.subscribe();
    let swapped = load(&a.ks, vec![current(1, &other)]);
    assert_eq!(swapped["code"], "key_list_invalid");
    assert!(events.try_recv().is_err());
    let (public, version) = current_public(&a.ks);
    assert_eq!(version, Some(1));
    assert_eq!(
        thelemail_crypto::openpgp::public_key_fingerprint_hex(&public).unwrap(),
        a.first.fingerprint_hex
    );
}

#[test]
fn an_alias_grant_wrapped_to_the_retired_key_still_unwraps() {
    let a = account();
    load(&a.ks, vec![current(1, &a.first)]);
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

    rotate(&a.ks);

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
    let retired = generate(&a.amk, &[EMAIL]);
    let created = serde_json::to_value(a.ks.create_alias_key(
        CreateAliasKeyArgs {
            account_id: ACCOUNT.to_owned(),
            email: "team@thelemail.test".to_owned(),
            display_name: "Team".to_owned(),
            recipients: vec![CreateAliasKeyRecipient {
                account_id: ACCOUNT.to_owned(),
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
        account_id: ACCOUNT.to_owned(),
        grants: vec![grant()],
    }))
    .unwrap();
    assert_eq!(early["loaded"], serde_json::json!([]));
    assert_eq!(early["failed"][0]["aliasId"], "alias-1");
    let alias_mail = encrypt_to(&a.ks, &alias_public, b"to the team");
    assert_eq!(decrypt(&a.ks, &alias_mail, None)["code"], "no_matching_key");

    load(&a.ks, history(&a.amk, &[&retired, &a.first]));
    assert_eq!(
        decrypt(&a.ks, &alias_mail, Some(&alias_fp))["plaintext"],
        "to the team"
    );

    a.ks.unload_alias_keys(ACCOUNT);
    let fresh = Account {
        ks: Keystore::new(),
        amk: a.amk,
        first: a.first,
    };
    assert!(
        fresh
            .ks
            .adopt_persisted(ACCOUNT, persisted_for(&fresh.amk, &fresh.first))
    );
    fresh.ks.load_alias_keys(LoadAliasKeysArgs {
        account_id: ACCOUNT.to_owned(),
        grants: vec![grant()],
    });
    fresh.ks.unload_alias_keys(ACCOUNT);
    load(&fresh.ks, history(&fresh.amk, &[&retired, &fresh.first]));
    assert_eq!(
        decrypt(&fresh.ks, &alias_mail, None)["code"],
        "no_matching_key",
        "unloading alias keys also drops the grants waiting for a key"
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
    load(&a.ks, vec![current(1, &a.first)]);
    let old_mail = encrypt_to(&a.ks, &a.first.public_key_armored, b"before the rotation");
    rotate(&a.ks);

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
    assert_eq!(
        a.ks.decrypt_bytes(ACCOUNT, &mail, None),
        Err("no_matching_key")
    );
    assert_eq!(
        a.ks.decrypt_bytes(ACCOUNT, b"not a pgp message", None),
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

    a.ks.abandon_key_rotation(ACCOUNT);
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

    let elsewhere = generate(&a.amk, &[EMAIL]);
    let loaded = load(&a.ks, history(&a.amk, &[&a.first, &elsewhere]));
    assert_eq!(loaded["keyVersion"], 2);

    assert_eq!(commit(&a.ks, &fp)["code"], "no_pending_rotation");
    let (public, version) = current_public(&a.ks);
    assert_eq!(version, Some(2));
    assert_eq!(
        thelemail_crypto::openpgp::public_key_fingerprint_hex(&public).unwrap(),
        elsewhere.fingerprint_hex
    );
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
    let older: Vec<GeneratedKey> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..17)
            .map(|_| scope.spawn(|| generate(&a.amk, &[EMAIL])))
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
        let message = keytransition::payload(ACCOUNT, &new_fp, &key.fingerprint_hex).unwrap();
        keytransition::verify(
            message.as_bytes(),
            &signature_bytes(&transition["signature"]),
            &key.public_key_armored,
        )
        .expect("each endorsement verifies against its own key");
    }
}

#[test]
fn the_web_keystore_payloads_deserialize() {
    let load: LoadAccountKeysArgs = serde_json::from_value(serde_json::json!({
        "accountId": ACCOUNT,
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
        "accountId": ACCOUNT,
        "emails": [EMAIL],
        "expectedKeyVersion": 3,
        "serverClockOffsetMs": 120
    }))
    .expect("prepareKeyRotation args");
    assert_eq!(prepare.expected_key_version, 3);

    let commit: CommitKeyRotationArgs = serde_json::from_value(serde_json::json!({
        "accountId": ACCOUNT,
        "fingerprintHex": "AB"
    }))
    .expect("commitKeyRotation args");
    assert_eq!(commit.fingerprint_hex, "AB");

    let list: AccountScopedArgs =
        serde_json::from_value(serde_json::json!({ "accountId": ACCOUNT }))
            .expect("listAccountKeys args");
    assert_eq!(list.account_id, ACCOUNT);

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

    let listed = serde_json::to_value(a.ks.list_account_keys(ACCOUNT)).unwrap();
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
            "accountId": ACCOUNT,
            "fingerprints": [fp, a.first.fingerprint_hex],
            "currentFingerprintHex": fp,
            "keyVersion": 2,
            "currentChanged": true
        })
    );

    let listed = serde_json::to_value(a.ks.list_account_keys(ACCOUNT)).unwrap();
    assert_eq!(listed["keys"][0]["isCurrent"], true);
    assert_eq!(listed["keys"][1]["keyVersion"], 1);
    assert_eq!(listed["keys"][1]["isCurrent"], false);
    assert_eq!(
        serde_json::to_value(Keystore::new().list_account_keys(ACCOUNT)).unwrap(),
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
    let amk = Amk::generate();
    let key = generate(&amk, &[EMAIL]);
    let mut persisted = persisted_for(&amk, &key);
    persisted.auth_scheme = AuthScheme::SrpV1;
    let ks = Keystore::new();
    assert!(ks.adopt_persisted(ACCOUNT, persisted));
    assert_eq!(load(&ks, vec![])["code"], "not_opaque");
    assert_eq!(prepare(&ks)["code"], "not_opaque");
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
    (
        thelemail_crypto::openpgp::public_key_fingerprint_hex(&public).unwrap(),
        version,
    )
}

fn assert_refused(ks: &Keystore, current_version: i64, keys: Vec<AccountKeyInput>, why: &str) {
    let before = current_fingerprint(ks);
    let listed = serde_json::to_value(ks.list_account_keys(ACCOUNT)).unwrap();
    let mut events = ks.subscribe();
    assert_eq!(
        load_with(ks, current_version, keys),
        serde_json::json!({ "ok": false, "code": "key_list_invalid" }),
        "{why}"
    );
    assert!(events.try_recv().is_err(), "{why} broadcast");
    assert_eq!(current_fingerprint(ks), before, "{why}");
    assert_eq!(
        serde_json::to_value(ks.list_account_keys(ACCOUNT)).unwrap(),
        listed,
        "{why}"
    );
}

#[test]
fn a_fresh_sign_in_never_promotes_a_replayed_retired_key() {
    let a = account();
    let second = generate(&a.amk, &[EMAIL]);
    let third = generate(&a.amk, &[EMAIL]);
    let real = history(&a.amk, &[&a.first, &second, &third]);

    let device = Keystore::new();
    assert!(device.adopt_persisted(ACCOUNT, persisted_for(&a.amk, &third)));
    let mut replayed = vec![input(1, &second), input(2, &third), current(3, &a.first)];
    replayed[1].transitions = copy(&real[2].transitions);
    let loaded = load(&device, replayed);
    assert_eq!(loaded["ok"], true, "{loaded}");
    assert_eq!(loaded["unverified"], serde_json::json!([3]));
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
    let second = generate(&a.amk, &[EMAIL]);
    let third = generate(&a.amk, &[EMAIL]);
    let filler = generate(&a.amk, &[EMAIL]);
    let loaded = load(&a.ks, history(&a.amk, &[&a.first, &second, &third]));
    assert_eq!(loaded["keyVersion"], 3);

    let mut replayed = history(&a.amk, &[&filler, &second, &third, &a.first]);
    replayed[3].transitions = vec![endorsement(&a.amk, &third, &a.first.fingerprint_hex)];
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
    let second = generate(&a.amk, &[EMAIL]);
    let third = generate(&a.amk, &[EMAIL]);
    let elsewhere = generate(&a.amk, &[EMAIL]);
    load(&a.ks, history(&a.amk, &[&a.first, &second]));

    let variants = [
        ("no endorsement", Vec::new()),
        (
            "endorsed only by v1",
            vec![endorsement(&a.amk, &a.first, &third.fingerprint_hex)],
        ),
        (
            "endorsement for another key",
            vec![AccountKeyTransitionInput {
                previous_key_fingerprint_hex: second.fingerprint_hex.clone(),
                ..endorsement(&a.amk, &second, &elsewhere.fingerprint_hex)
            }],
        ),
        (
            "endorsement for another account",
            vec![endorsement_for(
                "another-account",
                &a.amk,
                &second,
                &third.fingerprint_hex,
            )],
        ),
    ];
    for (why, transitions) in variants {
        let mut keys = history(&a.amk, &[&a.first, &second, &third]);
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
    let second = generate(&a.amk, &[EMAIL]);
    let third = generate(&a.amk, &[EMAIL]);
    let fourth = generate(&a.amk, &[EMAIL]);
    load(&a.ks, history(&a.amk, &[&a.first, &second]));

    let mut events = a.ks.subscribe();
    let mut keys = history(&a.amk, &[&a.first, &second, &third, &fourth]);
    keys[3].transitions = vec![endorsement(&a.amk, &third, &fourth.fingerprint_hex)];
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
    let second = generate(&a.amk, &[EMAIL]);
    let third = generate(&a.amk, &[EMAIL]);
    let keys = || history(&a.amk, &[&a.first, &second, &third]);

    let loaded = load(&a.ks, keys());
    assert_eq!(loaded["currentChanged"], true);
    assert_eq!(loaded["keyVersion"], 3);
    assert_eq!(
        loaded["currentFingerprintHex"],
        third.fingerprint_hex.as_str()
    );

    let device = Keystore::new();
    assert!(device.adopt_persisted(ACCOUNT, persisted_for(&a.amk, &second)));
    assert_eq!(current_public(&device).1, None);
    let loaded = load(&device, keys());
    assert_eq!(loaded["keyVersion"], 3);
    assert_eq!(
        current_fingerprint(&device),
        (third.fingerprint_hex.clone(), Some(3))
    );
    let listed = serde_json::to_value(device.list_account_keys(ACCOUNT)).unwrap();
    assert_eq!(
        listed["keys"][1]["fingerprintHex"],
        second.fingerprint_hex.as_str()
    );
    assert_eq!(listed["keys"][1]["keyVersion"], 2);

    let device = Keystore::new();
    assert!(device.adopt_persisted(ACCOUNT, persisted_for(&a.amk, &third)));
    let loaded = load(&device, keys());
    assert_eq!(loaded["currentChanged"], false);
    assert_eq!(loaded["keyVersion"], 3);
}

#[test]
fn malformed_key_lists_are_refused_without_changing_anything() {
    let a = account();
    let second = generate(&a.amk, &[EMAIL]);
    let third = generate(&a.amk, &[EMAIL]);
    load(&a.ks, history(&a.amk, &[&a.first, &second]));
    let full = || history(&a.amk, &[&a.first, &second, &third]);

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
    let endorsement = endorsement(&a.amk, &second, &third.fingerprint_hex);
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
    let second = generate(&a.amk, &[EMAIL]);
    let keys = || history(&a.amk, &[&a.first, &second]);
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
        account_id: ACCOUNT.to_owned(),
        fingerprints: Vec::new(),
        current_fingerprint_hex: String::new(),
        key_version: 2,
        current_changed: false,
        changed: true,
    })
    .unwrap();
    assert!(event.get("changed").is_none(), "{event}");
}

const PASSWORD: &str = "a long and sturdy passphrase";

fn sign_in(amk: &Amk, key: &GeneratedKey, stored: Vec<StoredAccountKey>) -> Keystore {
    use opaque_ke::{
        CredentialRequest, Identifiers, RegistrationRequest, RegistrationUpload, ServerLogin,
        ServerLoginParameters, ServerRegistration, ServerSetup,
    };
    use thelemail_crypto::amk::{derive_master_key_id, wrap_master_key};
    use thelemail_crypto::opaque::{ThelemailSuite, finish_registration, start_registration};

    let b64 = base64::engine::general_purpose::STANDARD;
    let mut rng = rand::rngs::OsRng;
    let identity = format!("thelemail/auth/opaque/v1:{ACCOUNT}");
    let identifiers = || Identifiers {
        client: Some(identity.as_bytes()),
        server: Some(b"thelemail.com"),
    };
    let setup = ServerSetup::<ThelemailSuite>::new(&mut rng);
    let registration = start_registration(PASSWORD).expect("registration start");
    let response = ServerRegistration::<ThelemailSuite>::start(
        &setup,
        RegistrationRequest::deserialize(&registration.request).expect("request"),
        ACCOUNT.as_bytes(),
    )
    .expect("server registration");
    let registered = finish_registration(
        registration.state,
        PASSWORD,
        &response.message.serialize(),
        &identity,
        "thelemail.com",
    )
    .expect("registration finish");
    let record = ServerRegistration::<ThelemailSuite>::finish(
        RegistrationUpload::deserialize(&registered.record).expect("upload"),
    );

    let ks = Keystore::new();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let started = runtime
        .block_on(ks.opaque_start_auth(OpaqueStartAuthArgs {
            password: PASSWORD.to_owned(),
            email: Some(EMAIL.to_owned()),
            recovery: false,
        }))
        .expect("auth start");
    let ke1 = b64.decode(&started.ke1).expect("ke1");
    let ke2 = ServerLogin::start(
        &mut rng,
        &setup,
        Some(record),
        CredentialRequest::deserialize(&ke1).expect("credential request"),
        ACCOUNT.as_bytes(),
        ServerLoginParameters {
            context: None,
            identifiers: identifiers(),
        },
    )
    .expect("server login")
    .message
    .serialize();
    let finished = runtime.block_on(ks.opaque_finish_auth(OpaqueFinishAuthArgs {
        operation_id: started.operation_id.clone(),
        account_id: ACCOUNT.to_owned(),
        ke2: b64.encode(ke2),
        recovery: false,
    }));
    assert!(matches!(finished, OpaqueFinishAuthResponse::Ok { .. }));

    let unlocked = ks.opaque_complete_login_unlock(
        OpaqueCompleteLoginUnlockArgs {
            operation_id: started.operation_id,
            account_id: ACCOUNT.to_owned(),
            encrypted_private_key: key.encrypted_private_key_armored.clone(),
            wrapped_master_key: b64.encode(wrap_master_key(&registered.export_key, amk, false)),
            master_key_id: b64.encode(derive_master_key_id(amk)),
            opaque_params_version: 1,
            server_auth_scheme: AuthScheme::OpaqueV1,
        },
        stored,
    );
    assert!(
        matches!(unlocked, OpaqueCompleteLoginUnlockResponse::Ok { .. }),
        "sign-in unlocks"
    );
    ks
}

fn stored_versions(ks: &Keystore) -> Vec<(i64, String, bool)> {
    ks.stored_account_keys(ACCOUNT)
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
    let a = account();
    let second = generate(&a.amk, &[EMAIL]);
    let third = generate(&a.amk, &[EMAIL]);
    load(&a.ks, history(&a.amk, &[&a.first, &second, &third]));
    assert_eq!(
        stored_versions(&a.ks),
        vec![
            (3, third.fingerprint_hex.clone(), true),
            (2, second.fingerprint_hex.clone(), true),
            (1, a.first.fingerprint_hex.clone(), true),
        ]
    );

    let device = sign_in(&a.amk, &a.first, a.ks.stored_account_keys(ACCOUNT).unwrap());
    assert_eq!(
        current_fingerprint(&device),
        (third.fingerprint_hex.clone(), Some(3))
    );

    let loaded = load(&device, vec![current(1, &a.first)]);
    assert_eq!(loaded["ok"], true, "{loaded}");
    assert_eq!(loaded["currentChanged"], false);
    assert_eq!(loaded["keyVersion"], 3);
    assert_eq!(
        current_fingerprint(&device),
        (third.fingerprint_hex.clone(), Some(3))
    );
    assert_eq!(stored_versions(&device), stored_versions(&a.ks));

    let mut replayed = history(&a.amk, &[&second, &third, &a.first]);
    replayed[2].transitions = vec![endorsement(&a.amk, &third, &a.first.fingerprint_hex)];
    assert_refused(&device, 3, replayed, "v1 relabelled above the stored v3");

    let mail = encrypt_to(&device, &second.public_key_armored, b"kept across sign-in");
    assert_eq!(
        decrypt(&device, &mail, None)["plaintext"],
        "kept across sign-in"
    );
}

#[test]
fn a_sign_in_with_a_newer_key_climbs_the_chain_from_the_stored_one() {
    let a = account();
    let second = generate(&a.amk, &[EMAIL]);
    let third = generate(&a.amk, &[EMAIL]);
    load(&a.ks, history(&a.amk, &[&a.first, &second]));

    let device = sign_in(&a.amk, &third, a.ks.stored_account_keys(ACCOUNT).unwrap());
    assert_eq!(
        current_fingerprint(&device),
        (second.fingerprint_hex.clone(), Some(2))
    );

    let mut unendorsed = history(&a.amk, &[&a.first, &second, &third]);
    unendorsed[2].transitions = Vec::new();
    let loaded = load(&device, unendorsed);
    assert_eq!(loaded["unverified"], serde_json::json!([3]));
    assert_eq!(current_fingerprint(&device).0, second.fingerprint_hex);

    let loaded = load(&device, history(&a.amk, &[&a.first, &second, &third]));
    assert_eq!(loaded["currentChanged"], true);
    assert_eq!(
        current_fingerprint(&device),
        (third.fingerprint_hex.clone(), Some(3))
    );
}

#[test]
fn a_sign_in_without_stored_keys_starts_from_the_key_it_was_given() {
    let a = account();
    let device = sign_in(&a.amk, &a.first, Vec::new());
    assert_eq!(current_public(&device).1, None);
    assert!(device.stored_account_keys(ACCOUNT).unwrap().is_empty());
    let loaded = load(&device, vec![current(1, &a.first)]);
    assert_eq!(loaded["keyVersion"], 1);
    assert_eq!(
        stored_versions(&device),
        vec![(1, a.first.fingerprint_hex.clone(), true)]
    );
}

#[test]
fn a_version_learned_from_one_list_follows_a_later_list_in_the_same_order() {
    let a = account();
    let second = generate(&a.amk, &[EMAIL]);
    let third = generate(&a.amk, &[EMAIL]);
    let real = || history(&a.amk, &[&a.first, &second, &third]);

    let device = Keystore::new();
    assert!(device.adopt_persisted(ACCOUNT, persisted_for(&a.amk, &third)));
    let loaded = load(&device, vec![input(1, &a.first), current(2, &third)]);
    assert_eq!(loaded["keyVersion"], 2, "{loaded}");

    let loaded = load(&device, real());
    assert_eq!(loaded["ok"], true, "{loaded}");
    assert_eq!(loaded["keyVersion"], 3);
    assert_eq!(loaded["currentChanged"], false);
    assert_eq!(
        current_fingerprint(&device),
        (third.fingerprint_hex.clone(), Some(3))
    );
    assert_eq!(
        stored_versions(&device),
        vec![
            (3, third.fingerprint_hex.clone(), true),
            (2, second.fingerprint_hex.clone(), true),
            (1, a.first.fingerprint_hex.clone(), true),
        ]
    );
    assert_eq!(prepare(&device)["keyVersion"], 4);

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
    let filler = generate(&a.amk, &[EMAIL]);
    assert_refused(
        &a.ks,
        3,
        vec![input(1, &a.first), input(2, &filler), rotated_input],
        "the rotated key moved to version 3",
    );
    assert_eq!(current_fingerprint(&a.ks), (rotated, Some(2)));
}
