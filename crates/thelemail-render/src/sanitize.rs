use std::sync::LazyLock;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use html5ever::ns;
use regex::Regex;
use thelemail_mime::InlineParts;

use crate::allowlist::{self, contains};
use crate::css::scrub_css;
use crate::dom::{self, Attr, DOCUMENT, Data, Dom, NodeId};
use crate::js;
use crate::tracking::strip_tracking_params;

pub const MAX_ELEMENT_DEPTH: u32 = 512;

const BODY_FORBID_TAGS: [&str; 10] = [
    "base", "embed", "form", "iframe", "link", "math", "meta", "object", "script", "svg",
];

const SIGNATURE_FORBID_TAGS: [&str; 11] = [
    "base", "embed", "form", "iframe", "link", "math", "meta", "object", "script", "style", "svg",
];

const FORBID_ATTRS: [&str; 4] = ["background", "formaction", "ping", "srcset"];

const FORM_CLOBBER_PROPERTIES: [&str; 13] = [
    "attributes",
    "childNodes",
    "getAttributeNode",
    "hasChildNodes",
    "insertBefore",
    "namespaceURI",
    "nodeName",
    "nodeType",
    "removeAttribute",
    "removeAttributeNode",
    "removeChild",
    "setAttribute",
    "textContent",
];

const FORM_LISTED: [&str; 8] = [
    "button", "fieldset", "img", "input", "object", "output", "select", "textarea",
];

const MATHML_TEXT_INTEGRATION_POINTS: [&str; 5] = ["mi", "mn", "mo", "ms", "mtext"];

const SIGNATURE_IMAGE_ATTR: &str = "data-thelemail-sig-image";

static UNSAFE_ATTR_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"--!?>|\]>|</(?i-u:style|script|title|xmp|textarea|noscript|iframe|noembed|noframes)",
    )
    .unwrap()
});

pub struct SanitizeOptions<'a> {
    pub inline: &'a InlineParts,
    pub strip_tracking: bool,
}

enum Profile<'a> {
    Body(&'a SanitizeOptions<'a>),
    Signature { keep_remote_images: bool },
}

enum Verdict {
    Keep,
    Remove,
    Hoist,
}

fn markup_probe(text: &str, allow_bang: bool) -> bool {
    let bytes = text.as_bytes();
    bytes.windows(2).any(|pair| {
        pair[0] == b'<'
            && (pair[1] == b'/'
                || pair[1].is_ascii_alphanumeric()
                || pair[1] == b'_'
                || (allow_bang && pair[1] == b'!'))
    })
}

fn allowed_uri(value: &str, allow_data: bool) -> bool {
    let schemes: &[&str] = if allow_data {
        &["http:", "https:", "mailto:", "tel:", "cid:", "data:"]
    } else {
        &["http:", "https:", "mailto:", "tel:", "cid:"]
    };
    if schemes
        .iter()
        .any(|scheme| js::starts_with_ignore_case(value, scheme))
    {
        return true;
    }
    let mut chars = value.chars();
    match chars.next() {
        None => return false,
        Some(first) if !first.is_ascii_alphabetic() => return true,
        Some(_) => {}
    }
    for c in chars {
        if c.is_ascii_alphabetic() || matches!(c, '+' | '.' | '-') {
            continue;
        }
        return c != ':';
    }
    true
}

fn strip_attr_whitespace(value: &str) -> String {
    value
        .chars()
        .filter(|&c| {
            !matches!(c, '\u{0}'..='\u{20}' | '\u{A0}' | '\u{1680}' | '\u{180E}' | '\u{2000}'..='\u{2029}' | '\u{205F}' | '\u{3000}')
        })
        .collect()
}

fn is_data_attr(name: &str) -> bool {
    name.strip_prefix("data-").is_some_and(|rest| {
        !rest.is_empty()
            && rest.chars().all(|c| {
                c == '-' || c == '.' || c == '_' || c.is_ascii_alphanumeric() || c >= '\u{B7}'
            })
    })
}

fn is_aria_attr(name: &str) -> bool {
    name.strip_prefix("aria-").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .chars()
                .all(|c| c == '-' || c == '_' || c.is_ascii_alphanumeric())
    })
}

fn dangerous_link(value: &str) -> bool {
    let as_url_parser_sees_it: String = js::trim(value)
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    ["javascript:", "vbscript:", "data:"]
        .iter()
        .any(|scheme| js::starts_with_ignore_case(&as_url_parser_sees_it, scheme))
}

fn strip_angle_brackets(cid: &str) -> &str {
    let cid = cid.strip_prefix('<').unwrap_or(cid);
    cid.strip_suffix('>').unwrap_or(cid)
}

struct Walker<'a> {
    dom: &'a mut Dom,
    profile: Profile<'a>,
}

impl Walker<'_> {
    fn forbid_tags(&self) -> &'static [&'static str] {
        match self.profile {
            Profile::Body(_) => &BODY_FORBID_TAGS,
            Profile::Signature { .. } => &SIGNATURE_FORBID_TAGS,
        }
    }

    fn run(&mut self, root: NodeId) {
        let mut fragments = Vec::new();
        self.walk(root, true, &mut fragments);
        while let Some(fragment) = fragments.pop() {
            self.walk(fragment, false, &mut fragments);
        }
    }

    fn walk(&mut self, root: NodeId, include_root: bool, fragments: &mut Vec<NodeId>) {
        let mut cursor = if include_root {
            Some(root)
        } else {
            self.dom.first_child(root)
        };
        while let Some(node) = cursor {
            let verdict = self.verdict(node);
            match verdict {
                Verdict::Keep => {
                    self.sanitize_attributes(node);
                    if let Some(contents) = self.dom.template_contents(node) {
                        fragments.push(contents);
                    }
                    cursor = self.dom.next_in_tree(node, root);
                }
                _ if node == root => cursor = self.dom.first_child(root),
                Verdict::Remove => {
                    cursor = self.dom.following(node, root);
                    self.dom.detach(node);
                }
                Verdict::Hoist => {
                    self.dom.hoist_children(node);
                    cursor = self.dom.following(node, root);
                    self.dom.detach(node);
                }
            }
        }
    }

    fn tag(&self, node: NodeId) -> String {
        self.dom
            .element_name(node)
            .map(|name| name.local.to_lowercase())
            .unwrap_or_default()
    }

    fn verdict(&mut self, node: NodeId) -> Verdict {
        if !matches!(self.dom.nodes[node].data, Data::Element { .. }) {
            return match self.dom.nodes[node].data {
                Data::Text(_) => Verdict::Keep,
                _ => Verdict::Remove,
            };
        }
        let tag = self.tag(node);
        let html = self
            .dom
            .element_name(node)
            .is_some_and(|name| name.ns == ns!(html));
        if html && tag == "style" && matches!(self.profile, Profile::Body(_)) {
            self.scrub_style_text(node);
        }
        if html && tag == "form" && self.form_is_clobbered(node) {
            return Verdict::Remove;
        }
        if self.is_unsafe(node, &tag, html) {
            return Verdict::Remove;
        }
        if self.forbid_tags().contains(&tag.as_str()) || !contains(&allowlist::ALLOWED_TAGS, &tag) {
            return if contains(&allowlist::FORBID_CONTENTS, &tag) {
                Verdict::Remove
            } else {
                Verdict::Hoist
            };
        }
        if !self.valid_namespace(node, &tag) {
            return Verdict::Remove;
        }
        Verdict::Keep
    }

    fn scrub_style_text(&mut self, node: NodeId) {
        let text = self.dom.text_content(node);
        let scrubbed = scrub_css(&text);
        if scrubbed == text {
            return;
        }
        while let Some(child) = self.dom.first_child(node) {
            self.dom.detach(child);
        }
        let id = self.dom.new_text(scrubbed);
        self.dom.append_child(node, id);
    }

    fn form_is_clobbered(&self, form: NodeId) -> bool {
        self.dom.descendants(form).into_iter().any(|id| {
            self.dom
                .element_name(id)
                .is_some_and(|name| name.ns == ns!(html) && FORM_LISTED.contains(&&*name.local))
                && ["id", "name"].iter().any(|attr| {
                    self.dom
                        .attr(id, attr)
                        .is_some_and(|value| FORM_CLOBBER_PROPERTIES.contains(&value))
                })
        })
    }

    fn is_unsafe(&self, node: NodeId, tag: &str, html: bool) -> bool {
        let has_children = self.dom.first_child(node).is_some();
        let has_element_child = self
            .dom
            .children(node)
            .any(|child| matches!(self.dom.nodes[child].data, Data::Element { .. }));
        if has_children && !has_element_child {
            let text = self.dom.text_content(node);
            if markup_probe(&text, true) && markup_probe(&self.dom.inner_html(node), true) {
                return true;
            }
        }
        html && contains(&allowlist::LITERAL_TEXT, tag)
            && (has_element_child || markup_probe(&self.dom.text_content(node), true))
    }

    fn valid_namespace(&self, node: NodeId, tag: &str) -> bool {
        let Some(name) = self.dom.element_name(node) else {
            return false;
        };
        let parent = self.dom.nodes[node]
            .parent
            .and_then(|p| self.dom.element_name(p));
        let (parent_ns, parent_tag) = match parent {
            Some(parent) => (parent.ns.clone(), parent.local.to_lowercase()),
            None => (ns!(html), "template".to_owned()),
        };
        let html_integration = parent_tag == "annotation-xml";
        let mathml_text = MATHML_TEXT_INTEGRATION_POINTS.contains(&parent_tag.as_str());
        if name.ns == ns!(svg) {
            return if parent_ns == ns!(html) {
                tag == "svg"
            } else if parent_ns == ns!(mathml) {
                tag == "svg" && (html_integration || mathml_text)
            } else {
                contains(&allowlist::SVG_TAGS, tag)
            };
        }
        if name.ns == ns!(mathml) {
            return if parent_ns == ns!(html) {
                tag == "math"
            } else if parent_ns == ns!(svg) {
                tag == "math" && html_integration
            } else {
                contains(&allowlist::MATHML_TAGS, tag)
            };
        }
        if name.ns != ns!(html) {
            return false;
        }
        if parent_ns == ns!(svg) && !html_integration {
            return false;
        }
        if parent_ns == ns!(mathml) && !mathml_text {
            return false;
        }
        !contains(&allowlist::MATHML_TAGS, tag)
            && (allowlist::COMMON_SVG_AND_HTML.contains(&tag)
                || !contains(&allowlist::SVG_TAGS, tag))
    }

    fn valid_attribute(&self, tag: &str, name: &str, value: &str) -> bool {
        if FORBID_ATTRS.contains(&name)
            || name == "patchsrc"
            || (name == "for" && tag != "label" && tag != "output")
        {
            return false;
        }
        if (name == "id" || name == "name") && contains(&allowlist::CLOBBER_NAMES, value) {
            return false;
        }
        if is_data_attr(name) || is_aria_attr(name) {
            return true;
        }
        if !contains(&allowlist::ALLOWED_ATTRS, name) {
            return false;
        }
        if contains(&allowlist::URI_SAFE_ATTRS, name) {
            return true;
        }
        let allow_data = matches!(self.profile, Profile::Body(_));
        if allowed_uri(&strip_attr_whitespace(value), allow_data) {
            return true;
        }
        if matches!(name, "src" | "xlink:href" | "href")
            && value.starts_with("data:")
            && contains(&allowlist::DATA_URI_TAGS, tag)
        {
            return true;
        }
        value.is_empty()
    }

    fn body_hook(
        &self,
        options: &SanitizeOptions<'_>,
        tag: &str,
        name: &str,
        value: &mut String,
    ) -> bool {
        if name == "src" && tag == "img" {
            let trimmed = js::trim(value);
            if js::starts_with_ignore_case(trimmed, "cid:") {
                return match options.inline.get(strip_angle_brackets(&trimmed[4..])) {
                    Some(part) => {
                        *value = format!(
                            "data:{};base64,{}",
                            part.mime_type,
                            STANDARD.encode(&part.data)
                        );
                        true
                    }
                    None => false,
                };
            }
            return js::starts_with_ignore_case(trimmed, "data:image/");
        }
        if name == "href" && (tag == "a" || tag == "area") {
            return !dangerous_link(value);
        }
        if name == "style" {
            *value = scrub_css(value);
        }
        true
    }

    fn signature_hook(
        &self,
        node: NodeId,
        keep_remote_images: bool,
        tag: &str,
        name: &str,
        value: &mut String,
    ) -> bool {
        if name == "style" {
            *value = scrub_css(value);
            return true;
        }
        if name == "href" {
            return !dangerous_link(value);
        }
        if name == "src" && tag == "img" {
            if self.dom.attr(node, SIGNATURE_IMAGE_ATTR).is_some() {
                return true;
            }
            let remote = js::starts_with_ignore_case(value, "http:")
                || js::starts_with_ignore_case(value, "https:");
            let data_image = js::starts_with_ignore_case(value, "data:image/");
            return (keep_remote_images && (remote || data_image))
                || js::starts_with_ignore_case(value, "cid:");
        }
        true
    }

    fn native_hook(tag: &str, name: &str) -> bool {
        name != "poster"
            && (name != "src" || tag == "img")
            && (name != "href" || tag == "a" || tag == "area")
    }

    fn sanitize_attributes(&mut self, node: NodeId) {
        if !matches!(self.dom.nodes[node].data, Data::Element { .. }) {
            return;
        }
        let tag = self.tag(node);
        let mut index = self.dom.attrs(node).len();
        while index > 0 {
            index -= 1;
            let Attr {
                name: raw_name,
                value: initial,
            } = self.dom.attrs(node)[index].clone();
            let name = raw_name.to_lowercase();
            let mut value = if raw_name == "value" {
                initial.clone()
            } else {
                js::trim(&initial).to_owned()
            };
            let keep = Self::native_hook(&tag, &name)
                && match &self.profile {
                    Profile::Body(options) => self.body_hook(options, &tag, &name, &mut value),
                    Profile::Signature { keep_remote_images } => {
                        self.signature_hook(node, *keep_remote_images, &tag, &name, &mut value)
                    }
                };
            let unsafe_value = UNSAFE_ATTR_VALUE.is_match(&value)
                || (name == "attributename" && value.contains("href"));
            if unsafe_value || !keep || !self.valid_attribute(&tag, &name, &value) {
                self.remove_attribute(node, index, &raw_name);
                continue;
            }
            if value != initial
                && let Some(attrs) = self.dom.attrs_mut(node)
            {
                attrs[index].value = value;
            }
        }
        if let Profile::Body(options) = &self.profile
            && self.dom.is_html(node, &tag)
            && (tag == "a" || tag == "area")
            && let Some(href) = self.dom.attr(node, "href").map(str::to_owned)
        {
            if options.strip_tracking {
                let cleaned = strip_tracking_params(&href);
                if cleaned != href {
                    self.dom.set_attr(node, "href", &cleaned);
                }
            }
            self.dom.set_attr(node, "target", "_blank");
            self.dom.set_attr(node, "rel", "noopener noreferrer");
        }
    }

    fn remove_attribute(&mut self, node: NodeId, index: usize, name: &str) {
        if let Some(attrs) = self.dom.attrs_mut(node) {
            attrs.remove(index);
            if name == "is" {
                attrs.push(Attr {
                    name: name.to_owned(),
                    value: String::new(),
                });
            }
        }
    }
}

fn limit_depth(dom: &mut Dom, root: NodeId) {
    let mut stack: Vec<(NodeId, u32)> = dom.children(root).map(|child| (child, 1)).collect();
    stack.reverse();
    while let Some((node, depth)) = stack.pop() {
        let children: Vec<NodeId> = dom.children(node).collect();
        let element = matches!(dom.nodes[node].data, Data::Element { .. });
        if element && depth > MAX_ELEMENT_DEPTH {
            dom.hoist_children(node);
            dom.detach(node);
            stack.extend(children.into_iter().rev().map(|child| (child, depth)));
        } else {
            stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
        }
    }
}

fn sanitize_body(dom: &mut Dom, body: NodeId, profile: Profile<'_>) {
    Walker { dom, profile }.run(body);
    limit_depth(dom, body);
}

const MAX_PASSES: usize = 4;

pub fn sanitize_html(raw: &str, options: &SanitizeOptions<'_>) -> String {
    let mut current = sanitize_pass(raw, options);
    for _ in 0..MAX_PASSES {
        let next = sanitize_pass(&current, options);
        if next == current {
            break;
        }
        current = next;
    }
    current
}

fn sanitize_pass(raw: &str, options: &SanitizeOptions<'_>) -> String {
    if raw.is_empty() {
        return String::new();
    }
    if !raw.contains('<') {
        return raw.to_owned();
    }
    let mut dom = dom::parse(&format!("<remove></remove>{raw}"));
    let Some(body) = dom.first_element(DOCUMENT, "body") else {
        return String::new();
    };
    if let Some(first) = dom.first_child(body) {
        dom.detach(first);
    }
    sanitize_body(&mut dom, body, Profile::Body(options));
    dom.inner_html(body)
}

fn purify_signature(raw: &str, keep_remote_images: bool) -> String {
    if !raw.contains('<') {
        return raw.to_owned();
    }
    let leading = &raw[..raw.len() - raw.trim_start_matches(['\r', '\n', '\t', ' ']).len()];
    let mut dom = dom::parse(raw);
    let Some(body) = dom.first_element(DOCUMENT, "body") else {
        return String::new();
    };
    if !leading.is_empty() {
        let id = dom.new_text(leading.to_owned());
        let first = dom.first_child(body);
        dom.insert_before(body, first, id);
    }
    sanitize_body(&mut dom, body, Profile::Signature { keep_remote_images });
    dom.inner_html(body)
}

pub fn sanitize_signature_html(html: &str, keep_remote_images: bool) -> String {
    if html.is_empty() {
        return String::new();
    }
    let clean = purify_signature(html, keep_remote_images);
    let mut dom = dom::parse(&format!("<div>{clean}</div>"));
    let Some(body) = dom.first_element(DOCUMENT, "body") else {
        return clean;
    };
    let Some(wrapper) = dom
        .children(body)
        .find(|&child| matches!(dom.nodes[child].data, Data::Element { .. }))
    else {
        return clean;
    };
    for img in dom.descendants_with_templates(wrapper) {
        if !dom.is_html(img, "img") {
            continue;
        }
        if dom.attr(img, SIGNATURE_IMAGE_ATTR).is_some() {
            dom.set_attr(img, "src", "");
            continue;
        }
        let src = dom.attr(img, "src").unwrap_or_default();
        let remote =
            js::starts_with_ignore_case(src, "http:") || js::starts_with_ignore_case(src, "https:");
        let keepable = (keep_remote_images
            && (remote || js::starts_with_ignore_case(src, "data:image/")))
            || js::starts_with_ignore_case(src, "cid:");
        if !keepable {
            dom.detach(img);
        }
    }
    dom.inner_html(wrapper)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_rule_matches_the_web_regexp() {
        assert!(allowed_uri("https://x", true));
        assert!(allowed_uri("#a", true));
        assert!(allowed_uri("calloway.test", true));
        assert!(allowed_uri("a+b.c-d", true));
        assert!(!allowed_uri("javascript:x", true));
        assert!(!allowed_uri("ftp:x", true));
        assert!(!allowed_uri("data:x", false));
        assert!(!allowed_uri("", true));
        assert!(allowed_uri("\u{212A}:x", true));
    }

    #[test]
    fn links_are_judged_as_a_url_parser_reads_them() {
        assert!(dangerous_link("\u{18}\u{1}data:text/html,x"));
        assert!(dangerous_link(" java\tscript:alert(1)"));
        assert!(dangerous_link("\u{FEFF}JavaScript:x"));
        assert!(!dangerous_link("https://calloway.test/data:x"));
        assert!(!dangerous_link("\u{202E}javascript:x"));
    }

    #[test]
    fn markup_probe_needs_a_tag_start() {
        assert!(markup_probe("a<b", true));
        assert!(markup_probe("<!x", true));
        assert!(!markup_probe("<!x", false));
        assert!(!markup_probe("a < b", true));
    }

    #[test]
    fn plain_text_input_is_returned_as_is() {
        let none = InlineParts::new();
        assert_eq!(
            sanitize_html(
                "a & b",
                &SanitizeOptions {
                    inline: &none,
                    strip_tracking: true
                }
            ),
            "a & b"
        );
    }

    #[test]
    fn output_depth_is_bounded() {
        let none = InlineParts::new();
        let html = format!("{}x", "<span>".repeat(2_000));
        let out = sanitize_html(
            &html,
            &SanitizeOptions {
                inline: &none,
                strip_tracking: true,
            },
        );
        let dom = dom::parse(&format!("<body>{out}"));
        let body = dom.first_element(DOCUMENT, "body").unwrap();
        let mut depth = 0;
        let mut cursor = dom.first_child(body);
        while let Some(node) = cursor {
            depth += 1;
            cursor = dom.first_child(node);
        }
        assert!(depth <= MAX_ELEMENT_DEPTH as usize + 1);
        assert!(out.ends_with('x') || out.contains('x'));
    }
}
