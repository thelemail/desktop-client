use thelemail_i18n::m;

use crate::tokens::ColorToken;

pub const PALETTE: [(ColorToken, ColorToken); 5] = [
    (ColorToken::Brass100, ColorToken::Brass700),
    (ColorToken::Pine100, ColorToken::Pine700),
    (ColorToken::Info100, ColorToken::Info700),
    (ColorToken::Success100, ColorToken::Success700),
    (ColorToken::BgRaised, ColorToken::FgMuted),
];

const FALLBACK: &str = "th";

fn js_space(c: char) -> bool {
    matches!(c, '\u{feff}' | '\u{2028}' | '\u{2029}') || (c.is_whitespace() && c != '\u{85}')
}

fn upper(text: impl Iterator<Item = char>) -> String {
    text.collect::<String>().to_uppercase()
}

pub fn initials(name: Option<&str>, email: Option<&str>) -> String {
    let name = name.unwrap_or_default().trim_matches(js_space);
    if !name.is_empty() {
        let parts: Vec<&str> = name.split(js_space).filter(|p| !p.is_empty()).collect();
        return match parts.as_slice() {
            [only] => upper(only.chars().take(2)),
            [first, .., last] => upper(first.chars().take(1).chain(last.chars().take(1))),
            [] => String::new(),
        };
    }
    let local = email
        .unwrap_or_default()
        .split('@')
        .next()
        .unwrap_or_default();
    if local.is_empty() {
        FALLBACK.to_uppercase()
    } else {
        upper(local.chars().take(2))
    }
}

pub fn palette_slot(seed: &str) -> usize {
    let hash = seed.encode_utf16().fold(0u32, |h, unit| {
        h.wrapping_mul(31).wrapping_add(u32::from(unit))
    });
    hash as usize % PALETTE.len()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sender {
    pub from: String,
    pub address: String,
    pub initials: String,
    pub slot: usize,
    pub mismatch: bool,
}

pub fn sender(display: &str, address: &str, forged: Option<&str>) -> Sender {
    let shown = forged.unwrap_or(address);
    let from = match forged {
        Some(attested) => attested.to_owned(),
        None if !display.is_empty() => display.to_owned(),
        None if !address.is_empty() => address.to_owned(),
        None => m::mailbox_unknown_sender().to_string(),
    };
    Sender {
        initials: initials(Some(&from), Some(shown)),
        slot: palette_slot(&shown.to_lowercase()),
        from,
        address: shown.to_owned(),
        mismatch: forged.is_some(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Initials,
    Photo,
    Logo,
}

pub fn choice(mismatch: bool, photo: bool, logo: bool) -> Choice {
    if mismatch {
        Choice::Initials
    } else if photo {
        Choice::Photo
    } else if logo {
        Choice::Logo
    } else {
        Choice::Initials
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn cases() -> Value {
        serde_json::from_str(include_str!("../../../../fixtures/avatar/cases.json"))
            .expect("fixture")
    }

    #[test]
    fn initials_match_the_web_client() {
        let cases = cases();
        for case in cases["initials"].as_array().expect("initials") {
            assert_eq!(
                initials(case["name"].as_str(), case["email"].as_str()),
                case["initials"].as_str().expect("initials"),
                "{case}"
            );
        }
    }

    #[test]
    fn palette_slots_match_the_web_client() {
        let cases = cases();
        for case in cases["palette"].as_array().expect("palette") {
            let seed = case["seed"].as_str().expect("seed");
            assert_eq!(
                palette_slot(seed) as u64,
                case["slot"].as_u64().expect("slot"),
                "{seed}"
            );
        }
    }

    #[test]
    fn sender_fields_match_the_web_client() {
        let cases = cases();
        for case in cases["senders"].as_array().expect("senders") {
            let got = sender(
                case["sender"]["display"].as_str().expect("display"),
                case["sender"]["address"].as_str().expect("address"),
                case["mismatch"].as_str(),
            );
            assert_eq!(got.from, case["from"].as_str().expect("from"), "{case}");
            assert_eq!(
                got.address,
                case["fromAddr"].as_str().expect("fromAddr"),
                "{case}"
            );
            assert_eq!(got.initials, case["init"].as_str().expect("init"), "{case}");
            assert_eq!(
                got.slot as u64,
                case["slot"].as_u64().expect("slot"),
                "{case}"
            );
            assert_eq!(
                got.mismatch,
                case["senderMismatch"].as_bool().expect("flag"),
                "{case}"
            );
        }
    }

    #[test]
    fn astral_characters_are_never_split() {
        assert_eq!(initials(Some("😀"), None), "😀");
        assert_eq!(initials(Some("𝒜da Ł"), None), "𝒜Ł");
    }

    #[test]
    fn a_forged_sender_shows_initials_only() {
        assert_eq!(choice(true, true, true), Choice::Initials);
        assert_eq!(choice(false, true, true), Choice::Photo);
        assert_eq!(choice(false, false, true), Choice::Logo);
        assert_eq!(choice(false, false, false), Choice::Initials);
    }
}
