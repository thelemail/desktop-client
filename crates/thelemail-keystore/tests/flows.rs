mod support;

use base64::Engine as _;
use serde_json::{Value, json};
use support::*;
use thelemail_keystore::*;

fn json_of(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn encrypt(ks: &Keystore, account_id: &str, to: &str, sign: bool, alias_id: Option<&str>) -> Value {
    json_of(ks.encrypt(EncryptArgs {
        account_id: account_id.to_owned(),
        recipient_public_key_armored: to.to_owned(),
        plaintext: b"hello from the desktop".to_vec(),
        sign_with_vault_key: sign,
        alias_id: alias_id.map(str::to_owned),
    }))
}

fn ciphertext(value: &Value) -> Vec<u8> {
    value["ciphertext"]
        .as_array()
        .unwrap_or_else(|| panic!("{value}"))
        .iter()
        .map(|b| b.as_u64().unwrap() as u8)
        .collect()
}

fn decrypt(
    ks: &Keystore,
    account_id: &str,
    ciphertext: Vec<u8>,
    senders: Option<Vec<String>>,
) -> Value {
    json_of(ks.decrypt(DecryptArgs {
        account_id: account_id.to_owned(),
        ciphertext_armored: None,
        ciphertext_binary: Some(ciphertext),
        binary: false,
        verification_keys_armored: senders,
        key_fingerprint_hex: None,
    }))
}

#[test]
fn a_registered_account_signs_in_again_and_reads_its_mail() {
    let server = OpaqueServer::start();
    let ks = Keystore::new();
    let account_id = new_account_id();
    let account = register(&ks, &server, &account_id);
    assert_eq!(
        json_of(ks.status())["accounts"][0]["accountId"],
        account_id.as_str()
    );

    let signed = ciphertext(&encrypt(&ks, &account_id, &account.public_key, true, None));
    let unsigned = ciphertext(&encrypt(&ks, &account_id, &account.public_key, false, None));
    let own = vec![account.public_key.clone()];

    let read = decrypt(&ks, &account_id, signed.clone(), Some(own.clone()));
    assert_eq!(read["plaintext"], "hello from the desktop");
    assert_eq!(read["signature"]["state"], "valid", "{read}");
    assert!(read["signature"]["signedAtMillis"].as_i64().unwrap() > 0);
    let fingerprint = hex_of(&json_of(ks.get_public_key(&account_id))["fingerprint"]);
    assert_eq!(read["signature"]["keyFingerprintHex"], fingerprint.as_str());
    assert_eq!(
        decrypt(&ks, &account_id, unsigned.clone(), Some(own.clone()))["signature"]["state"],
        "none"
    );
    assert!(decrypt(&ks, &account_id, signed.clone(), None)["signature"].is_null());

    let stored = ks.stored_account_keys(&account_id).expect("stored keys");
    assert_eq!(stored.len(), 1);
    assert!(stored[0].fixed);

    ks.lock(&account_id);
    assert_eq!(
        decrypt(&ks, &account_id, signed.clone(), None)["code"],
        "locked"
    );

    let wrong = sign_in_as(
        &ks,
        &server,
        &account,
        "not the password",
        &account.encrypted_private_key,
        Vec::new(),
        AuthScheme::OpaqueV1,
    );
    assert_eq!(wrong["code"], "invalid_credentials", "{wrong}");

    let unlocked = sign_in(
        &ks,
        &server,
        &account,
        &account.encrypted_private_key,
        stored,
    );
    assert_eq!(unlocked["ok"], true, "{unlocked}");
    assert_eq!(unlocked["email"], EMAIL);
    assert_eq!(
        decrypt(&ks, &account_id, unsigned, None)["plaintext"],
        "hello from the desktop"
    );
    assert_eq!(json_of(ks.get_public_key(&account_id))["keyVersion"], 1);
    ks.clear(&account_id);
}

fn hex_of(bytes: &Value) -> String {
    bytes
        .as_array()
        .unwrap()
        .iter()
        .map(|b| format!("{:02x}", b.as_u64().unwrap()))
        .collect()
}

#[test]
fn a_wrong_master_key_id_is_reported_as_a_mismatch() {
    let server = OpaqueServer::start();
    let ks = Keystore::new();
    let account_id = new_account_id();
    let mut account = register(&ks, &server, &account_id);
    ks.lock(&account_id);
    account.master_key_id = b64().encode([0u8; 32]);
    let refused = sign_in(
        &ks,
        &server,
        &account,
        &account.encrypted_private_key,
        Vec::new(),
    );
    assert_eq!(refused["code"], "master_key_mismatch", "{refused}");
    assert!(
        json_of(ks.status())["accounts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_password_change_keeps_the_master_key_and_the_new_password_signs_in() {
    let server = OpaqueServer::start();
    let ks = Keystore::new();
    let account_id = new_account_id();
    let mut account = register(&ks, &server, &account_id);
    let rt = runtime();
    let new_password = "an entirely different passphrase";

    let start = json_of(rt.block_on(ks.opaque_password_change_start(
        OpaquePasswordChangeStartArgs {
            account_id: account_id.clone(),
            new_password: new_password.to_owned(),
        },
    )));
    assert_eq!(start["ok"], true, "{start}");
    let operation_id = start["operationId"].as_str().unwrap().to_owned();
    let response = server.register(
        &account_id,
        start["registrationRequest"].as_str().unwrap(),
        false,
    );
    let finished = json_of(rt.block_on(ks.opaque_password_change_finish(
        OpaquePasswordChangeFinishArgs {
            account_id: account_id.clone(),
            operation_id: operation_id.clone(),
            registration_response: response,
        },
    )));
    assert_eq!(finished["ok"], true, "{finished}");
    assert_eq!(finished["masterKeyId"], account.master_key_id.as_str());
    let committed = json_of(
        ks.opaque_password_change_commit(OpaquePasswordChangeCommitArgs {
            account_id: account_id.clone(),
            operation_id,
        }),
    );
    assert_eq!(committed, json!({"ok": true, "persisted": false}));

    account.record = finished["opaqueRecord"].as_str().unwrap().to_owned();
    account.wrapped_master_key = finished["wrappedMasterKey"].as_str().unwrap().to_owned();
    ks.lock(&account_id);
    let unlocked = sign_in_as(
        &ks,
        &server,
        &account,
        new_password,
        &account.encrypted_private_key,
        Vec::new(),
        AuthScheme::OpaqueV1,
    );
    assert_eq!(unlocked["ok"], true, "{unlocked}");
    ks.clear(&account_id);
}

#[test]
fn a_recovery_phrase_resets_the_password_without_a_new_master_key() {
    let server = OpaqueServer::start();
    let ks = Keystore::new();
    let account_id = new_account_id();
    let mut account = register(&ks, &server, &account_id);
    let rt = runtime();

    let setup = json_of(rt.block_on(ks.opaque_recovery_setup_start(
        OpaqueRecoverySetupStartArgs {
            account_id: account_id.clone(),
        },
    )));
    assert_eq!(setup["ok"], true, "{setup}");
    let phrase = setup["phrase"].as_str().unwrap().to_owned();
    assert_eq!(phrase.split(' ').count(), 12);
    let response = server.register(
        &account_id,
        setup["registrationRequest"].as_str().unwrap(),
        true,
    );
    let recovery = json_of(rt.block_on(ks.opaque_recovery_setup_finish(
        OpaqueRecoverySetupFinishArgs {
            account_id: account_id.clone(),
            operation_id: setup["operationId"].as_str().unwrap().to_owned(),
            registration_response: response,
        },
    )));
    assert_eq!(recovery["ok"], true, "{recovery}");
    assert_eq!(recovery["masterKeyId"], account.master_key_id.as_str());
    let recovery_record = recovery["opaqueRecord"].as_str().unwrap().to_owned();
    let recovery_wrapped = recovery["wrappedMasterKey"].as_str().unwrap().to_owned();
    ks.clear(&account_id);

    let started = rt
        .block_on(ks.opaque_start_auth(OpaqueStartAuthArgs {
            password: phrase,
            email: Some(EMAIL.to_owned()),
            recovery: true,
        }))
        .unwrap();
    let ke2 = server.ke2(&account_id, &started.ke1, &recovery_record, true);
    let finished = json_of(rt.block_on(ks.opaque_finish_auth(OpaqueFinishAuthArgs {
        operation_id: started.operation_id.clone(),
        account_id: account_id.clone(),
        ke2,
        recovery: true,
    })));
    assert_eq!(finished["ok"], true, "{finished}");
    let unwrapped = json_of(
        ks.opaque_complete_recovery_unlock(OpaqueCompleteRecoveryUnlockArgs {
            operation_id: started.operation_id.clone(),
            encrypted_private_key: account.encrypted_private_key.clone(),
            wrapped_master_key: recovery_wrapped,
        }),
    );
    assert_eq!(unwrapped["ok"], true, "{unwrapped}");

    let new_password = "chosen after recovery";
    let reset = json_of(rt.block_on(ks.opaque_prepare_credential_reset(
        OpaquePrepareCredentialResetArgs {
            operation_id: started.operation_id.clone(),
            new_password: new_password.to_owned(),
        },
    )));
    assert_eq!(reset["ok"], true, "{reset}");
    let response = server.register(
        &account_id,
        reset["registrationRequest"].as_str().unwrap(),
        false,
    );
    let rewrapped = json_of(rt.block_on(ks.opaque_finish_credential_reset(
        OpaqueFinishCredentialResetArgs {
            operation_id: started.operation_id,
            registration_response: response,
        },
    )));
    assert_eq!(rewrapped["ok"], true, "{rewrapped}");
    assert_eq!(rewrapped["masterKeyId"], account.master_key_id.as_str());
    assert!(
        json_of(ks.status())["accounts"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    account.record = rewrapped["opaqueRecord"].as_str().unwrap().to_owned();
    account.wrapped_master_key = rewrapped["wrappedMasterKey"].as_str().unwrap().to_owned();
    let unlocked = sign_in_as(
        &ks,
        &server,
        &account,
        new_password,
        &account.encrypted_private_key,
        Vec::new(),
        AuthScheme::OpaqueV1,
    );
    assert_eq!(unlocked["ok"], true, "{unlocked}");
    ks.clear(&account_id);
}

#[test]
fn an_alias_key_is_shared_with_members_and_signs_for_the_alias() {
    let server = OpaqueServer::start();
    let ks = Keystore::new();
    let owner = new_account_id();
    let member = new_account_id();
    let owner_account = register(&ks, &server, &owner);
    let member_account = register(&ks, &server, &member);

    let created = json_of(ks.create_alias_key(
        CreateAliasKeyArgs {
            account_id: owner.clone(),
            email: "team@thelemail.test".to_owned(),
            display_name: "Team".to_owned(),
            recipients: vec![
                CreateAliasKeyRecipient {
                    account_id: owner.clone(),
                    public_key_armored: owner_account.public_key.clone(),
                },
                CreateAliasKeyRecipient {
                    account_id: member.clone(),
                    public_key_armored: member_account.public_key.clone(),
                },
            ],
        },
        0,
    ));
    assert_eq!(created["ok"], true, "{created}");
    let alias_fp = created["keyFingerprintHex"].as_str().unwrap().to_owned();
    let alias_public = created["publicKeyArmored"].as_str().unwrap().to_owned();
    let grants = created["grants"].as_array().unwrap();
    assert_eq!(grants.len(), 2);

    let grant_for = |account_id: &str| {
        let g = grants
            .iter()
            .find(|g| g["accountId"] == account_id)
            .unwrap();
        AliasKeyGrantInput {
            alias_id: "alias-1".to_owned(),
            key_version: 1,
            alias_key_fingerprint_hex: alias_fp.clone(),
            wrapped_private_key_armored: g["wrappedPrivateKeyArmored"].as_str().unwrap().to_owned(),
            member_key_fingerprint_hex: g["memberKeyFingerprintHex"].as_str().map(str::to_owned),
        }
    };
    let loaded = json_of(ks.load_alias_keys(LoadAliasKeysArgs {
        account_id: member.clone(),
        grants: vec![grant_for(&member), grant_for(&owner)],
    }));
    assert_eq!(loaded["loaded"], json!([alias_fp]), "{loaded}");
    assert_eq!(
        loaded["failed"],
        json!([{"aliasId": "alias-1", "keyVersion": 1}])
    );

    let to_alias = ciphertext(&encrypt(&ks, &owner, &alias_public, true, None));
    assert_eq!(
        decrypt(&ks, &member, to_alias, None)["plaintext"],
        "hello from the desktop"
    );
    let from_alias = ciphertext(&encrypt(
        &ks,
        &member,
        &owner_account.public_key,
        true,
        Some("alias-1"),
    ));
    let read = decrypt(&ks, &owner, from_alias, Some(vec![alias_public.clone()]));
    assert_eq!(read["signature"]["state"], "valid", "{read}");
    assert_eq!(read["signature"]["keyFingerprintHex"], alias_fp.as_str());
    assert_eq!(
        encrypt(
            &ks,
            &member,
            &owner_account.public_key,
            true,
            Some("alias-2")
        )["code"],
        "locked"
    );

    ks.unload_alias_keys(&member);
    let to_alias = ciphertext(&encrypt(&ks, &owner, &alias_public, true, None));
    assert_eq!(
        decrypt(&ks, &member, to_alias, None)["code"],
        "no_matching_key"
    );
    ks.clear(&owner);
    ks.clear(&member);
}

#[test]
fn a_rotation_keeps_old_mail_readable_and_survives_stay_unlocked() {
    let server = OpaqueServer::start();
    let ks = Keystore::new();
    let account_id = new_account_id();
    let account = register(&ks, &server, &account_id);
    let old_mail = ciphertext(&encrypt(&ks, &account_id, &account.public_key, true, None));

    let prepared = json_of(ks.prepare_key_rotation(
        PrepareKeyRotationArgs {
            account_id: account_id.clone(),
            emails: vec![EMAIL.to_owned()],
            expected_key_version: 1,
        },
        0,
    ));
    assert_eq!(prepared["ok"], true, "{prepared}");
    assert_eq!(prepared["keyVersion"], 2);
    let first_fp = hex_of(&json_of(ks.get_public_key(&account_id))["fingerprint"]);
    assert_eq!(
        prepared["transitions"][0]["previousKeyFingerprintHex"],
        first_fp.as_str()
    );
    let new_fp = prepared["fingerprintHex"].as_str().unwrap().to_owned();
    let committed = json_of(ks.commit_key_rotation(CommitKeyRotationArgs {
        account_id: account_id.clone(),
        fingerprint_hex: new_fp.clone(),
    }));
    assert_eq!(committed, json!({"ok": true, "keyVersion": 2}));
    let listed = json_of(ks.list_account_keys(&account_id));
    assert_eq!(listed["keys"][0]["fingerprintHex"], new_fp.as_str());
    assert_eq!(listed["keys"][1]["fingerprintHex"], first_fp.as_str());

    let server_half = [0x44u8; 32];
    let persisted = ks.persist(&account_id, &server_half).expect("persist");
    assert_eq!(persisted.keys.key_version, 2);
    assert_eq!(persisted.keys.history_keys.len(), 1);
    ks.lock(&account_id);
    assert!(!ks.restore(
        &account_id,
        persisted.email.clone(),
        &persisted.keys,
        &persisted.wrapped,
        persisted.wrap_version,
        &[0x45u8; 32],
    ));
    assert!(ks.restore(
        &account_id,
        persisted.email.clone(),
        &persisted.keys,
        &persisted.wrapped,
        persisted.wrap_version,
        &server_half,
    ));
    assert_eq!(
        decrypt(&ks, &account_id, old_mail, None)["plaintext"],
        "hello from the desktop"
    );
    let state = json_of(ks.get_public_key(&account_id));
    assert_eq!(state["keyVersion"], 2);
    assert_eq!(hex_of(&state["fingerprint"]), new_fp);

    let signed = json_of(ks.sign_detached(SignDetachedArgs {
        account_id: account_id.clone(),
        data: b"{\"type\":\"thelemail-read-delegation/v1\"}".to_vec(),
    }));
    assert_eq!(signed["ok"], true);
    assert_eq!(signed["keyFingerprintHex"], new_fp.as_str());
    ks.clear(&account_id);
}
