use thelemail_crypto::openpgp::{UnlockedKey, generate_account_key, inspect_wire_shape};

const OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/interop");
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

#[test]
fn a_generated_key_is_v6_with_a_32_byte_fingerprint() {
    let key =
        generate_account_key("", "user@thelemail.local", "a-passphrase", 0).expect("generate");
    assert_eq!(key.fingerprint_hex.len(), 64);
    let opened = UnlockedKey::open(&key.encrypted_private_key_armored, "a-passphrase")
        .expect("reopen generated key");
    assert_eq!(opened.fingerprint_bytes().len(), 32);
}

#[test]
fn a_v4_recipient_keeps_the_message_on_seipd_v1() {
    let key =
        generate_account_key("", "user@thelemail.local", "a-passphrase", 0).expect("generate");
    let unlocked =
        UnlockedKey::open(&key.encrypted_private_key_armored, "a-passphrase").expect("unlock");
    let legacy_pub =
        std::fs::read_to_string(format!("{FIXTURES}/keys/account.pub.asc")).expect("read pub");
    let legacy_meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{FIXTURES}/keys/meta.json")).expect("read meta"),
    )
    .expect("parse meta");
    let legacy = UnlockedKey::open(
        &std::fs::read_to_string(format!("{FIXTURES}/keys/account.enc.asc")).expect("read key"),
        legacy_meta["passphrase"].as_str().expect("passphrase"),
    )
    .expect("unlock legacy");

    let plaintext = b"to a v6 and a v4 recipient";
    let ciphertext = unlocked
        .encrypt_to(
            &[key.public_key_armored.clone(), legacy_pub],
            plaintext,
            Some(&unlocked),
        )
        .expect("encrypt");

    let shape = inspect_wire_shape(&ciphertext).expect("shape");
    assert_eq!(shape.pkesk_version, 3);
    assert_eq!(shape.seipd_version, 1);
    assert_eq!(
        unlocked.decrypt(&ciphertext).expect("v6 decrypt"),
        plaintext
    );
    assert_eq!(legacy.decrypt(&ciphertext).expect("v4 decrypt"), plaintext);
}

#[test]
fn a_generated_key_round_trips_in_rust() {
    let key = generate_account_key("Test User", "user@thelemail.local", "a-passphrase", 0)
        .expect("generate");
    let opened = UnlockedKey::open(&key.encrypted_private_key_armored, "a-passphrase")
        .expect("reopen generated key");
    assert_eq!(opened.fingerprint_hex(), key.fingerprint_hex);
    assert!(UnlockedKey::open(&key.encrypted_private_key_armored, "wrong").is_err());
}

#[test]
fn a_generated_key_is_readable_by_openpgp_js_with_the_expected_packet_shape() {
    let key = generate_account_key("Test User", "user@thelemail.local", "a-passphrase", 0)
        .expect("generate");
    std::fs::create_dir_all(OUT).expect("create out dir");
    std::fs::write(
        format!("{OUT}/rust-key.enc.asc"),
        &key.encrypted_private_key_armored,
    )
    .expect("write key");
    std::fs::write(format!("{OUT}/rust-key.pub.asc"), &key.public_key_armored).expect("write pub");

    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../xtask/js/verify-rust-key.mjs"
    );
    let out = std::process::Command::new("node")
        .arg(script)
        .arg(OUT)
        .arg("a-passphrase")
        .arg(&key.fingerprint_hex)
        .output()
        .expect("run verifier");

    assert!(
        out.status.success(),
        "openpgp.js could not use the Rust-generated key:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn rust_encrypted_messages_decrypt_in_openpgp_js() {
    let key = generate_account_key("Test User", "user@thelemail.local", "a-passphrase", 0)
        .expect("generate");
    let unlocked =
        UnlockedKey::open(&key.encrypted_private_key_armored, "a-passphrase").expect("unlock");

    let plaintext = b"encrypted by rust, read by openpgp.js";
    let ciphertext = unlocked
        .encrypt_to(
            std::slice::from_ref(&key.public_key_armored),
            plaintext,
            Some(&unlocked),
        )
        .expect("encrypt");

    let shape = thelemail_crypto::openpgp::inspect_wire_shape(&ciphertext).expect("shape");
    assert_eq!(shape.pkesk_version, 6);
    assert_eq!(shape.seipd_version, 2);

    assert_eq!(
        unlocked.decrypt(&ciphertext).expect("self decrypt"),
        plaintext
    );

    let dir = format!("{OUT}/signed");
    std::fs::create_dir_all(&dir).expect("create out dir");
    std::fs::write(
        format!("{dir}/rust-key.enc.asc"),
        &key.encrypted_private_key_armored,
    )
    .expect("write key");
    std::fs::write(format!("{dir}/rust-message.pgp"), &ciphertext).expect("write message");

    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../xtask/js/verify-rust-message.mjs"
    );
    let out = std::process::Command::new("node")
        .arg(script)
        .arg(&dir)
        .arg("a-passphrase")
        .arg(String::from_utf8_lossy(plaintext).as_ref())
        .arg(&key.fingerprint_hex)
        .output()
        .expect("run verifier");
    assert!(
        out.status.success(),
        "openpgp.js could not decrypt and verify the Rust ciphertext:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn openpgp_js_sees_a_signature_only_when_rust_was_asked_to_sign() {
    let key = generate_account_key("Test User", "user@thelemail.local", "a-passphrase", 0)
        .expect("generate");
    let unlocked =
        UnlockedKey::open(&key.encrypted_private_key_armored, "a-passphrase").expect("unlock");

    let plaintext = b"sent without a signature";
    let ciphertext = unlocked
        .encrypt_to(
            std::slice::from_ref(&key.public_key_armored),
            plaintext,
            None,
        )
        .expect("encrypt");

    let dir = format!("{OUT}/unsigned");
    std::fs::create_dir_all(&dir).expect("create out dir");
    std::fs::write(
        format!("{dir}/rust-key.enc.asc"),
        &key.encrypted_private_key_armored,
    )
    .expect("write key");
    std::fs::write(format!("{dir}/rust-message.pgp"), &ciphertext).expect("write message");

    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../xtask/js/verify-rust-message.mjs"
    );
    let out = std::process::Command::new("node")
        .arg(script)
        .arg(&dir)
        .arg("a-passphrase")
        .arg(String::from_utf8_lossy(plaintext).as_ref())
        .arg("unsigned")
        .output()
        .expect("run verifier");
    assert!(
        out.status.success(),
        "an unsigned Rust message must carry no signature:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn an_alias_grant_survives_the_armored_round_trip() {
    use thelemail_crypto::openpgp::generate_alias_key;

    let member = generate_account_key("Member", "member@thelemail.local", "member-pass", 0)
        .expect("generate member key");
    let member_key = UnlockedKey::open(&member.encrypted_private_key_armored, "member-pass")
        .expect("open member key");

    let alias =
        generate_alias_key("Team Alias", "team@thelemail.local", 0).expect("generate alias");
    let wrapped = member_key
        .encrypt_to_armored(
            std::slice::from_ref(&member.public_key_armored),
            alias.encrypted_private_key_armored.as_bytes(),
            Some(&member_key),
        )
        .expect("wrap the alias key to the member");

    assert!(
        wrapped.starts_with("-----BEGIN PGP MESSAGE"),
        "a grant is handed to the server as armor, not as raw bytes"
    );

    let unwrapped = member_key
        .decrypt(wrapped.as_bytes())
        .expect("an armored grant must decrypt without being base64-decoded first");
    let armored = String::from_utf8(unwrapped).expect("the grant carries an armored private key");
    let alias_key = UnlockedKey::open_unlocked(&armored).expect("the alias key opens unencrypted");
    assert_eq!(alias_key.fingerprint_hex(), alias.fingerprint_hex);
}

#[test]
fn a_signature_is_reported_valid_only_for_the_key_that_made_it() {
    use thelemail_crypto::openpgp::SignatureState;

    let sender = generate_account_key("Sender", "sender@thelemail.local", "sender-pass", 0)
        .expect("generate sender");
    let sender_key =
        UnlockedKey::open(&sender.encrypted_private_key_armored, "sender-pass").expect("unlock");
    let stranger = generate_account_key("Stranger", "stranger@thelemail.local", "stranger-pass", 0)
        .expect("generate stranger");

    let plaintext = b"signed by the sender";
    let signed = sender_key
        .encrypt_to(
            std::slice::from_ref(&sender.public_key_armored),
            plaintext,
            Some(&sender_key),
        )
        .expect("encrypt signed");

    let (data, check) = sender_key
        .decrypt_verified(&signed, std::slice::from_ref(&sender.public_key_armored))
        .expect("decrypt");
    let check = check.expect("a verdict is returned when verification keys are supplied");
    assert_eq!(data, plaintext);
    assert_eq!(check.state, SignatureState::Valid);
    assert_eq!(
        check.key_fingerprint_hex.as_deref(),
        Some(sender.fingerprint_hex.as_str())
    );

    let (_, check) = sender_key
        .decrypt_verified(&signed, std::slice::from_ref(&stranger.public_key_armored))
        .expect("decrypt");
    assert_eq!(
        check.expect("verdict").state,
        SignatureState::UnknownKey,
        "a signature from a key we were not given must not read as valid"
    );

    let unsigned = sender_key
        .encrypt_to(
            std::slice::from_ref(&sender.public_key_armored),
            plaintext,
            None,
        )
        .expect("encrypt unsigned");
    let (_, check) = sender_key
        .decrypt_verified(&unsigned, std::slice::from_ref(&sender.public_key_armored))
        .expect("decrypt");
    assert_eq!(check.expect("verdict").state, SignatureState::None);

    let (_, check) = sender_key.decrypt_verified(&signed, &[]).expect("decrypt");
    assert!(
        check.is_none(),
        "no verdict is claimed when no verification keys were supplied"
    );
}

#[test]
fn a_generated_key_is_stamped_behind_the_local_clock() {
    use pgp::composed::{Deserializable, SignedPublicKey};
    use pgp::types::KeyDetails;

    let before = std::time::SystemTime::now();
    let key = generate_account_key("Test User", "user@thelemail.local", "a-passphrase", 0)
        .expect("generate");
    let (public, _) =
        SignedPublicKey::from_armor_single(std::io::Cursor::new(&key.public_key_armored))
            .expect("parse public key");

    let primary: std::time::SystemTime = public.primary_key.created_at().into();
    assert!(
        primary < before,
        "a key stamped at or ahead of the local clock is one the server cannot encrypt to yet"
    );

    assert!(
        !public.public_subkeys.is_empty(),
        "expected an encryption subkey"
    );
    for sub in &public.public_subkeys {
        let at: std::time::SystemTime = sub.key.created_at().into();
        assert!(
            at < before,
            "the encryption subkey must be stamped in the past too"
        );
    }
}

#[test]
fn a_generated_key_follows_the_server_clock_not_the_device_clock() {
    use pgp::composed::{Deserializable, SignedPublicKey};
    use pgp::types::KeyDetails;

    let device_ahead = std::time::Duration::from_secs(3 * 60 * 60);
    let offset_ms = -(device_ahead.as_millis() as i64);
    let server_now = std::time::SystemTime::now() - device_ahead;

    let key = generate_account_key(
        "Test User",
        "user@thelemail.local",
        "a-passphrase",
        offset_ms,
    )
    .expect("generate");
    let (public, _) =
        SignedPublicKey::from_armor_single(std::io::Cursor::new(&key.public_key_armored))
            .expect("parse public key");

    let primary: std::time::SystemTime = public.primary_key.created_at().into();
    assert!(
        primary < server_now,
        "a device hours fast must still produce a key the server can encrypt to"
    );
    for sub in &public.public_subkeys {
        let at: std::time::SystemTime = sub.key.created_at().into();
        assert!(
            at < server_now,
            "the encryption subkey must follow the server clock too"
        );
    }
}

#[test]
fn rust_detached_signatures_verify_in_openpgp_js() {
    let key = generate_account_key("Signer", "signer@thelemail.local", "signer-pass", 0)
        .expect("generate signer");
    let unlocked =
        UnlockedKey::open(&key.encrypted_private_key_armored, "signer-pass").expect("unlock");
    let data = br#"{"address":"contact@acme.test","type":"thelemail-read-delegation/v1"}"#;
    let signature = unlocked.sign_detached(data).expect("sign detached");

    let dir = format!("{OUT}/detached");
    std::fs::create_dir_all(&dir).expect("create out dir");
    std::fs::write(
        format!("{dir}/rust-signer.pub.asc"),
        &key.public_key_armored,
    )
    .expect("write pub");
    std::fs::write(format!("{dir}/rust-signed.bin"), data).expect("write data");
    std::fs::write(format!("{dir}/rust-signature.sig"), &signature).expect("write signature");

    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../xtask/js/verify-rust-signature.mjs"
    );
    let out = std::process::Command::new("node")
        .arg(script)
        .arg(&dir)
        .arg(unlocked.fingerprint_hex())
        .output()
        .expect("run verifier");
    assert!(
        out.status.success(),
        "openpgp.js rejected a Rust detached signature:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
