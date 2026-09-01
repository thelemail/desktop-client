use base64::Engine as _;

pub const MAX_PGP_LAYERS: usize = 3;

const BEGIN: &str = "-----BEGIN PGP MESSAGE-----";
const END: &str = "-----END PGP MESSAGE-----";

fn split_part(raw: &str) -> (&str, &str) {
    let mut search = 0usize;
    while let Some(found) = raw[search..].find('\n') {
        let at = search + found;
        let rest = &raw[at..];
        for sep in ["\r\n\r\n", "\n\n", "\r\n\n", "\n\r\n"] {
            if rest.starts_with(sep) {
                return (&raw[..at], &raw[at + sep.len()..]);
            }
        }
        search = at + 1;
    }
    (raw, "")
}

fn unfold(headers: &str) -> String {
    let mut out = String::with_capacity(headers.len());
    let mut chars = headers.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' || c == '\n' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            match chars.peek() {
                Some(' ') | Some('\t') => {
                    while matches!(chars.peek(), Some(' ') | Some('\t')) {
                        chars.next();
                    }
                    out.push(' ');
                }
                _ => out.push('\n'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn header_value(headers: &str, name: &str) -> String {
    for line in unfold(headers).split('\n') {
        let Some(colon) = line.find(':') else {
            continue;
        };
        if colon == 0 {
            continue;
        }
        if line[..colon].trim().eq_ignore_ascii_case(name) {
            return line[colon + 1..].trim().to_owned();
        }
    }
    String::new()
}

fn param(value: &str, name: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let needle = format!("{name}=");
    let mut from = 0usize;
    while let Some(found) = lower[from..].find(&needle) {
        let start = from + found;
        let before_ok = start == 0
            || matches!(
                lower.as_bytes()[start - 1],
                b';' | b' ' | b'\t' | b'\r' | b'\n'
            );
        let mut rest = &value[start + needle.len()..];
        if before_ok {
            if let Some(stripped) = rest.strip_prefix('"')
                && let Some(end) = stripped.find('"')
            {
                return stripped[..end].trim().to_owned();
            }
            let end = rest.find(';').unwrap_or(rest.len());
            rest = &rest[..end];
            return rest.trim().to_owned();
        }
        from = start + needle.len();
    }
    String::new()
}

pub fn is_pgp_encrypted_mime(mime: &str) -> bool {
    let (headers, _) = split_part(mime);
    let content_type = header_value(headers, "content-type");
    if !content_type
        .to_ascii_lowercase()
        .starts_with("multipart/encrypted")
    {
        return false;
    }
    let protocol = param(&content_type, "protocol").to_ascii_lowercase();
    protocol.is_empty() || protocol == "application/pgp-encrypted"
}

pub fn extract_pgp_armor(mime: &str) -> Option<String> {
    let (headers, body) = split_part(mime);
    let content_type = header_value(headers, "content-type");
    if !content_type
        .to_ascii_lowercase()
        .starts_with("multipart/encrypted")
    {
        return None;
    }
    let boundary = param(&content_type, "boundary");
    if boundary.is_empty() {
        return None;
    }

    let marker = format!("--{boundary}");
    for segment in body.split(marker.as_str()) {
        if segment.starts_with("--") {
            break;
        }
        if segment.trim().is_empty() {
            continue;
        }
        let trimmed = segment
            .strip_prefix("\r\n")
            .or_else(|| segment.strip_prefix('\n'))
            .unwrap_or(segment);
        let (part_headers, part_body) = split_part(trimmed);
        let part_type = header_value(part_headers, "content-type").to_ascii_lowercase();
        if part_type.starts_with("application/pgp-encrypted") {
            continue;
        }

        let encoding = header_value(part_headers, "content-transfer-encoding")
            .to_ascii_lowercase()
            .trim()
            .to_owned();
        let decoded;
        let text = if encoding == "base64" {
            let compact: String = part_body.chars().filter(|c| !c.is_whitespace()).collect();
            match base64::engine::general_purpose::STANDARD.decode(compact.as_bytes()) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(s) => {
                        decoded = s;
                        decoded.as_str()
                    }
                    Err(_) => continue,
                },
                Err(_) => continue,
            }
        } else {
            part_body
        };

        let Some(begin) = text.find(BEGIN) else {
            continue;
        };
        let end = text[begin..].find(END)?;
        return Some(text[begin..begin + end + END.len()].to_owned());
    }
    None
}
