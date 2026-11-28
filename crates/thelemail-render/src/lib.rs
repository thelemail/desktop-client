#![forbid(unsafe_code)]

mod css;
mod designed;
mod js;
mod plain;
mod tracking;

pub use css::scrub_css;
pub use designed::looks_designed;
pub use plain::plain_to_html;
pub use tracking::{TRACKING_PARAMS, strip_tracking_params};
