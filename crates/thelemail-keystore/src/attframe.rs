use serde::{Deserialize, Serialize};

pub const MAGIC: &[u8; 4] = b"TMA1";
pub const VERSION: u8 = 0x01;
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
const PREFIX_LEN: usize = 4 + 1 + 4;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("not a thelemail attachment frame")]
    BadMagic,
    #[error("unsupported frame version")]
    BadVersion,
    #[error("header too large")]
    HeaderTooLarge,
    #[error("frame truncated")]
    Truncated,
    #[error("malformed header")]
    MalformedHeader,
    #[error("payload size mismatch")]
    SizeMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AttHeader {
    pub v: u8,
    pub filename: String,
    pub content_type: String,
    pub disposition: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_id: Option<String>,
    pub plaintext_size: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plaintext_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecryptedAttachmentHeader {
    pub filename: String,
    pub content_type: String,
    pub disposition: String,
    pub content_id: Option<String>,
    pub plaintext_size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plaintext_sha256: Option<String>,
}

pub fn parse_header(bytes: &[u8]) -> Result<(DecryptedAttachmentHeader, usize), FrameError> {
    if bytes.len() < PREFIX_LEN {
        return Err(FrameError::Truncated);
    }
    if &bytes[0..4] != MAGIC {
        return Err(FrameError::BadMagic);
    }
    if bytes[4] != VERSION {
        return Err(FrameError::BadVersion);
    }
    let header_len = u32::from_be_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
    if header_len > MAX_HEADER_BYTES {
        return Err(FrameError::HeaderTooLarge);
    }
    let header_end = PREFIX_LEN
        .checked_add(header_len)
        .ok_or(FrameError::Truncated)?;
    if bytes.len() < header_end {
        return Err(FrameError::Truncated);
    }

    let raw: AttHeader = serde_json::from_slice(&bytes[PREFIX_LEN..header_end])
        .map_err(|_| FrameError::MalformedHeader)?;
    if raw.v != 1 || raw.filename.is_empty() || raw.content_type.is_empty() {
        return Err(FrameError::MalformedHeader);
    }
    if raw.disposition != "attachment" && raw.disposition != "inline" {
        return Err(FrameError::MalformedHeader);
    }

    Ok((
        DecryptedAttachmentHeader {
            filename: raw.filename,
            content_type: raw.content_type,
            disposition: raw.disposition,
            content_id: raw.content_id,
            plaintext_size: raw.plaintext_size,
            plaintext_sha256: raw.plaintext_sha256,
        },
        header_end,
    ))
}

pub fn parse(bytes: &[u8]) -> Result<(DecryptedAttachmentHeader, Vec<u8>), FrameError> {
    let (header, header_end) = parse_header(bytes)?;
    let payload = &bytes[header_end..];
    if payload.len() != header.plaintext_size {
        return Err(FrameError::SizeMismatch);
    }
    Ok((header, payload.to_vec()))
}

pub fn build(header: &AttHeader, payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    let mut full = header.clone();
    full.v = 1;
    full.plaintext_size = payload.len();
    let json = serde_json::to_vec(&full).map_err(|_| FrameError::MalformedHeader)?;
    if json.len() > MAX_HEADER_BYTES {
        return Err(FrameError::HeaderTooLarge);
    }
    let mut out = Vec::with_capacity(PREFIX_LEN + json.len() + payload.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&(json.len() as u32).to_be_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(payload);
    Ok(out)
}
