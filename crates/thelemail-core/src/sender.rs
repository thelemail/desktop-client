use icu_normalizer::ComposingNormalizerBorrowed;

pub fn canonical(address: &str) -> String {
    ComposingNormalizerBorrowed::new_nfc()
        .normalize(address.trim())
        .to_lowercase()
}

pub fn attested_mismatch<'a>(attested: Option<&'a str>, preview_address: &str) -> Option<&'a str> {
    let attested = attested.filter(|a| !a.is_empty())?;
    (canonical(attested) != canonical(preview_address)).then_some(attested)
}

pub fn bimi_domain(raw: Option<&str>) -> Option<String> {
    let domain = raw?.trim().to_lowercase();
    (!domain.is_empty()).then_some(domain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Sender {
        address: String,
    }

    #[derive(Deserialize)]
    struct Case {
        sender: Sender,
        attested: Option<String>,
        mismatch: Option<String>,
    }

    #[derive(Deserialize)]
    struct Cases {
        senders: Vec<Case>,
    }

    #[test]
    fn mismatches_match_the_web_client() {
        let raw = include_str!("../../../fixtures/avatar/cases.json");
        let cases: Cases = serde_json::from_str(raw).expect("fixture");
        assert!(!cases.senders.is_empty());
        for case in &cases.senders {
            assert_eq!(
                attested_mismatch(case.attested.as_deref(), &case.sender.address),
                case.mismatch.as_deref(),
                "{:?} vs {:?}",
                case.attested,
                case.sender.address
            );
        }
    }

    #[test]
    fn composed_and_decomposed_addresses_are_the_same_sender() {
        assert_eq!(
            attested_mismatch(Some("élodie@acme.test"), "e\u{301}lodie@acme.test"),
            None
        );
    }

    #[test]
    fn no_attestation_is_never_a_mismatch() {
        assert_eq!(attested_mismatch(None, "a@b.test"), None);
        assert_eq!(attested_mismatch(Some(""), "a@b.test"), None);
    }

    #[test]
    fn the_bimi_domain_is_trimmed_and_lowercased() {
        assert_eq!(
            bimi_domain(Some(" Acme.TEST ")).as_deref(),
            Some("acme.test")
        );
        assert_eq!(bimi_domain(Some("  ")), None);
        assert_eq!(bimi_domain(None), None);
    }
}
