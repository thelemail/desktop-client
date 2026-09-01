use thelemail_mime::{DEFAULT_MAX_TEXT_BYTES, extract};

#[test]
fn reads_a_plain_body() {
    let raw = b"Subject: Hi\r\nContent-Type: text/plain\r\n\r\nthe quick brown fox\r\n";
    let got = extract(raw, DEFAULT_MAX_TEXT_BYTES);
    assert!(got.plain_text.contains("quick brown fox"));
    assert!(got.attachments.is_empty());
}

#[test]
fn prefers_the_plain_alternative_and_still_sees_html_only_mail() {
    let alt = b"Content-Type: multipart/alternative; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nplain wins\r\n\
--b\r\nContent-Type: text/html\r\n\r\n<p>html loses</p>\r\n--b--\r\n";
    assert!(
        extract(alt, DEFAULT_MAX_TEXT_BYTES)
            .plain_text
            .contains("plain wins")
    );

    let html_only = b"Content-Type: text/html\r\n\r\n<p>only <b>html</b> here</p>\r\n";
    let got = extract(html_only, DEFAULT_MAX_TEXT_BYTES);
    assert!(
        got.plain_text.contains("html"),
        "html-only mail must still be indexable, got {:?}",
        got.plain_text
    );
    assert!(!got.plain_text.contains("<p>"), "tags must be stripped");
}

#[test]
fn decodes_a_legacy_charset() {
    let raw = b"Content-Type: text/plain; charset=iso-8859-1\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\r\nCaf=E9 cr=E8me\r\n";
    let got = extract(raw, DEFAULT_MAX_TEXT_BYTES);
    assert!(
        got.plain_text.contains("Café"),
        "legacy charset not decoded: {:?}",
        got.plain_text
    );
}

#[test]
fn lists_attachments_with_metadata() {
    let raw = b"Content-Type: multipart/mixed; boundary=\"m\"\r\n\r\n\
--m\r\nContent-Type: text/plain\r\n\r\nbody\r\n\
--m\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\n\
Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\nSGVsbG8=\r\n\
--m\r\nContent-Type: image/png\r\nContent-ID: <logo@x>\r\n\
Content-Disposition: inline; filename=\"logo.png\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\nSGk=\r\n--m--\r\n";
    let got = extract(raw, DEFAULT_MAX_TEXT_BYTES);

    let pdf = got
        .attachments
        .iter()
        .find(|a| a.filename == "report.pdf")
        .expect("pdf attachment");
    assert_eq!(pdf.content_type, "application/pdf");
    assert!(!pdf.is_inline);

    let logo = got
        .attachments
        .iter()
        .find(|a| a.filename == "logo.png")
        .expect("inline image");
    assert!(logo.is_inline, "an inline image must be marked inline");
    assert_eq!(logo.content_id.as_deref(), Some("logo@x"));
}

#[test]
fn text_is_capped_without_splitting_a_character() {
    let mut raw = b"Content-Type: text/plain; charset=utf-8\r\n\r\n".to_vec();
    raw.extend(std::iter::repeat_n("é".as_bytes(), 5000).flatten());
    let got = extract(&raw, 1024);
    assert!(got.plain_text.len() <= 1024, "cap not honoured");
    assert!(std::str::from_utf8(got.plain_text.as_bytes()).is_ok());
}

#[test]
fn hostile_and_empty_input_never_panics() {
    for raw in [
        &b""[..],
        &b"\x00\x01\x02\xff\xfe"[..],
        &b"Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\n"[..],
        &b"Content-Type: message/rfc822\r\n\r\nContent-Type: message/rfc822\r\n\r\nnested"[..],
    ] {
        let _ = extract(raw, DEFAULT_MAX_TEXT_BYTES);
    }

    let mut deep = b"Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n".to_vec();
    for _ in 0..2000 {
        deep.extend_from_slice(b"--b\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n");
    }
    let _ = extract(&deep, DEFAULT_MAX_TEXT_BYTES);
}
