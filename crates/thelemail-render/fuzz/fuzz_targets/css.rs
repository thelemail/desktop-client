#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let input = String::from_utf8_lossy(data);
    if let Err(why) = thelemail_render::fuzzing::check_css(&input) {
        panic!("{why}");
    }
});
