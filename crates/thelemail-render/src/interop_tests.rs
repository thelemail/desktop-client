use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use std::collections::BTreeMap;

use crate::dom::canonical_body;
use crate::{InlinePart, InlineParts, SanitizeOptions, sanitize_html, sanitize_signature_html};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/render");

fn load<T: for<'de> Deserialize<'de>>(name: &str) -> T {
    let raw = std::fs::read_to_string(format!("{FIXTURES}/{name}")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Part {
    mime_type: String,
    data: String,
}

#[derive(Deserialize)]
struct SanitizeCase {
    name: String,
    input: String,
    strip: String,
    keep: String,
}

#[derive(Deserialize)]
struct SanitizeFixture {
    inline: BTreeMap<String, Part>,
    cases: Vec<SanitizeCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignatureCase {
    name: String,
    input: String,
    keep_remote_images: bool,
    output: String,
}

fn without_native_drops(html: &str) -> String {
    let mut dom = crate::dom::parse(&format!("<remove></remove>{html}"));
    let body = dom.first_element(crate::dom::DOCUMENT, "body").unwrap();
    for node in dom.descendants(body) {
        let tag = dom
            .element_name(node)
            .map(|n| n.local.to_string())
            .unwrap_or_default();
        if let Some(attrs) = dom.attrs_mut(node) {
            attrs.retain(|attr| {
                attr.name != "poster"
                    && !(attr.name == "src"
                        && ["audio", "input", "source", "track", "video"].contains(&tag.as_str()))
            });
        }
    }
    let first = dom.first_child(body).unwrap();
    dom.detach(first);
    dom.inner_html(body)
}

fn same_dom(rust: &str, web: &str) -> bool {
    canonical_body(rust) == canonical_body(&without_native_drops(web))
}

#[test]
fn sanitizer_matches_web() {
    let fixture: SanitizeFixture = load("sanitize.json");
    let inline: InlineParts = fixture
        .inline
        .into_iter()
        .map(|(cid, part)| {
            (
                cid,
                InlinePart {
                    mime_type: part.mime_type,
                    data: STANDARD.decode(part.data).unwrap(),
                },
            )
        })
        .collect();
    let mut failures = Vec::new();
    for case in &fixture.cases {
        for (strip, expected) in [(true, &case.strip), (false, &case.keep)] {
            let out = sanitize_html(
                &case.input,
                &SanitizeOptions {
                    inline: &inline,
                    strip_tracking: strip,
                },
            );
            if !same_dom(&out, expected) {
                failures.push(format!(
                    "{} strip={strip}\n  rust: {}\n  web:  {}",
                    case.name,
                    canonical_body(&out),
                    canonical_body(&without_native_drops(expected))
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn signature_sanitizer_matches_web() {
    let mut failures = Vec::new();
    for case in load::<Vec<SignatureCase>>("signature.json") {
        let out = sanitize_signature_html(&case.input, case.keep_remote_images);
        if !same_dom(&out, &case.output) {
            failures.push(format!(
                "{} keep={}\n  rust: {}\n  web:  {}",
                case.name,
                case.keep_remote_images,
                canonical_body(&out),
                canonical_body(&case.output)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
