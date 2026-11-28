use std::sync::LazyLock;

use regex::Regex;

pub(crate) const WS: &str = r"\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}";

const NOT_LINE_END: &str = r"[^\n\r\u{2028}\u{2029}]";

pub(crate) fn is_js_whitespace(c: char) -> bool {
    let space_separator = matches!(
        c,
        '\u{2000}'..='\u{200A}' | '\u{1680}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    );
    space_separator
        || matches!(
            c,
            '\t'..='\r' | ' ' | '\u{A0}' | '\u{FEFF}' | '\u{2028}' | '\u{2029}'
        )
}

pub(crate) fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

fn re(pattern: &str) -> Regex {
    Regex::new(&pattern.replace("{ws}", WS).replace("{dot}", NOT_LINE_END)).unwrap()
}

pub(crate) fn escape_html(s: &str) -> String {
    let mut out = String::new();
    for c in js_trim(s).chars() {
        match c {
            '<' | '>' | '"' | '\'' | '?' | '&' => out.push_str(&format!("&#x{:02X};", c as u32)),
            _ => out.push(c),
        }
    }
    out
}

pub(crate) fn text_to_html(text: &str) -> String {
    format!("<div>{}</div>", escape_html(text).replace('\n', "<br />"))
}

static ENTITY: LazyLock<Regex> =
    LazyLock::new(|| re(r"&(#[0-9]+|#[xX][a-fA-F0-9]+|[a-zA-Z]+[0-9]*);?"));

fn decode_numeric(entity: &str) -> String {
    let code = if entity.as_bytes().get(1) == Some(&b'x') {
        u64::from_str_radix(&entity[2..], 16).unwrap_or(u64::MAX)
    } else if entity[1..].bytes().all(|b| b.is_ascii_digit()) {
        entity[1..].parse::<u64>().unwrap_or(u64::MAX)
    } else {
        0
    };
    if (0xD800..=0xDFFF).contains(&code) || code > 0x10FFFF {
        return "\u{FFFD}".to_owned();
    }
    char::from_u32(code as u32)
        .map(String::from)
        .unwrap_or_else(|| "\u{FFFD}".to_owned())
}

pub(crate) fn decode_html_entities(s: &str) -> String {
    ENTITY
        .replace_all(s, |caps: &regex::Captures<'_>| {
            let whole = &caps[0];
            let entity = &caps[1];
            if let Some(&(first, second)) = web_atoms::NAMED_ENTITIES.get(&whole[1..])
                && first != 0
            {
                let mut out = String::new();
                out.extend(char::from_u32(first));
                if second != 0 {
                    out.extend(char::from_u32(second));
                }
                return out;
            }
            if !entity.starts_with('#') || !whole.ends_with(';') {
                return whole.to_owned();
            }
            decode_numeric(entity)
        })
        .into_owned()
}

static HTML_TO_TEXT: LazyLock<Vec<(Regex, &'static str, bool)>> = LazyLock::new(|| {
    vec![
        (re(r"\r?\n"), "\u{1}", true),
        (re(r"<!--{dot}*?-->"), " ", true),
        (re(r"<(?i-u:br)(?-u:\b)[^>]*>"), "\n", true),
        (
            re(r"</?(?i-u:p|div|table|tr|td|th)(?-u:\b)[^>]*>"),
            "\n\n",
            true,
        ),
        (
            re(r"<(?i-u:script)(?-u:\b)[^>]*>{dot}*?</(?i-u:script)(?-u:\b)[^>]*>"),
            " ",
            true,
        ),
        (re(r"^{dot}*<(?i-u:body)(?-u:\b)[^>]*>"), "", false),
        (re(r"^{dot}*</(?i-u:head)(?-u:\b)[^>]*>"), "", false),
        (re(r"^{dot}*<!(?i-u:doctype)(?-u:\b)[^>]*>"), "", false),
        (re(r"</(?i-u:body)(?-u:\b)[^>]*>{dot}*$"), "", false),
        (re(r"</(?i-u:html)(?-u:\b)[^>]*>{dot}*$"), "", false),
        (
            re(r#"<(?i-u:a)(?-u:\b)[^>]*(?i-u:href)[{ws}]*=[{ws}]*["']?([^{ws}"']+)[^>]*>"#),
            " ($1) ",
            true,
        ),
        (
            re(r"</?(?i-u:span|em|i|strong|b|u|a)(?-u:\b)[^>]*>"),
            "",
            true,
        ),
        (re(r"<(?i-u:li)(?-u:\b)[^>]*>[\n\u{1}{ws}]*"), "* ", true),
        (re(r"<hr(?-u:\b)[^>]*>"), "\n-------------\n", true),
        (re(r"<[^>]*>"), " ", true),
        (re(r"\u{1}"), "\n", true),
        (re(r"[ \t]+"), " ", true),
    ]
});

static PARAGRAPHS: LazyLock<Regex> = LazyLock::new(|| re(r"\n\n+"));
static LEADING_NEWLINES: LazyLock<Regex> = LazyLock::new(|| re(r"^\n+"));
static TRAILING_NEWLINES: LazyLock<Regex> = LazyLock::new(|| re(r"\n+$"));

fn is_js_line_end(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn remove_blank_lines(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let line_start = i == 0 || is_js_line_end(chars[i - 1]);
        if line_start && is_js_whitespace(chars[i]) {
            let mut end = i;
            while end < chars.len() && is_js_whitespace(chars[end]) {
                end += 1;
            }
            while end > i && end < chars.len() && !is_js_line_end(chars[end]) {
                end -= 1;
            }
            if end > i {
                i = end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

pub(crate) fn html_to_text(html: &str) -> String {
    let mut s = html.to_owned();
    for (pattern, replacement, global) in HTML_TO_TEXT.iter() {
        s = if *global {
            pattern.replace_all(&s, *replacement).into_owned()
        } else {
            pattern.replace(&s, *replacement).into_owned()
        };
    }
    s = remove_blank_lines(&s);
    s = PARAGRAPHS.replace_all(&s, "\n\n").into_owned();
    s = LEADING_NEWLINES.replace(&s, "\n").into_owned();
    s = TRAILING_NEWLINES.replace(&s, "\n").into_owned();
    decode_html_entities(&s)
}

#[derive(Debug, Clone)]
pub(crate) enum Mailbox {
    Single {
        name: String,
        address: Option<String>,
    },
    Group {
        name: String,
        members: Vec<Mailbox>,
    },
}

pub(crate) enum SubDate {
    Valid(i64),
    Invalid,
}

pub(crate) struct SubMessage {
    pub from: Option<Mailbox>,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    pub bcc: Vec<Mailbox>,
    pub subject: Option<String>,
    pub date: Option<SubDate>,
    pub html: Option<String>,
    pub text: Option<String>,
}

pub(crate) struct HeaderError;

fn civil(timestamp: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = timestamp.div_euclid(86_400);
    let secs = timestamp.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year,
        month,
        day,
        (secs / 3_600) as u32,
        (secs % 3_600 / 60) as u32,
        (secs % 60) as u32,
    )
}

fn iso_date(timestamp: i64) -> String {
    let (y, mo, d, h, mi, s) = civil(timestamp);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.000Z")
}

fn display_date(timestamp: i64, utc_offset_seconds: i32) -> String {
    let (y, mo, d, h, mi, s) = civil(timestamp + i64::from(utc_offset_seconds));
    format!("{mo}/{d}/{y}, {h:02}:{mi:02}:{s:02}")
}

fn dates(
    message: &SubMessage,
    utc_offset_seconds: i32,
) -> Result<Option<(String, String)>, HeaderError> {
    match message.date {
        None => Ok(None),
        Some(SubDate::Invalid) => Err(HeaderError),
        Some(SubDate::Valid(ts)) => Ok(Some((iso_date(ts), display_date(ts, utc_offset_seconds)))),
    }
}

fn html_address(mailbox: &Mailbox) -> Result<String, HeaderError> {
    let Mailbox::Single {
        name,
        address: Some(address),
    } = mailbox
    else {
        return Err(HeaderError);
    };
    let label = if name.is_empty() {
        format!("<{address}>")
    } else {
        name.clone()
    };
    Ok(format!(
        "<a href=\"mailto:{}\" class=\"postal-email-address\">{}</a>",
        escape_html(address),
        escape_html(&label)
    ))
}

fn html_addresses(list: &[Mailbox]) -> Result<String, HeaderError> {
    fn walk(list: &[Mailbox], parts: &mut Vec<String>) -> Result<(), HeaderError> {
        for (index, mailbox) in list.iter().enumerate() {
            if index > 0 {
                parts.push("<span class=\"postal-email-address-separator\">, </span>".to_owned());
            }
            match mailbox {
                Mailbox::Group { name, members } => {
                    parts.push(format!(
                        "<span class=\"postal-email-address-group\">{}:</span>",
                        escape_html(name)
                    ));
                    walk(members, parts)?;
                    parts.push("<span class=\"postal-email-address-group\">;</span>".to_owned());
                }
                single => parts.push(html_address(single)?),
            }
        }
        Ok(())
    }
    let mut parts = Vec::new();
    walk(list, &mut parts)?;
    Ok(parts.join(" "))
}

fn text_address(mailbox: &Mailbox) -> String {
    match mailbox {
        Mailbox::Single { name, address } if !name.is_empty() => {
            format!("{name} <{}>", address.as_deref().unwrap_or("undefined"))
        }
        Mailbox::Single { address, .. } => address.clone().unwrap_or_default(),
        Mailbox::Group { name, .. } if !name.is_empty() => format!("{name} <undefined>"),
        Mailbox::Group { .. } => String::new(),
    }
}

fn text_addresses(list: &[Mailbox]) -> String {
    fn walk(list: &[Mailbox], parts: &mut String) {
        for (index, mailbox) in list.iter().enumerate() {
            if index > 0 {
                parts.push_str(", ");
            }
            match mailbox {
                Mailbox::Group { name, members } => {
                    parts.push_str(name);
                    parts.push(':');
                    walk(members, parts);
                    parts.push(';');
                }
                single => parts.push_str(&text_address(single)),
            }
        }
    }
    let mut out = String::new();
    walk(list, &mut out);
    out
}

pub(crate) fn format_html_header(
    message: &SubMessage,
    utc_offset_seconds: i32,
) -> Result<String, HeaderError> {
    let mut rows = Vec::new();
    if let Some(from) = &message.from {
        rows.push(format!(
            "<div class=\"postal-email-header-key\">From</div><div class=\"postal-email-header-value\">{}</div>",
            html_address(from)?
        ));
    }
    if let Some(subject) = message.subject.as_deref().filter(|s| !s.is_empty()) {
        rows.push(format!(
            "<div class=\"postal-email-header-key\">Subject</div><div class=\"postal-email-header-value postal-email-header-subject\">{}</div>",
            escape_html(subject)
        ));
    }
    if let Some((iso, shown)) = dates(message, utc_offset_seconds)? {
        rows.push(format!(
            "<div class=\"postal-email-header-key\">Date</div><div class=\"postal-email-header-value postal-email-header-date\" data-date=\"{}\">{}</div>",
            escape_html(&iso),
            escape_html(&shown)
        ));
    }
    for (key, list) in [
        ("To", &message.to),
        ("Cc", &message.cc),
        ("Bcc", &message.bcc),
    ] {
        if !list.is_empty() {
            rows.push(format!(
                "<div class=\"postal-email-header-key\">{key}</div><div class=\"postal-email-header-value\">{}</div>",
                html_addresses(list)?
            ));
        }
    }
    let (open, close) = if rows.is_empty() {
        ("", "")
    } else {
        ("<div class=\"postal-email-header-row\">", "</div>")
    };
    Ok(format!(
        "<div class=\"postal-email-header\">{open}{}{close}</div>",
        rows.join("</div>\n<div class=\"postal-email-header-row\">")
    ))
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn is_ws16(unit: u16) -> bool {
    char::from_u32(u32::from(unit)).is_some_and(is_js_whitespace)
}

fn fold_lines(text: &str, line_length: usize) -> String {
    let units = utf16(text);
    let len = units.len();
    let mut pos = 0;
    let mut result: Vec<u16> = Vec::new();
    while pos < len {
        let mut line: Vec<u16> = units[pos..(pos + line_length).min(len)].to_vec();
        if line.len() < line_length {
            result.extend(&line);
            break;
        }
        if let Some(break_at) = line
            .iter()
            .position(|&u| u == u16::from(b'\n') || u == u16::from(b'\r'))
        {
            let mut end = break_at + 1;
            if line[break_at] == u16::from(b'\r')
                && line.get(break_at + 1) == Some(&u16::from(b'\n'))
            {
                end += 1;
            }
            result.extend(&line[..end]);
            pos += end;
            continue;
        }
        let tail_start = line.iter().rposition(|&u| is_ws16(u)).map(|last_ws| {
            let mut start = last_ws;
            while start > 0 && is_ws16(line[start - 1]) {
                start -= 1;
            }
            (start, last_ws + 1)
        });
        let folded_at_space = tail_start.and_then(|(start, ws_end)| {
            let matched = line.len() - start;
            let spaces = ws_end - start;
            (matched - spaces < line.len()).then_some(line.len() - (matched - spaces))
        });
        if let Some(cut) = folded_at_space {
            line.truncate(cut);
        } else {
            let rest = &units[pos + line.len()..];
            let word = rest.iter().take_while(|&&u| !is_ws16(u)).count();
            if word > 0 {
                let spaces = rest[word..].iter().take_while(|&&u| is_ws16(u)).count();
                line.extend(&rest[..word + spaces]);
            }
        }
        result.extend(&line);
        pos += line.len();
        if pos < len {
            result.extend(utf16("\r\n"));
        }
    }
    String::from_utf16_lossy(&result)
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

pub(crate) fn format_text_header(
    message: &SubMessage,
    utc_offset_seconds: i32,
) -> Result<String, HeaderError> {
    let mut rows: Vec<(&str, String)> = Vec::new();
    if let Some(from) = &message.from {
        rows.push(("From", text_address(from)));
    }
    if let Some(subject) = message.subject.as_deref().filter(|s| !s.is_empty()) {
        rows.push(("Subject", subject.to_owned()));
    }
    if let Some((_, shown)) = dates(message, utc_offset_seconds)? {
        rows.push(("Date", shown));
    }
    for (key, list) in [
        ("To", &message.to),
        ("Cc", &message.cc),
        ("Bcc", &message.bcc),
    ] {
        if !list.is_empty() {
            rows.push((key, text_addresses(list)));
        }
    }
    let key_width = rows.iter().map(|(key, _)| key.len()).max().unwrap_or(0);
    let mut lines = Vec::new();
    for (key, value) in &rows {
        let pad = " ".repeat(key_width - key.len());
        let prefix = format!("{key}: {pad}");
        let empty_prefix = format!("{} {pad}", " ".repeat(key.len() + 1));
        let folded = fold_lines(value, 80);
        for (index, line) in LINE_SPLIT.split(&folded).enumerate() {
            lines.push(format!(
                "{}{}",
                if index > 0 { &empty_prefix } else { &prefix },
                js_trim(line)
            ));
        }
    }
    let marker = "-".repeat(lines.iter().map(|line| utf16_len(line)).max().unwrap_or(0));
    Ok(format!("\n{marker}\n{}\n{marker}\n", lines.join("\n")))
}

static LINE_SPLIT: LazyLock<Regex> = LazyLock::new(|| re(r"\r?\n"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_follow_the_postal_rules() {
        assert_eq!(
            decode_html_entities("&amp; &AMP; &copy &#169; &#xA9; &#65 &bogus; &#xD800; &#X41;"),
            "& & © © © &#65 &bogus; \u{FFFD} \u{0}"
        );
    }

    #[test]
    fn html_to_text_matches_postal_shapes() {
        assert_eq!(html_to_text("<p>a<br>b</p>"), "\na\nb\n");
        assert_eq!(html_to_text("<ul><li>one</li></ul>"), " * one ");
        assert_eq!(html_to_text("x\n  \n\n  y\n \n"), "x\n\n y\n");
        assert_eq!(
            html_to_text("<a href=\"https://x\">l</a><hr><HR>"),
            " (https://x) l\n-------------\n"
        );
    }

    #[test]
    fn text_to_html_escapes_with_hex_references() {
        assert_eq!(
            text_to_html(" a & <b>?\nc "),
            "<div>a &#x26; &#x3C;b&#x3E;&#x3F;<br />c</div>"
        );
    }

    #[test]
    fn civil_dates_are_gregorian() {
        assert_eq!(iso_date(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_date(1_793_696_400), "2026-11-03T09:00:00.000Z");
        assert_eq!(display_date(1_793_696_400, 3_600), "11/3/2026, 10:00:00");
    }

    #[test]
    fn long_values_fold_after_spaces() {
        let value = format!("{} tail", "word ".repeat(20));
        assert!(fold_lines(&value, 80).contains("\r\n"));
    }
}
