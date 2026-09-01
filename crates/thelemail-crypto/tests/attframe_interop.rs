use serde::Deserialize;
use thelemail_crypto::attframe::{FrameError, MAX_HEADER_BYTES, parse, parse_header};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/attframe");

#[derive(Deserialize)]
struct Case {
    name: String,
    header: Header,
    #[serde(rename = "payloadLen")]
    payload_len: usize,
}

#[derive(Deserialize)]
struct Header {
    filename: String,
    #[serde(rename = "contentType")]
    content_type: String,
    disposition: String,
    #[serde(rename = "contentId", default)]
    content_id: Option<String>,
}

fn cases() -> Vec<Case> {
    let raw = std::fs::read_to_string(format!("{FIXTURES}/meta.json")).expect("read meta");
    serde_json::from_str(&raw).expect("parse meta")
}

#[test]
fn parses_frames_built_by_the_web_client() {
    let cases = cases();
    assert!(!cases.is_empty());
    for case in cases {
        let bytes = std::fs::read(format!("{FIXTURES}/{}.bin", case.name))
            .unwrap_or_else(|_| panic!("read {}", case.name));
        let (header, payload) = parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", case.name));

        assert_eq!(header.filename, case.header.filename, "{}", case.name);
        assert_eq!(
            header.content_type, case.header.content_type,
            "{}",
            case.name
        );
        assert_eq!(header.disposition, case.header.disposition, "{}", case.name);
        assert_eq!(header.content_id, case.header.content_id, "{}", case.name);
        assert_eq!(header.plaintext_size, case.payload_len, "{}", case.name);
        assert_eq!(payload.len(), case.payload_len, "{}", case.name);
    }
}

#[test]
fn a_header_can_be_read_from_a_prefix_before_the_payload_arrives() {
    let bytes = std::fs::read(format!("{FIXTURES}/inline-cid.bin")).expect("read");
    let (_, header_end) = parse_header(&bytes).expect("full parse");
    let (header, same_end) = parse_header(&bytes[..header_end]).expect("prefix parse");
    assert_eq!(same_end, header_end);
    assert_eq!(header.filename, "logo.png");
}

#[test]
fn rejects_malformed_frames() {
    let bytes = std::fs::read(format!("{FIXTURES}/minimal.bin")).expect("read");

    assert!(matches!(parse(b"XXXX").unwrap_err(), FrameError::Truncated));

    let mut bad_magic = bytes.clone();
    bad_magic[0] = b'X';
    assert!(matches!(
        parse(&bad_magic).unwrap_err(),
        FrameError::BadMagic
    ));

    let mut bad_version = bytes.clone();
    bad_version[4] = 0x02;
    assert!(matches!(
        parse(&bad_version).unwrap_err(),
        FrameError::BadVersion
    ));

    let mut huge = bytes.clone();
    huge[5..9].copy_from_slice(&((MAX_HEADER_BYTES as u32) + 1).to_be_bytes());
    assert!(matches!(
        parse(&huge).unwrap_err(),
        FrameError::HeaderTooLarge
    ));

    assert!(matches!(
        parse(&bytes[..bytes.len() - 1]).unwrap_err(),
        FrameError::SizeMismatch
    ));
}

#[test]
fn a_lying_plaintext_size_is_rejected() {
    let bytes = std::fs::read(format!("{FIXTURES}/minimal.bin")).expect("read");
    let (_, header_end) = parse_header(&bytes).expect("header");
    let mut truncated = bytes[..header_end].to_vec();
    truncated.extend_from_slice(b"too short");
    assert!(matches!(
        parse(&truncated).unwrap_err(),
        FrameError::SizeMismatch
    ));
}
