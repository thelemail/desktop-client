#![forbid(unsafe_code)]

mod allowlist;
mod css;
mod designed;
mod dom;
#[cfg(test)]
mod interop_tests;
mod js;
mod plain;
mod sanitize;
mod tracking;

pub use css::scrub_css;
pub use designed::looks_designed;
pub use plain::plain_to_html;
pub use sanitize::{MAX_ELEMENT_DEPTH, SanitizeOptions, sanitize_html, sanitize_signature_html};
pub use thelemail_mime::{InlinePart, InlineParts};
pub use tracking::{TRACKING_PARAMS, strip_tracking_params};
