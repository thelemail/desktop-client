#![forbid(unsafe_code)]

mod extract;
mod pgpmime;

pub use extract::{AttachmentMeta, DEFAULT_MAX_TEXT_BYTES, Extracted, extract};
pub use pgpmime::{MAX_PGP_LAYERS, extract_pgp_armor, is_pgp_encrypted_mime};
