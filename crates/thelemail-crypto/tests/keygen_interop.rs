use thelemail_crypto::openpgp::{UnlockedKey, generate_account_key};

const OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/interop");

#[test]
fn a_generated_key_round_trips_in_rust() {
    let key = generate_account_key("Test User", "user@thelemail.local", "a-passphrase")
        .expect("generate");
    let opened = UnlockedKey::open(&key.encrypted_private_key_armored, "a-passphrase")
        .expect("reopen generated key");
    assert_eq!(opened.fingerprint_hex(), key.fingerprint_hex);
    assert!(UnlockedKey::open(&key.encrypted_private_key_armored, "wrong").is_err());
}

#[test]
fn a_generated_key_is_readable_by_openpgp_js_with_the_expected_packet_shape() {
    let key = generate_account_key("Test User", "user@thelemail.local", "a-passphrase")
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
    let key = generate_account_key("Test User", "user@thelemail.local", "a-passphrase")
        .expect("generate");
    let unlocked =
        UnlockedKey::open(&key.encrypted_private_key_armored, "a-passphrase").expect("unlock");

    let plaintext = b"encrypted by rust, read by openpgp.js";
    let ciphertext = unlocked
        .encrypt_to(
            std::slice::from_ref(&key.public_key_armored),
            plaintext,
            true,
        )
        .expect("encrypt");

    let shape = thelemail_crypto::openpgp::inspect_wire_shape(&ciphertext).expect("shape");
    assert_eq!(
        shape.pkesk_version, 3,
        "must match the server's PKESK version"
    );
    assert_eq!(
        shape.seipd_version, 1,
        "must match the server's SEIPD version"
    );

    assert_eq!(
        unlocked.decrypt(&ciphertext).expect("self decrypt"),
        plaintext
    );

    std::fs::create_dir_all(OUT).expect("create out dir");
    std::fs::write(
        format!("{OUT}/rust-key.enc.asc"),
        &key.encrypted_private_key_armored,
    )
    .expect("write key");
    std::fs::write(format!("{OUT}/rust-message.pgp"), &ciphertext).expect("write message");

    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../xtask/js/verify-rust-message.mjs"
    );
    let out = std::process::Command::new("node")
        .arg(script)
        .arg(OUT)
        .arg("a-passphrase")
        .arg(String::from_utf8_lossy(plaintext).as_ref())
        .output()
        .expect("run verifier");
    assert!(
        out.status.success(),
        "openpgp.js could not decrypt the Rust ciphertext:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn an_alias_grant_survives_the_armored_round_trip() {
    use thelemail_crypto::openpgp::generate_alias_key;

    let member = generate_account_key("Member", "member@thelemail.local", "member-pass")
        .expect("generate member key");
    let member_key = UnlockedKey::open(&member.encrypted_private_key_armored, "member-pass")
        .expect("open member key");

    let alias = generate_alias_key("Team Alias", "team@thelemail.local").expect("generate alias");
    let wrapped = member_key
        .encrypt_to_armored(
            std::slice::from_ref(&member.public_key_armored),
            alias.encrypted_private_key_armored.as_bytes(),
            true,
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
