use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
use pgp::crypto::hash::HashAlgorithm;
use pgp::packet::{RevocationCode, Signature, SignatureType, SignatureVersion};
use pgp::types::{KeyDetails, KeyVersion, Tag};
use serde::Deserialize;

use crate::openpgp::{UnlockedKey, hex_encode, read_packet_header};

pub const PAYLOAD_TYPE: &str = "thelemail-key-transition/v1";
pub const MAX_SIGNATURE_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("endorsement is malformed")]
    Malformed,
    #[error("endorsement is not a binary document signature")]
    SignatureType,
    #[error("endorsement hash is weaker than SHA-256")]
    Hash,
    #[error("previous key is unusable")]
    PreviousKey,
    #[error("endorsement was not made by the previous primary key")]
    Signer,
    #[error("endorsement signature does not verify")]
    Signature,
    #[error("previous key was not valid when the endorsement was made")]
    Expired,
}

impl TransitionError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::SignatureType => "signature_type",
            Self::Hash => "hash",
            Self::PreviousKey => "previous_key",
            Self::Signer => "signer",
            Self::Signature => "signature",
            Self::Expired => "expired",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PayloadWire {
    account_id: String,
    new_key_fingerprint: String,
    previous_key_fingerprint: String,
    #[serde(rename = "type")]
    kind: String,
}

fn is_fingerprint_hex(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_plain_account_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')
}

pub fn payload(
    account_id: &str,
    new_key_fingerprint: &str,
    previous_key_fingerprint: &str,
) -> Result<String, TransitionError> {
    let new_fp = new_key_fingerprint.to_ascii_lowercase();
    let prev_fp = previous_key_fingerprint.to_ascii_lowercase();
    if !is_plain_account_id(account_id)
        || !is_fingerprint_hex(&new_fp)
        || !is_fingerprint_hex(&prev_fp)
    {
        return Err(TransitionError::Malformed);
    }
    Ok(format!(
        "{{\"accountId\":\"{account_id}\",\"newKeyFingerprint\":\"{new_fp}\",\"previousKeyFingerprint\":\"{prev_fp}\",\"type\":\"{PAYLOAD_TYPE}\"}}"
    ))
}

pub fn endorse(
    previous: &UnlockedKey,
    account_id: &str,
    new_key_fingerprint: &str,
) -> Result<Vec<u8>, TransitionError> {
    let message = payload(account_id, new_key_fingerprint, &previous.fingerprint_hex())?;
    let signature = previous
        .sign_detached(message.as_bytes())
        .map_err(|_| TransitionError::Signature)?;
    let public_key = previous
        .public_key_armored()
        .map_err(|_| TransitionError::PreviousKey)?;
    verify(message.as_bytes(), &signature, &public_key)?;
    Ok(signature)
}

pub fn verify(
    payload_bytes: &[u8],
    signature: &[u8],
    previous_public_key_armored: &str,
) -> Result<(), TransitionError> {
    let previous_fingerprint = payload_previous_fingerprint(payload_bytes)?;
    let sig = parse_single_signature(signature)?;
    if sig.typ() != Some(SignatureType::Binary) {
        return Err(TransitionError::SignatureType);
    }
    if !matches!(
        sig.hash_alg(),
        Some(
            HashAlgorithm::Sha256
                | HashAlgorithm::Sha384
                | HashAlgorithm::Sha512
                | HashAlgorithm::Sha3_256
                | HashAlgorithm::Sha3_512
        )
    ) {
        return Err(TransitionError::Hash);
    }

    let previous = read_previous_key(previous_public_key_armored)?;
    let primary = &previous.primary_key;
    let primary_fingerprint = primary.fingerprint();
    if hex_encode(primary_fingerprint.as_bytes()) != previous_fingerprint {
        return Err(TransitionError::Signer);
    }
    let issuer_fingerprints = sig.issuer_fingerprint();
    let issuer_key_ids = sig.issuer_key_id();
    if issuer_fingerprints.is_empty() && issuer_key_ids.is_empty() {
        return Err(TransitionError::Signer);
    }
    if issuer_fingerprints
        .iter()
        .any(|fp| fp.as_bytes() != primary_fingerprint.as_bytes())
    {
        return Err(TransitionError::Signer);
    }
    let primary_key_id = primary.legacy_key_id();
    if issuer_key_ids.iter().any(|id| **id != primary_key_id) {
        return Err(TransitionError::Signer);
    }

    let versions_match = matches!(
        (sig.version(), primary.version()),
        (SignatureVersion::V4, KeyVersion::V4) | (SignatureVersion::V6, KeyVersion::V6)
    );
    if !versions_match || sig.verify(primary, payload_bytes).is_err() {
        return Err(TransitionError::Signature);
    }
    let created = sig.created().ok_or(TransitionError::Signature)?;
    check_valid_at(&previous, created.as_secs())
}

fn payload_previous_fingerprint(payload_bytes: &[u8]) -> Result<String, TransitionError> {
    let wire: PayloadWire =
        serde_json::from_slice(payload_bytes).map_err(|_| TransitionError::Malformed)?;
    if wire.kind != PAYLOAD_TYPE {
        return Err(TransitionError::Malformed);
    }
    let rebuilt = payload(
        &wire.account_id,
        &wire.new_key_fingerprint,
        &wire.previous_key_fingerprint,
    )?;
    if rebuilt.as_bytes() != payload_bytes {
        return Err(TransitionError::Malformed);
    }
    Ok(wire.previous_key_fingerprint)
}

fn parse_single_signature(raw: &[u8]) -> Result<Signature, TransitionError> {
    if raw.is_empty() || raw.len() > MAX_SIGNATURE_BYTES {
        return Err(TransitionError::Malformed);
    }
    let header = read_packet_header(raw, 0).map_err(|_| TransitionError::Malformed)?;
    let body_len = header.body_len.ok_or(TransitionError::Malformed)?;
    if header.tag != 2 || header.body_start.checked_add(body_len) != Some(raw.len()) {
        return Err(TransitionError::Malformed);
    }
    let detached = DetachedSignature::from_bytes(raw).map_err(|_| TransitionError::Malformed)?;
    Ok(detached.signature)
}

fn read_previous_key(armored: &str) -> Result<SignedPublicKey, TransitionError> {
    let (keys, _) = SignedPublicKey::from_armor_many(armored.as_bytes())
        .map_err(|_| TransitionError::PreviousKey)?;
    let mut keys = keys
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TransitionError::PreviousKey)?;
    if keys.len() != 1 {
        return Err(TransitionError::PreviousKey);
    }
    Ok(keys.remove(0))
}

fn newest(sigs: Vec<&Signature>) -> Option<&Signature> {
    sigs.into_iter()
        .max_by_key(|s| s.created().map(|c| c.as_secs()).unwrap_or(0))
}

fn self_signature(key: &SignedPublicKey) -> Option<&Signature> {
    let primary = &key.primary_key;
    if primary.version() == KeyVersion::V6 {
        let direct = key
            .details
            .direct_signatures
            .iter()
            .filter(|s| s.verify_key(primary).is_ok())
            .collect::<Vec<_>>();
        if let Some(sig) = newest(direct) {
            return Some(sig);
        }
    }
    let certifications = key
        .details
        .users
        .iter()
        .flat_map(|user| {
            user.signatures.iter().filter(move |s| {
                s.verify_certification(primary, Tag::UserId, &user.id)
                    .is_ok()
            })
        })
        .collect::<Vec<_>>();
    let primary_marked = certifications
        .iter()
        .copied()
        .filter(|s| s.is_primary())
        .collect::<Vec<_>>();
    newest(primary_marked).or_else(|| newest(certifications))
}

fn check_valid_at(key: &SignedPublicKey, at_secs: u32) -> Result<(), TransitionError> {
    let primary = &key.primary_key;
    let created = primary.created_at().as_secs();
    if at_secs < created {
        return Err(TransitionError::Expired);
    }
    let self_sig = self_signature(key).ok_or(TransitionError::PreviousKey)?;
    if let Some(lifetime) = self_sig.key_expiration_time() {
        let lifetime = lifetime.as_secs();
        if lifetime > 0 && u64::from(at_secs) > u64::from(created) + u64::from(lifetime) {
            return Err(TransitionError::Expired);
        }
    }
    if key
        .details
        .revocation_signatures
        .iter()
        .any(|s| s.verify_key(primary).is_ok() && revokes_at(s, at_secs))
    {
        return Err(TransitionError::Expired);
    }
    Ok(())
}

fn revokes_at(revocation: &Signature, at_secs: u32) -> bool {
    if revocation.revocation_reason_code() == Some(&RevocationCode::KeyCompromised) {
        return true;
    }
    let Some(created) = revocation.created().map(|c| c.as_secs()) else {
        return true;
    };
    if created > at_secs {
        return false;
    }
    match revocation.signature_expiration_time().map(|d| d.as_secs()) {
        Some(lifetime) if lifetime > 0 => {
            u64::from(at_secs) <= u64::from(created) + u64::from(lifetime)
        }
        _ => true,
    }
}
