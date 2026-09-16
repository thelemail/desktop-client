use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign_verify::{PublicKey, Signature};

pub fn pubkey_from_config(config: &str) -> Result<String, String> {
    let parsed: serde_json::Value = serde_json::from_str(config).map_err(|e| e.to_string())?;
    let key = parsed
        .pointer("/plugins/updater/pubkey")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if key.trim().is_empty() {
        return Err("plugins.updater.pubkey is empty".to_owned());
    }
    Ok(key.to_owned())
}

fn decode_text(label: &str, encoded: &str) -> Result<String, String> {
    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|e| format!("{label} is not base64: {e}"))?;
    String::from_utf8(bytes).map_err(|_| format!("{label} is not text"))
}

pub fn verify(pubkey: &str, archive: &[u8], signature: &str) -> Result<(), String> {
    let key = PublicKey::decode(&decode_text("the public key", pubkey)?)
        .map_err(|e| format!("the public key does not parse: {e}"))?;
    let sig = Signature::decode(&decode_text("the signature", signature)?)
        .map_err(|e| format!("the signature does not parse: {e}"))?;
    key.verify(archive, &sig, true)
        .map_err(|e| format!("the signature does not match: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDlFODQ0RUZFOUNGNTU1ODcKUldTSFZmV2MvazZFbm00Q1NlRWhuNkc3MEJkSlVWQjRDdEJXYXQzUE9sMjk4OXBlcGpxUWlKakMK";
    const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVTSFZmV2MvazZFbmxYWlNnU1l0MjZhc2h5YzNDV0FscFNweDdTYjQ1MzJqTmpoVnY0WDJXU1R3eEdpVDJ1R09vaWxoSTgxQ1Z1U1RBMVVhRlRrUEpxaTF5cUNmOWlaVWdrPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzg5NTg1NDg0CWZpbGU6cGF5bG9hZAoydVIrNUZSNUMza0tCeUMwZmQ2NmhjWXhOTGRmZUgvbXUydjN1cXcxa1FjQ0Uwdmw1QnlaMzdmaWlGK2JocEtGc01PZkNSRGVOaVBVMENRZWluVXFEdz09Cg==";

    #[test]
    fn a_correctly_signed_archive_verifies() {
        assert!(verify(PUBKEY, b"test", SIGNATURE).is_ok());
    }

    #[test]
    fn a_signature_from_another_key_is_refused() {
        let other = STANDARD.encode(
            "untrusted comment: minisign public key: 0000000000000000\nRWQAAAAAAAAAAG4CSeEhn6G70BdJUVB4CtBWat3POl2989pepjqQiJjC\n",
        );
        assert!(verify(&other, b"test", SIGNATURE).is_err());
    }

    #[test]
    fn a_tampered_archive_is_refused() {
        assert!(verify(PUBKEY, b"tesT", SIGNATURE).is_err());
    }

    #[test]
    fn a_missing_pubkey_in_the_config_is_refused() {
        assert!(pubkey_from_config(r#"{"plugins":{"updater":{"pubkey":""}}}"#).is_err());
        assert!(pubkey_from_config(r#"{}"#).is_err());
        assert_eq!(
            pubkey_from_config(r#"{"plugins":{"updater":{"pubkey":"abc"}}}"#).as_deref(),
            Ok("abc")
        );
    }
}
