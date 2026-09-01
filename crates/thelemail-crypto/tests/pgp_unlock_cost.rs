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
