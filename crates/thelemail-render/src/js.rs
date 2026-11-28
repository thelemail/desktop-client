pub(crate) const WHITESPACE_CLASS: &str = r"\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}";

pub(crate) fn is_whitespace(c: char) -> bool {
    let space_separator = matches!(
        c,
        '\u{2000}'..='\u{200A}' | '\u{1680}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    );
    let line_terminator = matches!(c, '\u{2028}' | '\u{2029}');
    space_separator || line_terminator || matches!(c, '\t'..='\r' | ' ' | '\u{A0}' | '\u{FEFF}')
}

pub(crate) fn trim(s: &str) -> &str {
    s.trim_matches(is_whitespace)
}

pub(crate) fn starts_with_ignore_case(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len()
        && s.is_char_boundary(prefix.len())
        && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_matches_ecmascript_whitespace() {
        assert_eq!(trim("\u{FEFF}\u{A0} x \u{3000}\n"), "x");
        assert_eq!(trim("\u{85}x\u{85}"), "\u{85}x\u{85}");
        assert_eq!(trim("\u{180E}x"), "\u{180E}x");
    }

    #[test]
    fn prefix_is_ascii_case_insensitive() {
        assert!(starts_with_ignore_case("CID:x", "cid:"));
        assert!(!starts_with_ignore_case("ci", "cid:"));
        assert!(!starts_with_ignore_case("cié:", "cid:"));
    }
}
