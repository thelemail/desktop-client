#![no_main]

use libfuzzer_sys::fuzz_target;
use thelemail_render::{BodySource, RenderOptions, render_body, strip_tracking_params};

fuzz_target!(|data: &[u8]| {
    let rendered = render_body(BodySource::Mime(data), RenderOptions { strip_tracking: true, utc_offset_seconds: 3_600 });
    if let Some(html) = &rendered.content_html
        && let Err(why) = thelemail_render::fuzzing::check_body(html)
    {
        panic!("{why}");
    }
    let href = String::from_utf8_lossy(data);
    let once = strip_tracking_params(&href);
    assert_eq!(strip_tracking_params(&once), once);
});
