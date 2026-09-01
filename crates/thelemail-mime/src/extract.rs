use mail_parser::decoders::html::html_to_text;
use mail_parser::{MessageParser, MimeHeaders, PartType};
use serde::Serialize;

pub const DEFAULT_MAX_TEXT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentMeta {
    pub ordinal: usize,
    pub filename: String,
    pub content_type: String,
    pub disposition: String,
    pub content_id: Option<String>,
    pub plaintext_size: usize,
    pub is_inline: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Extracted {
    pub plain_text: String,
    pub attachments: Vec<AttachmentMeta>,
}

pub fn extract(raw: &[u8], max_text_bytes: usize) -> Extracted {
    let Some(message) = MessageParser::default().parse(raw) else {
        return Extracted::default();
    };

    let mut text = String::new();
    for part in message.text_bodies() {
        if text.len() >= max_text_bytes {
            break;
        }
        match &part.body {
            PartType::Text(body) => push_capped(&mut text, body.as_ref(), max_text_bytes),
            PartType::Html(body) => {
                push_capped(&mut text, &html_to_text(body.as_ref()), max_text_bytes)
            }
            _ => {}
        }
    }

    let mut attachments = Vec::new();
    for (ordinal, part) in message.attachments().enumerate() {
        let content_type = part
            .content_type()
            .map(|ct| match ct.subtype() {
                Some(sub) => format!("{}/{}", ct.ctype(), sub),
                None => ct.ctype().to_owned(),
            })
            .unwrap_or_else(|| "application/octet-stream".to_owned());
        let is_inline = part
            .content_disposition()
            .map(|d| d.is_inline())
            .unwrap_or(false);

        attachments.push(AttachmentMeta {
            ordinal,
            filename: part.attachment_name().unwrap_or_default().to_owned(),
            content_type,
            disposition: if is_inline { "inline" } else { "attachment" }.to_owned(),
            content_id: part.content_id().map(|c| c.to_owned()),
            plaintext_size: part.len(),
            is_inline,
        });
    }

    Extracted {
        plain_text: text,
        attachments,
    }
}

fn push_capped(out: &mut String, addition: &str, max_bytes: usize) {
    if out.len() >= max_bytes {
        return;
    }
    if !out.is_empty() {
        out.push('\n');
    }
    let remaining = max_bytes.saturating_sub(out.len());
    if addition.len() <= remaining {
        out.push_str(addition);
        return;
    }
    let mut end = remaining;
    while end > 0 && !addition.is_char_boundary(end) {
        end -= 1;
    }
    out.push_str(&addition[..end]);
}
