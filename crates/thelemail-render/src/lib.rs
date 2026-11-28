#![forbid(unsafe_code)]

mod allowlist;
mod css;
mod designed;
mod document;
mod dom;
#[cfg(test)]
mod interop_tests;
mod js;
mod plain;
mod policy;
mod sanitize;
mod tracking;

pub use css::scrub_css;
pub use designed::looks_designed;
pub use document::{
    BodySource, CSP, RenderOptions, RenderedBody, Theme, base_style, build_src_doc, fit_zoom,
    render_body, with_theme,
};
pub use plain::plain_to_html;
pub use policy::{Resource, opens_externally, resolve_resource};
pub use sanitize::{MAX_ELEMENT_DEPTH, SanitizeOptions, sanitize_html, sanitize_signature_html};
pub use thelemail_mime::{InlinePart, InlineParts};
pub use tracking::{TRACKING_PARAMS, strip_tracking_params};
