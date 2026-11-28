use serde::Deserialize;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/render");

fn load<T: for<'de> Deserialize<'de>>(name: &str) -> T {
    let raw = std::fs::read_to_string(format!("{FIXTURES}/{name}")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[derive(Deserialize)]
struct Pair<I, O> {
    input: I,
    output: O,
}

#[test]
fn tracking_strip_matches_web() {
    for case in load::<Vec<Pair<String, String>>>("tracking.json") {
        assert_eq!(
            thelemail_render::strip_tracking_params(&case.input),
            case.output,
            "{}",
            case.input
        );
    }
}

#[test]
fn designed_detection_matches_web() {
    for case in load::<Vec<Pair<String, bool>>>("designed.json") {
        assert_eq!(
            thelemail_render::looks_designed(&case.input),
            case.output,
            "{}",
            case.input
        );
    }
}

#[test]
fn plain_to_html_matches_web() {
    for case in load::<Vec<Pair<String, String>>>("plain.json") {
        assert_eq!(
            thelemail_render::plain_to_html(&case.input),
            case.output,
            "{:?}",
            case.input
        );
    }
}

#[test]
fn css_scrub_matches_web() {
    for case in load::<Vec<Pair<String, String>>>("css.json") {
        assert_eq!(
            thelemail_render::scrub_css(&case.input),
            case.output,
            "{:?}",
            case.input
        );
    }
}
