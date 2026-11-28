use std::time::{Duration, Instant};

use serde::Deserialize;
use thelemail_mime::{InlinePart, InlineParts};

use crate::dom::{self, DOCUMENT, Data, Dom, NodeId, canonical_body};
use crate::{SanitizeOptions, build_src_doc, sanitize_html, sanitize_signature_html, scrub_css};

const FORBIDDEN_TAGS: [&str; 19] = [
    "applet",
    "base",
    "embed",
    "form",
    "frame",
    "frameset",
    "iframe",
    "link",
    "math",
    "meta",
    "noembed",
    "noframes",
    "noscript",
    "object",
    "plaintext",
    "portal",
    "script",
    "svg",
    "xmp",
];

const FORBIDDEN_ATTRS: [&str; 9] = [
    "background",
    "dynsrc",
    "formaction",
    "lowsrc",
    "ping",
    "poster",
    "srcdoc",
    "srcset",
    "data",
];

#[derive(Deserialize)]
struct Vector {
    name: String,
    html: String,
}

fn vectors() -> Vec<Vector> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/render/hostile.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn inline() -> InlineParts {
    InlineParts::from([(
        "logo".to_owned(),
        InlinePart {
            mime_type: "image/gif".to_owned(),
            data: b"GIF89a".to_vec(),
        },
    )])
}

fn normalized_url(value: &str) -> String {
    value
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect::<String>()
        .to_ascii_lowercase()
}

fn all_nodes(dom: &Dom, root: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut roots = vec![root];
    while let Some(root) = roots.pop() {
        for node in dom.descendants(root) {
            if let Some(contents) = dom.template_contents(node) {
                roots.push(contents);
            }
            out.push(node);
        }
    }
    out
}

fn violations(dom: &Dom, root: NodeId, remote_images: bool) -> Vec<String> {
    let mut found = Vec::new();
    for node in all_nodes(dom, root) {
        match &dom.nodes[node].data {
            Data::Comment(text) => found.push(format!("comment {text:?}")),
            Data::Element { name, attrs, .. } => {
                let tag = name.local.to_string();
                if name.ns != html5ever::ns!(html) {
                    found.push(format!("foreign element {tag}"));
                }
                if FORBIDDEN_TAGS.contains(&tag.as_str()) {
                    found.push(format!("element {tag}"));
                }
                if tag == "style" {
                    let text = dom.text_content(node);
                    if scrub_css(&text) != text {
                        found.push(format!("style sheet still loads {text:?}"));
                    }
                }
                for attr in attrs {
                    let lower = attr.name.to_ascii_lowercase();
                    if lower.starts_with("on") || FORBIDDEN_ATTRS.contains(&lower.as_str()) {
                        found.push(format!("{tag}@{lower}"));
                    }
                    let url = normalized_url(&attr.value);
                    if lower == "src" {
                        let remote_ok = remote_images
                            && (url.starts_with("https:") || url.starts_with("http:"));
                        if tag != "img"
                            || !(url.starts_with("data:") || url.starts_with("cid:") || remote_ok)
                        {
                            found.push(format!("{tag}@src={:?}", attr.value));
                        }
                    }
                    if (lower == "href" || lower.ends_with(":href") || lower == "action")
                        && ["javascript:", "vbscript:", "data:", "file:"]
                            .iter()
                            .any(|s| url.starts_with(s))
                    {
                        found.push(format!("{tag}@{lower}={:?}", attr.value));
                    }
                    if lower == "style" && scrub_css(&attr.value) != attr.value {
                        found.push(format!("{tag}@style still loads {:?}", attr.value));
                    }
                }
            }
            _ => {}
        }
    }
    found
}

fn rendered_violations(sanitized: &str, remote_images: bool) -> Vec<String> {
    let dom = dom::parse(&build_src_doc(sanitized, false));
    let body = dom.first_element(DOCUMENT, "body").unwrap();
    violations(&dom, body, remote_images)
}

#[test]
fn hostile_bodies_are_neutralised_as_rendered() {
    let parts = inline();
    let mut failures = Vec::new();
    for vector in vectors() {
        for strip_tracking in [true, false] {
            let out = sanitize_html(
                &vector.html,
                &SanitizeOptions {
                    inline: &parts,
                    strip_tracking,
                },
            );
            for violation in rendered_violations(&out, false) {
                failures.push(format!("{}: {violation}\n  output: {out}", vector.name));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn hostile_signatures_are_neutralised_as_rendered() {
    let mut failures = Vec::new();
    for vector in vectors() {
        for keep_remote_images in [false, true] {
            let out = sanitize_signature_html(&vector.html, keep_remote_images);
            for violation in rendered_violations(&out, keep_remote_images) {
                failures.push(format!(
                    "{} keep={keep_remote_images}: {violation}\n  output: {out}",
                    vector.name
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn sanitizing_twice_changes_nothing() {
    let parts = inline();
    let options = SanitizeOptions {
        inline: &parts,
        strip_tracking: true,
    };
    let mut failures = Vec::new();
    for vector in vectors() {
        let once = sanitize_html(&vector.html, &options);
        let twice = sanitize_html(&once, &options);
        if canonical_body(&once) != canonical_body(&twice) {
            failures.push(format!(
                "{}\n  once:  {once}\n  twice: {twice}",
                vector.name
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn output_survives_a_parse_round_trip() {
    let parts = inline();
    let options = SanitizeOptions {
        inline: &parts,
        strip_tracking: true,
    };
    let mut failures = Vec::new();
    for vector in vectors() {
        let out = sanitize_html(&vector.html, &options);
        let dom = dom::parse(&format!("<body>{out}"));
        let body = dom.first_element(DOCUMENT, "body").unwrap();
        let again = dom.inner_html(body);
        if again != out {
            failures.push(format!("{}\n  out:   {out}\n  again: {again}", vector.name));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn timed(html: &str) -> (String, Duration) {
    let parts = InlineParts::new();
    let started = Instant::now();
    let out = sanitize_html(
        html,
        &SanitizeOptions {
            inline: &parts,
            strip_tracking: true,
        },
    );
    (out, started.elapsed())
}

#[test]
fn oversized_input_stays_bounded() {
    let budget = Duration::from_secs(if cfg!(debug_assertions) { 30 } else { 3 });
    let cases = [
        ("deep divs", "<div>".repeat(200_000)),
        ("deep spans", format!("{}x", "<span>".repeat(10_000))),
        ("deep tables", "<table><tr><td>".repeat(5_000)),
        ("deep blockquotes", "<blockquote>".repeat(20_000)),
        (
            "stray end tags",
            format!("{}{}", "<div>".repeat(3_000), "</div>".repeat(200_000)),
        ),
        ("wide siblings", "<p>x</p>".repeat(100_000)),
        (
            "huge attribute",
            format!("<p title=\"{}\">x</p>", "a".repeat(4 << 20)),
        ),
        (
            "many attributes",
            format!(
                "<p {}>x</p>",
                (0..20_000)
                    .map(|i| format!("a{i}=1"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        ),
        (
            "deep css",
            format!(
                "<style>{}a{{background:url(https://x.test)}}{}</style>",
                "@media all{".repeat(500),
                "}".repeat(500)
            ),
        ),
        ("formatting storm", "<b><i><u><s>".repeat(20_000)),
    ];
    for (name, html) in cases {
        let (out, elapsed) = timed(&html);
        assert!(elapsed < budget, "{name} took {elapsed:?}");
        let violations = rendered_violations(&out, false);
        assert!(violations.is_empty(), "{name}: {violations:?}");
    }
}

#[test]
fn checker_flags_the_raw_corpus() {
    let unflagged: Vec<String> = vectors()
        .into_iter()
        .filter(|vector| rendered_violations(&vector.html, false).is_empty())
        .map(|vector| vector.name)
        .collect();
    assert_eq!(
        unflagged,
        ["attr-breakouts", "is-attribute", "attributename"]
    );
}
