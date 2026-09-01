use pgp::composed::{Deserializable, Message, SignedPublicKey, SignedSecretKey};
use pgp::crypto::hash::HashAlgorithm;
use pgp::crypto::sym::SymmetricKeyAlgorithm;
use pgp::types::{KeyDetails, Password};

#[derive(Debug, thiserror::Error)]
pub enum PgpError {
    #[error("invalid key")]
    InvalidKey,
    #[error("key unlock failed")]
    UnlockFailed,
    #[error("invalid ciphertext")]
    InvalidCiphertext,
    #[error("no matching key")]
    NoMatchingKey,
    #[error("decryption failed")]
    DecryptFailed,
    #[error("encryption failed")]
    EncryptFailed,
    #[error("invalid recipient key")]
    InvalidRecipientKey,
    #[error("no recipients")]
    NoRecipients,
}

const ARMOR_PREFIX: &[u8] = b"-----BEGIN PGP MESSAGE";

pub struct UnlockedKey {
    key: SignedSecretKey,
}

impl UnlockedKey {
    pub fn open(armored: &str, passphrase: &str) -> Result<Self, PgpError> {
        let (mut key, _) =
            SignedSecretKey::from_string(armored).map_err(|_| PgpError::InvalidKey)?;
        let password = Password::from(passphrase.to_owned());

        key.primary_key
            .remove_password(&password)
            .map_err(|_| PgpError::UnlockFailed)?;
        for sub in &mut key.secret_subkeys {
            sub.key
                .remove_password(&password)
                .map_err(|_| PgpError::UnlockFailed)?;
        }

        Ok(Self { key })
    }

    pub fn fingerprint_hex(&self) -> String {
        hex_encode(self.key.public_key().fingerprint().as_bytes())
    }

    pub fn fingerprint_bytes(&self) -> Vec<u8> {
        self.key.public_key().fingerprint().as_bytes().to_vec()
    }

    pub fn public_key_armored(&self) -> Result<String, PgpError> {
        let subkeys = self
            .key
            .secret_subkeys
            .iter()
            .map(|sub| sub.signed_public_key())
            .collect();
        SignedPublicKey::new(
            self.key.primary_key.public_key().clone(),
            self.key.details.clone(),
            subkeys,
        )
        .to_armored_string(Default::default())
        .map_err(|_| PgpError::InvalidKey)
    }

    pub fn encrypt_to(
        &self,
        recipients_armored: &[String],
        plaintext: &[u8],
        sign: bool,
    ) -> Result<Vec<u8>, PgpError> {
        use pgp::composed::MessageBuilder;
        use pgp::types::Password;
        use rand::rngs::OsRng;

        if recipients_armored.is_empty() {
            return Err(PgpError::NoRecipients);
        }

        let mut keys = Vec::with_capacity(recipients_armored.len());
        for armored in recipients_armored {
            let (key, _) =
                SignedPublicKey::from_string(armored).map_err(|_| PgpError::InvalidRecipientKey)?;
            keys.push(key);
        }

        let owned = plaintext.to_vec();
        let mut builder =
            MessageBuilder::from_bytes("", owned).seipd_v1(OsRng, SymmetricKeyAlgorithm::AES256);

        for key in &keys {
            let subkey = key
                .public_subkeys
                .iter()
                .find(|sub| sub.key.algorithm().can_encrypt())
                .ok_or(PgpError::InvalidRecipientKey)?;
            builder
                .encrypt_to_key(OsRng, subkey)
                .map_err(|_| PgpError::InvalidRecipientKey)?;
        }

        if sign {
            builder.sign(
                &self.key.primary_key,
                Password::empty(),
                HashAlgorithm::Sha256,
            );
        }

        builder.to_vec(OsRng).map_err(|_| PgpError::EncryptFailed)
    }

    pub fn encrypt_to_armored(
        &self,
        recipients_armored: &[String],
        plaintext: &[u8],
        sign: bool,
    ) -> Result<String, PgpError> {
        use pgp::composed::MessageBuilder;
        use pgp::types::Password;
        use rand::rngs::OsRng;

        if recipients_armored.is_empty() {
            return Err(PgpError::NoRecipients);
        }

        let mut keys = Vec::with_capacity(recipients_armored.len());
        for armored in recipients_armored {
            let (key, _) =
                SignedPublicKey::from_string(armored).map_err(|_| PgpError::InvalidRecipientKey)?;
            keys.push(key);
        }

        let owned = plaintext.to_vec();
        let mut builder =
            MessageBuilder::from_bytes("", owned).seipd_v1(OsRng, SymmetricKeyAlgorithm::AES256);

        for key in &keys {
            let subkey = key
                .public_subkeys
                .iter()
                .find(|sub| sub.key.algorithm().can_encrypt())
                .ok_or(PgpError::InvalidRecipientKey)?;
            builder
                .encrypt_to_key(OsRng, subkey)
                .map_err(|_| PgpError::InvalidRecipientKey)?;
        }

        if sign {
            builder.sign(
                &self.key.primary_key,
                Password::empty(),
                HashAlgorithm::Sha256,
            );
        }

        builder
            .to_armored_string(OsRng, Default::default())
            .map_err(|_| PgpError::EncryptFailed)
    }

    pub fn open_unlocked(armored: &str) -> Result<Self, PgpError> {
        let (key, _) = SignedSecretKey::from_string(armored).map_err(|_| PgpError::InvalidKey)?;
        Ok(Self { key })
    }

    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, PgpError> {
        if ciphertext.starts_with(ARMOR_PREFIX) {
            let (message, _) = Message::from_armor(std::io::Cursor::new(ciphertext))
                .map_err(|_| PgpError::InvalidCiphertext)?;
            let mut decrypted = message
                .decrypt(&Password::empty(), &self.key)
                .map_err(|_| PgpError::DecryptFailed)?;
            return decrypted.as_data_vec().map_err(|_| PgpError::DecryptFailed);
        }
        let message = Message::from_bytes(ciphertext).map_err(|_| PgpError::InvalidCiphertext)?;
        let mut decrypted = message
            .decrypt(&Password::empty(), &self.key)
            .map_err(|_| PgpError::DecryptFailed)?;
        decrypted.as_data_vec().map_err(|_| PgpError::DecryptFailed)
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireShape {
    pub pkesk_version: u8,
    pub seipd_version: u8,
}

pub fn inspect_wire_shape(ciphertext: &[u8]) -> Result<WireShape, PgpError> {
    let mut cursor = 0usize;
    let mut pkesk_version = 0u8;
    let mut seipd_version = 0u8;

    while cursor < ciphertext.len() {
        let header = read_packet_header(ciphertext, cursor)?;
        if header.body_start >= ciphertext.len() {
            return Err(PgpError::InvalidCiphertext);
        }
        match header.tag {
            1 => pkesk_version = ciphertext[header.body_start],
            18 => seipd_version = ciphertext[header.body_start],
            _ => {}
        }
        if pkesk_version != 0 && seipd_version != 0 {
            break;
        }
        match header.body_len {
            Some(len) => {
                let next = header
                    .body_start
                    .checked_add(len)
                    .ok_or(PgpError::InvalidCiphertext)?;
                if next > ciphertext.len() {
                    return Err(PgpError::InvalidCiphertext);
                }
                cursor = next;
            }
            None => return Err(PgpError::InvalidCiphertext),
        }
    }

    if pkesk_version == 0 || seipd_version == 0 {
        return Err(PgpError::InvalidCiphertext);
    }
    Ok(WireShape {
        pkesk_version,
        seipd_version,
    })
}

struct PacketHeader {
    tag: u8,
    body_start: usize,
    body_len: Option<usize>,
}

fn read_packet_header(buf: &[u8], at: usize) -> Result<PacketHeader, PgpError> {
    let first = *buf.get(at).ok_or(PgpError::InvalidCiphertext)?;
    if first & 0x80 == 0 {
        return Err(PgpError::InvalidCiphertext);
    }
    if first & 0x40 != 0 {
        let tag = first & 0x3f;
        let n = *buf.get(at + 1).ok_or(PgpError::InvalidCiphertext)?;
        if n < 192 {
            Ok(PacketHeader {
                tag,
                body_start: at + 2,
                body_len: Some(n as usize),
            })
        } else if n < 224 {
            let second = *buf.get(at + 2).ok_or(PgpError::InvalidCiphertext)?;
            let len = ((n as usize - 192) << 8) + second as usize + 192;
            Ok(PacketHeader {
                tag,
                body_start: at + 3,
                body_len: Some(len),
            })
        } else if n < 255 {
            Ok(PacketHeader {
                tag,
                body_start: at + 2,
                body_len: None,
            })
        } else {
            let b = buf.get(at + 2..at + 6).ok_or(PgpError::InvalidCiphertext)?;
            let len = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
            Ok(PacketHeader {
                tag,
                body_start: at + 6,
                body_len: Some(len),
            })
        }
    } else {
        let tag = (first >> 2) & 0x0f;
        match first & 0x03 {
            0 => {
                let n = *buf.get(at + 1).ok_or(PgpError::InvalidCiphertext)?;
                Ok(PacketHeader {
                    tag,
                    body_start: at + 2,
                    body_len: Some(n as usize),
                })
            }
            1 => {
                let b = buf.get(at + 1..at + 3).ok_or(PgpError::InvalidCiphertext)?;
                Ok(PacketHeader {
                    tag,
                    body_start: at + 3,
                    body_len: Some(u16::from_be_bytes([b[0], b[1]]) as usize),
                })
            }
            2 => {
                let b = buf.get(at + 1..at + 5).ok_or(PgpError::InvalidCiphertext)?;
                let len = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
                Ok(PacketHeader {
                    tag,
                    body_start: at + 5,
                    body_len: Some(len),
                })
            }
            _ => Ok(PacketHeader {
                tag,
                body_start: at + 1,
                body_len: None,
            }),
        }
    }
}

pub fn public_key_fingerprint_hex(armored: &str) -> Result<String, PgpError> {
    let (key, _) = SignedPublicKey::from_string(armored).map_err(|_| PgpError::InvalidRecipientKey)?;
    Ok(hex_encode(key.fingerprint().as_bytes()))
}

pub struct GeneratedKey {
    pub public_key_armored: String,
    pub encrypted_private_key_armored: String,
    pub fingerprint_hex: String,
}

pub fn generate_account_key(
    display_name: &str,
    email: &str,
    passphrase: &str,
) -> Result<GeneratedKey, PgpError> {
    generate_key(display_name, email, Some(passphrase))
}

pub fn generate_alias_key(display_name: &str, email: &str) -> Result<GeneratedKey, PgpError> {
    generate_key(display_name, email, None)
}

fn generate_key(
    display_name: &str,
    email: &str,
    passphrase: Option<&str>,
) -> Result<GeneratedKey, PgpError> {
    use pgp::composed::{EncryptionCaps, KeyType, SecretKeyParamsBuilder, SubkeyParamsBuilder};
    use pgp::types::KeyVersion;
    use rand::rngs::OsRng;

    let user_id = if display_name.is_empty() {
        format!("<{email}>")
    } else {
        format!("{display_name} <{email}>")
    };

    let subkey = SubkeyParamsBuilder::default()
        .version(KeyVersion::V4)
        .key_type(KeyType::X25519)
        .can_encrypt(EncryptionCaps::All)
        .passphrase(passphrase.map(str::to_owned))
        .build()
        .map_err(|_| PgpError::InvalidKey)?;

    let params = SecretKeyParamsBuilder::default()
        .version(KeyVersion::V4)
        .key_type(KeyType::Ed25519)
        .can_sign(true)
        .can_certify(true)
        .primary_user_id(user_id)
        .passphrase(passphrase.map(str::to_owned))
        .subkey(subkey)
        .build()
        .map_err(|_| PgpError::InvalidKey)?;

    let signed = params.generate(OsRng).map_err(|_| PgpError::InvalidKey)?;

    let subkeys = signed
        .secret_subkeys
        .iter()
        .map(|sub| sub.signed_public_key())
        .collect();
    let public_key_armored = SignedPublicKey::new(
        signed.primary_key.public_key().clone(),
        signed.details.clone(),
        subkeys,
    )
    .to_armored_string(Default::default())
    .map_err(|_| PgpError::InvalidKey)?;

    let fingerprint_hex = hex_encode(signed.public_key().fingerprint().as_bytes());
    let encrypted_private_key_armored = signed
        .to_armored_string(Default::default())
        .map_err(|_| PgpError::InvalidKey)?;

    Ok(GeneratedKey {
        public_key_armored,
        encrypted_private_key_armored,
        fingerprint_hex,
    })
}
