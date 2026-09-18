use std::time::Instant;
use thelemail_crypto::openpgp::UnlockedKey;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

const MAX_PER_DECRYPT_MICROS: u128 = 50_000;

#[test]
fn decrypting_does_not_rerun_the_key_derivation() {
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{FIXTURES}/keys/meta.json")).expect("read meta"),
    )
    .expect("parse meta");
    let passphrase = meta["passphrase"].as_str().expect("passphrase");
    let armored =
        std::fs::read_to_string(format!("{FIXTURES}/keys/account.enc.asc")).expect("read key");

    let key = UnlockedKey::open(&armored, passphrase).expect("unlock");
    let ct = std::fs::read(format!("{FIXTURES}/messages/body-plain-go.pgp")).expect("read message");

    key.decrypt(&ct).expect("warmup decrypt");

    let rounds = 20;
    let started = Instant::now();
    for _ in 0..rounds {
        key.decrypt(&ct).expect("decrypt");
    }
    let per_decrypt = started.elapsed().as_micros() / rounds;

    assert!(
        per_decrypt < MAX_PER_DECRYPT_MICROS,
        "decrypt took {per_decrypt}us per message, budget {MAX_PER_DECRYPT_MICROS}us. \
         The key is being re-derived per message instead of once per unlock."
    );
}

#[test]
fn generated_keys_are_locked_with_the_iterated_s2k_under_aead() {
    use pgp::composed::{Deserializable, SignedSecretKey};
    use pgp::crypto::aead::AeadAlgorithm;
    use pgp::crypto::sym::SymmetricKeyAlgorithm;
    use pgp::types::{S2kParams, SecretParams, StringToKey};

    let key = thelemail_crypto::openpgp::generate_account_key(
        "",
        "user@thelemail.local",
        "a-passphrase",
        0,
    )
    .expect("generate");
    let (parsed, _) =
        SignedSecretKey::from_string(&key.encrypted_private_key_armored).expect("parse");

    let params = std::iter::once(parsed.primary_key.secret_params()).chain(
        parsed
            .secret_subkeys
            .iter()
            .map(|sub| sub.key.secret_params()),
    );
    for params in params {
        let SecretParams::Encrypted(encrypted) = params else {
            panic!("secret key material is not passphrase-protected");
        };
        match encrypted.string_to_key_params() {
            S2kParams::Aead {
                sym_alg: SymmetricKeyAlgorithm::AES256,
                aead_mode: AeadAlgorithm::Gcm,
                s2k: StringToKey::IteratedAndSalted { .. },
                ..
            } => {}
            other => panic!("unexpected key protection {other:?}"),
        }
    }
}
