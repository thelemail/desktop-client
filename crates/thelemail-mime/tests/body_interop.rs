use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use thelemail_mime::{BodyOptions, parse_body};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/render");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Part {
    mime_type: String,
    data: String,
}

#[derive(Deserialize)]
struct Expected {
    #[serde(default)]
    error: bool,
    #[serde(default)]
    html: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    inline: BTreeMap<String, Part>,
}

#[test]
fn body_selection_matches_postal_mime() {
    let raw = std::fs::read_to_string(format!("{FIXTURES}/mime.json")).unwrap();
    let expected: BTreeMap<String, Expected> = serde_json::from_str(&raw).unwrap();
    let mut failures = Vec::new();
    for (name, want) in &expected {
        let eml = std::fs::read(format!("{FIXTURES}/mime/{name}.eml")).unwrap();
        let got = parse_body(&eml, BodyOptions::default());
        let Ok(got) = got else {
            if !want.error {
                failures.push(format!("{name}: unexpected {got:?}"));
            }
            continue;
        };
        if want.error {
            failures.push(format!("{name}: expected an error"));
            continue;
        }
        if got.html != want.html {
            failures.push(format!(
                "{name} html\n  rust: {:?}\n  web:  {:?}",
                got.html, want.html
            ));
        }
        if got.text != want.text {
            failures.push(format!(
                "{name} text\n  rust: {:?}\n  web:  {:?}",
                got.text, want.text
            ));
        }
        let inline: BTreeMap<String, (String, Vec<u8>)> = want
            .inline
            .iter()
            .map(|(cid, part)| {
                (
                    cid.clone(),
                    (part.mime_type.clone(), STANDARD.decode(&part.data).unwrap()),
                )
            })
            .collect();
        let got_inline: BTreeMap<String, (String, Vec<u8>)> = got
            .inline
            .into_iter()
            .map(|(cid, part)| (cid, (part.mime_type, part.data)))
            .collect();
        if got_inline != inline {
            failures.push(format!(
                "{name} inline\n  rust: {got_inline:?}\n  web:  {inline:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
