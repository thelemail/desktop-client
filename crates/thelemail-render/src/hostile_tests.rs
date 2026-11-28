use std::time::{Duration, Instant};

use serde::Deserialize;
use thelemail_mime::{InlinePart, InlineParts};

use crate::invariants::{check_body, check_css, check_signature, rendered_violations};
use crate::{SanitizeOptions, sanitize_html};

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

#[test]
fn hostile_bodies_are_neutralised_as_rendered() {
    let failures: Vec<String> = vectors()
        .into_iter()
        .filter_map(|vector| {
            check_body(&vector.html)
                .err()
                .map(|why| format!("{}: {why}", vector.name))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn hostile_bodies_without_tracking_strip_are_neutralised() {
    let parts = inline();
    let options = SanitizeOptions {
        inline: &parts,
        strip_tracking: false,
    };
    let failures: Vec<String> = vectors()
        .into_iter()
        .flat_map(|vector| {
            let out = sanitize_html(&vector.html, &options);
            rendered_violations(&out, false)
                .into_iter()
                .map(move |why| format!("{}: {why}", vector.name))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn hostile_signatures_are_neutralised_as_rendered() {
    let failures: Vec<String> = vectors()
        .into_iter()
        .filter_map(|vector| {
            check_signature(&vector.html)
                .err()
                .map(|why| format!("{}: {why}", vector.name))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn hostile_style_sheets_scrub_to_a_fixed_point() {
    let failures: Vec<String> = vectors()
        .into_iter()
        .filter_map(|vector| {
            check_css(&vector.html)
                .err()
                .map(|why| format!("{}: {why}", vector.name))
        })
        .collect();
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
