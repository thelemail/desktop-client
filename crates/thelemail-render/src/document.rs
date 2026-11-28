use thelemail_mime::{BodyOptions, InlineParts, parse_body};

use crate::designed::looks_designed;
use crate::js;
use crate::plain::plain_to_html;
use crate::sanitize::{SanitizeOptions, sanitize_html};

const PLAIN_DARK: &str = r#":root[data-theme="dark"]:not(.designed)"#;

pub const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src data: cid:; font-src data:; connect-src 'none'; script-src 'none'; object-src 'none'; form-action 'none'";

const EMPTY_BODY: &str = r#"<p class="empty">(no body)</p>"#;

pub fn base_style() -> String {
    [
        "html,body{margin:0;padding:0;background:transparent}".to_owned(),
        r#"body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif;font-size:15px;line-height:1.65;color:#21201F;word-wrap:break-word}"#.to_owned(),
        ".plain{padding:2px}".to_owned(),
        ".plain p{margin:0 0 16px}".to_owned(),
        ".empty{color:#7A7976;font-style:italic}".to_owned(),
        "img{max-width:100%;height:auto}".to_owned(),
        "a{color:#0E6240}".to_owned(),
        "blockquote{margin:0 0 16px;padding-left:14px;border-left:2px solid #D4D3D1;color:#62605D}".to_owned(),
        "pre{white-space:pre-wrap;word-wrap:break-word;font-family:ui-monospace,Menlo,Consolas,monospace;font-size:13px}".to_owned(),
        format!("{PLAIN_DARK}{{color-scheme:dark}}"),
        format!(
            "{PLAIN_DARK} body,{PLAIN_DARK} body *{{color:#E0E3E7!important;background-color:transparent!important;border-color:#393C3F!important}}"
        ),
        format!("{PLAIN_DARK} blockquote,{PLAIN_DARK} blockquote *{{color:#B8BDC2!important}}"),
        format!("{PLAIN_DARK} a,{PLAIN_DARK} a *{{color:#93CDAD!important}}"),
        r#":root[data-theme="dark"].designed{color-scheme:light;background:#fff}"#.to_owned(),
        r#":root[data-theme="dark"].designed body{padding:16px 18px}"#.to_owned(),
    ]
    .concat()
}

fn escape_for_attribute(s: &str) -> String {
    crate::plain::escape_html(s)
}

pub fn build_src_doc(body_html: &str, wrap_plain: bool) -> String {
    let designed = !wrap_plain && looks_designed(body_html);
    let content = if wrap_plain {
        format!(r#"<div class="plain">{body_html}</div>"#)
    } else {
        body_html.to_owned()
    };
    format!(
        r#"<!doctype html><html{}><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="{}"><base target="_blank"><style>{}</style></head><body>{content}</body></html>"#,
        if designed { r#" class="designed""# } else { "" },
        escape_for_attribute(CSP),
        base_style()
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

pub fn with_theme(src_doc: &str, theme: Theme) -> String {
    let value = match theme {
        Theme::Light => "light",
        Theme::Dark => "dark",
    };
    match src_doc.find("<html") {
        Some(at) => {
            let insert = at + "<html".len();
            format!(
                r#"{} data-theme="{value}"{}"#,
                &src_doc[..insert],
                &src_doc[insert..]
            )
        }
        None => src_doc.to_owned(),
    }
}

pub fn fit_zoom(natural_width: f32, available_width: f32) -> Option<f32> {
    if available_width <= 0.0 || natural_width <= available_width + 1.0 {
        return None;
    }
    Some(available_width / natural_width)
}

#[derive(Debug, Clone, Copy)]
pub enum BodySource<'a> {
    Mime(&'a [u8]),
    Parts {
        html: Option<&'a str>,
        text: Option<&'a str>,
    },
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RenderOptions {
    pub strip_tracking: bool,
    pub utc_offset_seconds: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderedBody {
    pub src_doc: String,
    pub designed: bool,
    pub content_html: Option<String>,
    pub content_text: Option<String>,
    pub inline: InlineParts,
}

fn is_blank(s: Option<&str>) -> bool {
    s.is_none_or(|s| js::trim(s).is_empty())
}

pub fn render_body(source: BodySource<'_>, options: RenderOptions) -> RenderedBody {
    let mut inline = InlineParts::new();
    let (html, text) = match source {
        BodySource::Mime(raw) => {
            let lossy = String::from_utf8_lossy(raw);
            if is_blank(Some(&lossy)) {
                (None, None)
            } else {
                match parse_body(
                    raw,
                    BodyOptions {
                        utc_offset_seconds: options.utc_offset_seconds,
                    },
                ) {
                    Ok(parsed) => {
                        inline = parsed.inline;
                        (parsed.html, parsed.text)
                    }
                    Err(_) => (None, Some(lossy.into_owned())),
                }
            }
        }
        BodySource::Parts { html, text } => (html.map(str::to_owned), text.map(str::to_owned)),
    };
    let has_html = !is_blank(html.as_deref());
    let has_text = !is_blank(text.as_deref());
    let (body, wrap_plain, content_html) = if has_html {
        let sanitized = sanitize_html(
            html.as_deref().unwrap_or_default(),
            &SanitizeOptions {
                inline: &inline,
                strip_tracking: options.strip_tracking,
            },
        );
        (
            sanitized.clone(),
            false,
            Some(sanitized).filter(|s| !s.is_empty()),
        )
    } else if has_text {
        (
            plain_to_html(text.as_deref().unwrap_or_default()),
            true,
            None,
        )
    } else {
        (EMPTY_BODY.to_owned(), true, None)
    };
    RenderedBody {
        designed: !wrap_plain && looks_designed(&body),
        src_doc: build_src_doc(&body, wrap_plain),
        content_html,
        content_text: if has_text { text } else { None },
        inline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_goes_on_the_root_element() {
        let doc = build_src_doc("<p>x</p>", true);
        assert!(
            with_theme(&doc, Theme::Dark)
                .starts_with(r#"<!doctype html><html data-theme="dark"><head>"#)
        );
    }

    #[test]
    fn designed_mail_is_flagged_and_fits() {
        let rendered = render_body(
            BodySource::Parts {
                html: Some(r##"<table bgcolor="#eee"><tr><td>x</td></tr></table>"##),
                text: None,
            },
            RenderOptions::default(),
        );
        assert!(rendered.designed);
        assert!(rendered.src_doc.contains(r#"<html class="designed">"#));
        assert_eq!(fit_zoom(800.0, 600.0), Some(0.75));
        assert_eq!(fit_zoom(600.5, 600.0), None);
        assert_eq!(fit_zoom(800.0, 0.0), None);
    }

    #[test]
    fn mime_html_wins_and_inline_parts_resolve() {
        let raw = b"Content-Type: multipart/related; boundary=\"r\"\r\n\r\n--r\r\nContent-Type: text/html\r\n\r\n<img src=\"cid:a\"><script>x</script>\r\n--r\r\nContent-Type: image/gif\r\nContent-ID: <a>\r\nContent-Transfer-Encoding: base64\r\n\r\nR0lGODlh\r\n--r--\r\n";
        let rendered = render_body(
            BodySource::Mime(raw),
            RenderOptions {
                strip_tracking: true,
                utc_offset_seconds: 0,
            },
        );
        let html = rendered.content_html.unwrap();
        assert!(html.contains("src=\"data:image/gif;base64,R0lGODlh\""));
        assert!(!html.contains("script"));
        assert!(rendered.inline.contains_key("a"));
    }

    #[test]
    fn unparseable_mime_renders_as_text() {
        let mut raw = String::new();
        for depth in 0..300 {
            raw.push_str(&format!(
                "Content-Type: multipart/mixed; boundary=\"b{depth}\"\r\n\r\n--b{depth}\r\n"
            ));
        }
        let rendered = render_body(BodySource::Mime(raw.as_bytes()), RenderOptions::default());
        assert!(rendered.content_html.is_none());
        assert!(rendered.content_text.is_some());
        assert!(
            rendered
                .src_doc
                .contains(r#"<div class="plain"><p>Content-Type"#)
        );
    }

    #[test]
    fn empty_input_shows_the_empty_marker() {
        let rendered = render_body(
            BodySource::Parts {
                html: Some("  "),
                text: None,
            },
            RenderOptions::default(),
        );
        assert!(rendered.src_doc.contains(EMPTY_BODY));
        assert!(!rendered.designed);
    }
}
