#![forbid(unsafe_code)]

mod body;
mod extract;
mod pgpmime;
mod postal_text;

pub use body::{
    BodyError, BodyOptions, InlinePart, InlineParts, MAX_EMBEDDED_MESSAGES, MAX_MIME_DEPTH,
    ParsedBody, parse_body,
};
pub use extract::{AttachmentMeta, DEFAULT_MAX_TEXT_BYTES, Extracted, extract};
pub use pgpmime::{MAX_PGP_LAYERS, extract_pgp_armor, is_pgp_encrypted_mime};
