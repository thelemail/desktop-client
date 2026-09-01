use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const INFO_AMK_WRAP_PW: &[u8] = b"thelemail/amk-wrap/v1";
pub const INFO_AMK_WRAP_RECOVERY: &[u8] = b"thelemail/amk-wrap/recovery/v1";
pub const INFO_AMK_ID: &[u8] = b"thelemail/amk-id/v1";
pub const INFO_PGP_PASSPHRASE: &[u8] = b"thelemail/pgp-passphrase/v1";

pub const AMK_LEN: usize = 32;
pub const MASTER_KEY_ID_LEN: usize = 16;
pub const NONCE_LEN: usize = 12;
pub const WRAP_VERSION: u8 = 0x01;
pub const WRAPPED_MASTER_KEY_LEN: usize = 1 + NONCE_LEN + AMK_LEN + 16;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AmkError {
    #[error("invalid wrapped master key")]
    InvalidWrapped,
    #[error("unwrap failed")]
    UnwrapFailed,
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Amk([u8; AMK_LEN]);

impl Amk {
    pub fn generate() -> Self {
        let mut bytes = [0u8; AMK_LEN];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; AMK_LEN]) -> Self {
        Self(bytes)
    }

    pub fn expose(&self) -> &[u8; AMK_LEN] {
        &self.0
    }
}

impl std::fmt::Debug for Amk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Amk(..)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct B64Std(String);

impl B64Std {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for B64Std {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("B64Std(..)")
    }
}

fn wrap_info(recovery: bool) -> &'static [u8] {
    if recovery {
        INFO_AMK_WRAP_RECOVERY
    } else {
        INFO_AMK_WRAP_PW
    }
}

fn hkdf(ikm: &[u8], info: &[u8], out: &mut [u8]) {
    Hkdf::<Sha256>::new(None, ikm)
        .expand(info, out)
        .expect("hkdf output length within sha256 limit");
}

fn wrap_key(export_key: &[u8], info: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    hkdf(export_key, info, &mut key);
    key
}

pub fn wrap_master_key(
    export_key: &[u8],
    amk: &Amk,
    recovery: bool,
) -> [u8; WRAPPED_MASTER_KEY_LEN] {
    let info = wrap_info(recovery);
    let mut key = wrap_key(export_key, info);
    let cipher = Aes256Gcm::new_from_slice(&key).expect("aes-256 key length");
    key.zeroize();

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: amk.expose(),
                aad: info,
            },
        )
        .expect("aes-gcm encryption of fixed-length plaintext");

    let mut out = [0u8; WRAPPED_MASTER_KEY_LEN];
    out[0] = WRAP_VERSION;
    out[1..1 + NONCE_LEN].copy_from_slice(&nonce_bytes);
    out[1 + NONCE_LEN..].copy_from_slice(&ciphertext);
    out
}

pub fn unwrap_master_key(
    export_key: &[u8],
    wrapped: &[u8],
    recovery: bool,
) -> Result<Amk, AmkError> {
    if wrapped.len() != WRAPPED_MASTER_KEY_LEN || wrapped[0] != WRAP_VERSION {
        return Err(AmkError::InvalidWrapped);
    }
    let info = wrap_info(recovery);
    let mut key = wrap_key(export_key, info);
    let cipher = Aes256Gcm::new_from_slice(&key).expect("aes-256 key length");
    key.zeroize();

    let nonce = Nonce::from_slice(&wrapped[1..1 + NONCE_LEN]);
    let mut plaintext = cipher
        .decrypt(
            nonce,
            Payload {
                msg: &wrapped[1 + NONCE_LEN..],
                aad: info,
            },
        )
        .map_err(|_| AmkError::UnwrapFailed)?;

    if plaintext.len() != AMK_LEN {
        plaintext.zeroize();
        return Err(AmkError::UnwrapFailed);
    }
    let mut bytes = [0u8; AMK_LEN];
    bytes.copy_from_slice(&plaintext);
    plaintext.zeroize();
    Ok(Amk::from_bytes(bytes))
}

pub fn derive_master_key_id(amk: &Amk) -> [u8; MASTER_KEY_ID_LEN] {
    let mut out = [0u8; MASTER_KEY_ID_LEN];
    hkdf(amk.expose(), INFO_AMK_ID, &mut out);
    out
}

pub fn derive_pgp_passphrase(amk: &Amk) -> B64Std {
    use base64::Engine as _;
    let mut out = [0u8; 32];
    hkdf(amk.expose(), INFO_PGP_PASSPHRASE, &mut out);
    let encoded = base64::engine::general_purpose::STANDARD.encode(out);
    out.zeroize();
    B64Std(encoded)
}
