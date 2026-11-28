use std::sync::LazyLock;

use regex::Regex;

use crate::js;

static URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?-u:\b)https?://[^{w}<>'")\]]+"#,
        w = js::WHITESPACE_CLASS
    ))
    .unwrap()
});

static EMAIL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?-u:\b)[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}(?-u:\b)").unwrap()
});

static PARAGRAPH_BREAK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\r?\n\r?\n+").unwrap());

static LINE_BREAK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\r?\n").unwrap());

pub(crate) fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn push_link(out: &mut String, href: &str, value: &str) {
    out.push_str("<a href=\"");
    out.push_str(href);
    out.push_str("\" target=\"_blank\" rel=\"noopener noreferrer\">");
    out.push_str(value);
    out.push_str("</a>");
}

fn linkify_emails(out: &mut String, text: &str) {
    let mut cursor = 0;
    for m in EMAIL_RE.find_iter(text) {
        out.push_str(&text[cursor..m.start()]);
        push_link(out, &format!("mailto:{}", m.as_str()), m.as_str());
        cursor = m.end();
    }
    out.push_str(&text[cursor..]);
}

fn linkify(escaped: &str) -> String {
    let mut out = String::with_capacity(escaped.len());
    let mut last = 0;
    for m in URL_RE.find_iter(escaped) {
        linkify_emails(&mut out, &escaped[last..m.start()]);
        push_link(&mut out, m.as_str(), m.as_str());
        last = m.end();
    }
    linkify_emails(&mut out, &escaped[last..]);
    out
}

pub fn plain_to_html(text: &str) -> String {
    let linked = linkify(&escape_html(text));
    let mut out = String::with_capacity(linked.len() + 16);
    for paragraph in PARAGRAPH_BREAK.split(&linked) {
        out.push_str("<p>");
        out.push_str(&LINE_BREAK.replace_all(paragraph, "<br>"));
        out.push_str("</p>");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_links() {
        assert_eq!(
            plain_to_html("a<b> https://x.test/?a=1&b=2 c@d.test"),
            "<p>a&lt;b&gt; <a href=\"https://x.test/?a=1&amp;b=2\" target=\"_blank\" rel=\"noopener noreferrer\">https://x.test/?a=1&amp;b=2</a> <a href=\"mailto:c@d.test\" target=\"_blank\" rel=\"noopener noreferrer\">c@d.test</a></p>"
        );
    }

    #[test]
    fn empty_text_is_one_empty_paragraph() {
        assert_eq!(plain_to_html(""), "<p></p>");
    }

    #[test]
    fn paragraphs_and_line_breaks() {
        assert_eq!(plain_to_html("a\nb\n\nc"), "<p>a<br>b</p><p>c</p>");
    }
}
