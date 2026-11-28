use crate::dom::{self, DOCUMENT, Data, Dom, NodeId, canonical_body};
use crate::{
    InlinePart, InlineParts, SanitizeOptions, build_src_doc, sanitize_html,
    sanitize_signature_html, scrub_css,
};

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

fn normalized_url(value: &str) -> String {
    value
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect::<String>()
        .to_ascii_lowercase()
}

fn violations(dom: &Dom, root: NodeId, remote_images: bool) -> Vec<String> {
    let mut found = Vec::new();
    for node in dom.descendants_with_templates(root) {
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
                            || !(url.is_empty()
                                || url.starts_with("data:")
                                || url.starts_with("cid:")
                                || remote_ok)
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

pub(crate) fn rendered_violations(sanitized: &str, remote_images: bool) -> Vec<String> {
    let dom = dom::parse(&build_src_doc(sanitized, false));
    let body = dom.first_element(DOCUMENT, "body").unwrap();
    violations(&dom, body, remote_images)
}

fn fuzz_inline() -> InlineParts {
    InlineParts::from([(
        "logo".to_owned(),
        InlinePart {
            mime_type: "image/gif".to_owned(),
            data: b"GIF89a".to_vec(),
        },
    )])
}

fn reparse(html: &str) -> Result<String, String> {
    let dom = dom::parse(&format!("<body>{html}"));
    let body = dom.first_element(DOCUMENT, "body").ok_or("no body")?;
    Ok(dom.inner_html(body))
}

fn settles_after_one_parse(out: &str) -> Result<(), String> {
    let once = reparse(out)?;
    let twice = reparse(&once)?;
    if once == twice {
        Ok(())
    } else {
        Err(format!(
            "reparse keeps changing\n  out:   {out}\n  once:  {once}\n  twice: {twice}"
        ))
    }
}

pub fn check_body(input: &str) -> Result<(), String> {
    let parts = fuzz_inline();
    let options = SanitizeOptions {
        inline: &parts,
        strip_tracking: true,
    };
    let once = sanitize_html(input, &options);
    let found = rendered_violations(&once, false);
    if !found.is_empty() {
        return Err(format!("{found:?}\n  output: {once}"));
    }
    let twice = sanitize_html(&once, &options);
    if canonical_body(&once) != canonical_body(&twice) {
        return Err(format!(
            "not a fixed point\n  once:  {once}\n  twice: {twice}"
        ));
    }
    settles_after_one_parse(&once)
}

pub fn check_signature(input: &str) -> Result<(), String> {
    for keep_remote_images in [false, true] {
        let out = sanitize_signature_html(input, keep_remote_images);
        let found = rendered_violations(&out, keep_remote_images);
        if !found.is_empty() {
            return Err(format!("{found:?}\n  output: {out}"));
        }
    }
    Ok(())
}

pub fn check_css(input: &str) -> Result<(), String> {
    let once = scrub_css(input);
    let twice = scrub_css(&once);
    if once == twice {
        Ok(())
    } else {
        Err(format!(
            "css not a fixed point\n  once:  {once}\n  twice: {twice}"
        ))
    }
}
