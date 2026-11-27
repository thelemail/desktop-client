pub const PHOTO_MAX: usize = 512 * 1024;
pub const SVG_MAX: usize = 98_304;
pub const PERSON_REFRESH: i64 = 50 * 60;
pub const BIMI_REFRESH: i64 = 24 * 60 * 60;
pub const RETRY_AFTER: i64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
    Svg,
}

impl ImageFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Gif => "gif",
            Self::Webp => "webp",
            Self::Svg => "svg",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::Png, Self::Jpeg, Self::Gif, Self::Webp, Self::Svg]
            .into_iter()
            .find(|f| f.as_str() == value)
    }
}

pub fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(ImageFormat::Jpeg);
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(ImageFormat::Png);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(ImageFormat::Gif);
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(ImageFormat::Webp);
    }
    None
}

pub fn photo(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.is_empty() || bytes.len() > PHOTO_MAX {
        return None;
    }
    sniff(bytes)
}

const REFUSED: [&str; 7] = [
    "<image",
    "<foreignobject",
    "<script",
    "<!doctype",
    "<!entity",
    "@import",
    "javascript:",
];

fn local_refs_only(lower: &str, marker: &str, allowed: impl Fn(&str) -> bool) -> bool {
    let mut rest = lower;
    while let Some(at) = rest.find(marker) {
        rest = &rest[at + marker.len()..];
        if !allowed(rest.trim_start()) {
            return false;
        }
    }
    true
}

fn href_value_is_local(after: &str) -> bool {
    let Some(value) = after.strip_prefix('=') else {
        return true;
    };
    let value = value.trim_start();
    let value = value
        .strip_prefix('"')
        .or_else(|| value.strip_prefix('\''))
        .unwrap_or(value);
    value.starts_with('#')
}

fn url_is_local(after: &str) -> bool {
    let value = after
        .strip_prefix('"')
        .or_else(|| after.strip_prefix('\''))
        .unwrap_or(after)
        .trim_start();
    value.starts_with('#')
}

pub fn logo(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > SVG_MAX {
        return false;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let lower = text.to_lowercase();
    let head = lower.trim_start_matches('\u{feff}').trim_start();
    if !(head.starts_with("<svg") || head.starts_with("<?xml")) {
        return false;
    }
    if REFUSED.iter().any(|needle| lower.contains(needle)) {
        return false;
    }
    local_refs_only(&lower, "href", href_value_is_local)
        && local_refs_only(&lower, "url(", url_is_local)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    Fresh,
    Refresh,
}

pub fn freshness(checked_at: i64, now: i64, every: i64) -> Next {
    if now.saturating_sub(checked_at) < every && checked_at <= now {
        Next::Fresh
    } else {
        Next::Refresh
    }
}

pub fn needs_download(cached_source: Option<&str>, has_image: bool, path: &str) -> bool {
    !(has_image && cached_source == Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG_OK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 10 10"><defs><linearGradient id="g"/></defs><rect fill="url(#g)" width="10" height="10"/><use xlink:href="#g"/></svg>"##;

    #[test]
    fn magic_bytes_pick_the_format() {
        assert_eq!(sniff(b"\xFF\xD8\xFF\xE0rest"), Some(ImageFormat::Jpeg));
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), Some(ImageFormat::Png));
        assert_eq!(sniff(b"GIF89a.."), Some(ImageFormat::Gif));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(ImageFormat::Webp));
        assert_eq!(sniff(b"<svg/>"), None);
        assert_eq!(sniff(b"<html>"), None);
    }

    #[test]
    fn a_photo_must_be_a_small_raster() {
        assert_eq!(photo(b""), None);
        assert_eq!(photo(b"\x89PNG\r\n\x1a\n"), Some(ImageFormat::Png));
        let mut big = b"\x89PNG\r\n\x1a\n".to_vec();
        big.resize(PHOTO_MAX + 1, 0);
        assert_eq!(photo(&big), None);
        assert_eq!(photo(b"<svg xmlns='x'/>"), None);
    }

    #[test]
    fn a_plain_logo_with_local_references_is_accepted() {
        assert!(logo(SVG_OK.as_bytes()));
        assert!(logo(
            b"<?xml version=\"1.0\"?><svg><path d=\"M0 0\"/></svg>"
        ));
        assert!(logo("\u{feff}  <svg></svg>".as_bytes()));
    }

    #[test]
    fn anything_that_can_reach_outside_the_logo_is_refused() {
        for svg in [
            r#"<svg><image href="/etc/passwd"/></svg>"#,
            r#"<svg><IMAGE xlink:href="file:///Users/x/a.png"/></svg>"#,
            r#"<svg><use href="other.svg#a"/></svg>"#,
            r#"<svg><use xlink:href = 'https://evil.test/a.svg#x'/></svg>"#,
            r#"<svg><filter><feImage href="/tmp/a.png"/></filter></svg>"#,
            r#"<svg><rect fill="url(/tmp/x.svg#g)"/></svg>"#,
            r#"<svg><rect style="fill:url( 'file:///x' )"/></svg>"#,
            r#"<svg><style>@import url(https://evil.test/a.css);</style></svg>"#,
            r#"<svg><foreignObject><div/></foreignObject></svg>"#,
            r#"<svg><script>alert(1)</script></svg>"#,
            r#"<!DOCTYPE svg [<!ENTITY x SYSTEM "file:///etc/passwd">]><svg>&x;</svg>"#,
            r#"<svg><a href="javascript:alert(1)"/></svg>"#,
            r#"<svg><use href="&#35;a"/></svg>"#,
            r#"<html><svg/></html>"#,
        ] {
            assert!(!logo(svg.as_bytes()), "{svg}");
        }
    }

    #[test]
    fn oversized_or_binary_logos_are_refused() {
        assert!(!logo(b""));
        assert!(!logo(&[0xFF, 0xFE, 0x00]));
        let mut big = b"<svg>".to_vec();
        big.resize(SVG_MAX + 1, b' ');
        assert!(!logo(&big));
    }

    #[test]
    fn entries_refresh_on_their_own_schedule() {
        assert_eq!(
            freshness(1_000, 1_000 + PERSON_REFRESH - 1, PERSON_REFRESH),
            Next::Fresh
        );
        assert_eq!(
            freshness(1_000, 1_000 + PERSON_REFRESH, PERSON_REFRESH),
            Next::Refresh
        );
        assert_eq!(freshness(5_000, 1_000, PERSON_REFRESH), Next::Refresh);
    }

    #[test]
    fn an_image_is_downloaded_again_only_when_its_path_changes() {
        assert!(!needs_download(
            Some("/v1/avatars/a/1"),
            true,
            "/v1/avatars/a/1"
        ));
        assert!(needs_download(
            Some("/v1/avatars/a/1"),
            true,
            "/v1/avatars/a/2"
        ));
        assert!(needs_download(
            Some("/v1/avatars/a/1"),
            false,
            "/v1/avatars/a/1"
        ));
        assert!(needs_download(None, false, "/v1/avatars/a/1"));
    }
}
