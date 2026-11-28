use std::collections::HashSet;

use url::Url;

use crate::js;

pub const TRACKING_PARAMS: [&str; 45] = [
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "utm_id",
    "utm_source_platform",
    "utm_creative_format",
    "utm_marketing_tactic",
    "fbclid",
    "gclid",
    "gbraid",
    "wbraid",
    "dclid",
    "msclkid",
    "twclid",
    "ttclid",
    "igshid",
    "wickedid",
    "yclid",
    "mc_cid",
    "mc_eid",
    "mkt_tok",
    "_hsenc",
    "_hsmi",
    "hsctatracking",
    "_branch_match_id",
    "vero_id",
    "vero_conv",
    "ref_src",
    "ref_url",
    "oly_anon_id",
    "oly_enc_id",
    "sc_campaign",
    "sc_channel",
    "sc_content",
    "sc_country",
    "sc_geo",
    "sc_medium",
    "sc_outcome",
    "sc_publisher",
    "piwik_campaign",
    "piwik_kwd",
    "pk_campaign",
    "pk_kwd",
];

fn is_tracking(key: &str) -> bool {
    let lower = key.to_lowercase();
    TRACKING_PARAMS.contains(&lower.as_str())
}

pub fn strip_tracking_params(href: &str) -> String {
    if !(js::starts_with_ignore_case(href, "http://")
        || js::starts_with_ignore_case(href, "https://"))
    {
        return href.to_owned();
    }
    let Ok(mut url) = Url::parse(href) else {
        return href.to_owned();
    };
    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let doomed: HashSet<&str> = pairs
        .iter()
        .map(|(key, _)| key.as_str())
        .filter(|key| is_tracking(key))
        .collect();
    if doomed.is_empty() {
        return href.to_owned();
    }
    let kept: Vec<&(String, String)> = pairs
        .iter()
        .filter(|(key, _)| !doomed.contains(key.as_str()))
        .collect();
    if kept.is_empty() {
        url.set_query(None);
    } else {
        let query = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(
                kept.iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            )
            .finish();
        url.set_query(Some(&query));
    }
    url.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_every_occurrence_and_keeps_the_rest() {
        assert_eq!(
            strip_tracking_params("https://shop.test/?a=1&utm_source=x&UTM_SOURCE=y&b=2"),
            "https://shop.test/?a=1&b=2"
        );
    }

    #[test]
    fn unchanged_links_are_returned_byte_for_byte() {
        let href = "https://Shop.test:443/a%20b?q=a%20b";
        assert_eq!(strip_tracking_params(href), href);
        assert_eq!(
            strip_tracking_params("mailto:a@b.test?utm_source=x"),
            "mailto:a@b.test?utm_source=x"
        );
    }

    #[test]
    fn empty_query_drops_the_question_mark() {
        assert_eq!(
            strip_tracking_params("https://shop.test/?fbclid=1#f"),
            "https://shop.test/#f"
        );
    }
}
