use std::borrow::Cow;

use base64::Engine;
use base64::alphabet::STANDARD;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use thelemail_mime::InlineParts;
use url::Url;

use crate::js;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource<'a> {
    pub mime_type: Cow<'a, str>,
    pub data: Cow<'a, [u8]>,
}

const FORGIVING_BASE64: GeneralPurpose = GeneralPurpose::new(
    &STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

fn percent_decode(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit()
            && let Ok(byte) = u8::from_str_radix(&input[index + 1..index + 3], 16)
        {
            out.push(byte);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    out
}

fn data_url(url: &str) -> Option<Resource<'static>> {
    let rest = &url[5..];
    let comma = rest.find(',')?;
    let meta = rest[..comma].trim_matches(|c: char| c.is_ascii_whitespace());
    let payload = &rest[comma + 1..];
    let (mime, base64) = match meta
        .len()
        .checked_sub(7)
        .filter(|&at| meta.is_char_boundary(at))
    {
        Some(at) if meta[at..].eq_ignore_ascii_case(";base64") => (meta[..at].trim_end(), true),
        _ => (meta, false),
    };
    let decoded = percent_decode(payload);
    let data = if base64 {
        let compact: Vec<u8> = decoded
            .into_iter()
            .filter(|b| !b.is_ascii_whitespace())
            .collect();
        FORGIVING_BASE64.decode(compact).ok()?
    } else {
        decoded
    };
    let mime_type = if mime.is_empty() {
        "text/plain;charset=US-ASCII"
    } else {
        mime
    };
    Some(Resource {
        mime_type: Cow::Owned(mime_type.to_owned()),
        data: Cow::Owned(data),
    })
}

pub fn resolve_resource<'a>(url: &str, inline: &'a InlineParts) -> Option<Resource<'a>> {
    let url = js::trim(url);
    if js::starts_with_ignore_case(url, "cid:") {
        let cid = &url[4..];
        let cid = cid.strip_prefix('<').unwrap_or(cid);
        let cid = cid.strip_suffix('>').unwrap_or(cid);
        let part = inline.get(cid)?;
        return Some(Resource {
            mime_type: Cow::Borrowed(&part.mime_type),
            data: Cow::Borrowed(&part.data),
        });
    }
    if js::starts_with_ignore_case(url, "data:") {
        return data_url(url);
    }
    None
}

pub fn opens_externally(href: &str) -> bool {
    Url::parse(js::trim(href)).is_ok_and(|url| matches!(url.scheme(), "http" | "https" | "mailto"))
}

#[cfg(test)]
mod tests {
    use thelemail_mime::InlinePart;

    use super::*;

    fn inline() -> InlineParts {
        InlineParts::from([(
            "hero".to_owned(),
            InlinePart {
                mime_type: "image/png".to_owned(),
                data: vec![1, 2],
            },
        )])
    }

    #[test]
    fn only_in_memory_resources_resolve() {
        let parts = inline();
        assert_eq!(
            resolve_resource("cid:<hero>", &parts)
                .unwrap()
                .data
                .as_ref(),
            &[1, 2]
        );
        assert!(resolve_resource("cid:other", &parts).is_none());
        for url in [
            "https://x.test/a.png",
            "http://x.test",
            "file:///etc/passwd",
            "//x.test/a",
            "a.png",
            "about:blank",
            "blob:x",
        ] {
            assert!(resolve_resource(url, &parts).is_none(), "{url}");
        }
    }

    #[test]
    fn data_urls_decode_in_memory() {
        let parts = InlineParts::new();
        let png = resolve_resource("data:image/png;base64,AQI", &parts).unwrap();
        assert_eq!(
            (png.mime_type.as_ref(), png.data.as_ref()),
            ("image/png", &[1u8, 2][..])
        );
        let text = resolve_resource("DATA:,a%20b", &parts).unwrap();
        assert_eq!(
            (text.mime_type.as_ref(), text.data.as_ref()),
            ("text/plain;charset=US-ASCII", &b"a b"[..])
        );
        assert!(resolve_resource("data:image/png;base64,!!!", &parts).is_none());
    }

    #[test]
    fn links_open_externally_only_for_web_and_mail() {
        assert!(opens_externally("https://calloway.test/a"));
        assert!(opens_externally(" mailto:ada@calloway.test"));
        for href in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "tel:+1",
            "#top",
            "/path",
            "thelemail-body://message/doc",
            "data:text/html,x",
        ] {
            assert!(!opens_externally(href), "{href}");
        }
    }
}
