use std::sync::LazyLock;

use regex::Regex;

use crate::js;

fn ws() -> &'static str {
    js::WHITESPACE_CLASS
}

static BACKGROUND_DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?i-u:background(?:-color|-image)?)[{w}]*:[{w}]*([^;}}"']+)"#,
        w = ws()
    ))
    .unwrap()
});

static BGCOLOR_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?-u:\b)(?i-u:bgcolor)[{w}]*=[{w}]*["']?([^"'{w}>]+)"#,
        w = ws()
    ))
    .unwrap()
});

static BACKGROUND_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"<[^>]*[{w}](?i-u:background)[{w}]*=[{w}]*["']?[^"'{w}>]"#,
        w = ws()
    ))
    .unwrap()
});

static PLAIN_BACKGROUND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        concat!(
            r"^[{w}]*(?:(?i-u:transparent|none|inherit|initial|unset|white|#fff|#ffffff)",
            r"|(?i-u:rgba?)\([{w}]*255[{w}]*,[{w}]*255[{w}]*,[{w}]*255[{w}]*(?:,[{w}]*[0-9.]+[{w}]*)?\)",
            r"|(?i-u:rgba)\([^)]*,[{w}]*0(?:\.0+)?[{w}]*\))",
            r"[{w}]*(?:(?i-u:!important))?[{w}]*$"
        ),
        w = ws()
    ))
    .unwrap()
});

fn painted(value: &str) -> bool {
    !js::trim(value).is_empty() && !PLAIN_BACKGROUND.is_match(value)
}

fn any_painted(pattern: &Regex, html: &str) -> bool {
    pattern
        .captures_iter(html)
        .any(|caps| caps.get(1).is_some_and(|m| painted(m.as_str())))
}

pub fn looks_designed(html: &str) -> bool {
    any_painted(&BGCOLOR_ATTR, html)
        || BACKGROUND_ATTR.is_match(html)
        || any_painted(&BACKGROUND_DECL, html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_and_transparent_are_plain() {
        assert!(!looks_designed(r##"<td bgcolor="#FFFFFF">x</td>"##));
        assert!(!looks_designed(
            r#"<p style="background: rgba(1,2,3,0) !important">x</p>"#
        ));
    }

    #[test]
    fn any_paint_is_designed() {
        assert!(looks_designed(r##"<td bgcolor="#f4f4f4">x</td>"##));
        assert!(looks_designed("<style>.a{background:#0E6240}</style>"));
    }

    #[test]
    fn case_folding_stays_ascii() {
        assert!(!looks_designed("<p style=\"bac\u{212A}ground:#000\">x</p>"));
    }
}
