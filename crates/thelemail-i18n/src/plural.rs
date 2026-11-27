use crate::Locale;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

impl Category {
    pub fn name(self) -> &'static str {
        match self {
            Category::Zero => "zero",
            Category::One => "one",
            Category::Two => "two",
            Category::Few => "few",
            Category::Many => "many",
            Category::Other => "other",
        }
    }
}

pub fn cardinal(locale: Locale, n: u64) -> Category {
    match locale {
        Locale::En | Locale::De if n == 1 => Category::One,
        Locale::En | Locale::De => Category::Other,
        Locale::Fr | Locale::Pt if n <= 1 => Category::One,
        Locale::Fr | Locale::Pt if n.is_multiple_of(1_000_000) => Category::Many,
        Locale::Fr | Locale::Pt => Category::Other,
    }
}

pub fn ordinal(locale: Locale, n: u64) -> Category {
    match locale {
        Locale::En => match (n % 10, n % 100) {
            (1, r) if r != 11 => Category::One,
            (2, r) if r != 12 => Category::Two,
            (3, r) if r != 13 => Category::Few,
            _ => Category::Other,
        },
        Locale::Fr if n == 1 => Category::One,
        Locale::De | Locale::Fr | Locale::Pt => Category::Other,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn english_cardinals_and_ordinals() {
        assert_eq!(cardinal(Locale::En, 0), Category::Other);
        assert_eq!(cardinal(Locale::En, 1), Category::One);
        assert_eq!(cardinal(Locale::En, 2), Category::Other);
        let ordinals: Vec<_> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 23, 101, 111, 112]
            .into_iter()
            .map(|n| ordinal(Locale::En, n).name())
            .collect();
        assert_eq!(
            ordinals,
            [
                "one", "two", "few", "other", "other", "other", "other", "one", "two", "few",
                "one", "other", "other"
            ]
        );
    }

    #[test]
    fn french_and_portuguese_count_zero_as_one() {
        for locale in [Locale::Fr, Locale::Pt] {
            assert_eq!(cardinal(locale, 0), Category::One);
            assert_eq!(cardinal(locale, 1), Category::One);
            assert_eq!(cardinal(locale, 2), Category::Other);
            assert_eq!(cardinal(locale, 1_000_000), Category::Many);
            assert_eq!(cardinal(locale, 1_000_001), Category::Other);
        }
        assert_eq!(cardinal(Locale::De, 0), Category::Other);
        assert_eq!(ordinal(Locale::Fr, 1), Category::One);
        assert_eq!(ordinal(Locale::Pt, 1), Category::Other);
        assert_eq!(ordinal(Locale::De, 2), Category::Other);
    }

    type Expected = BTreeMap<String, BTreeMap<String, BTreeMap<String, Vec<u64>>>>;

    #[test]
    fn matches_intl_plural_rules_from_the_web() {
        let raw = include_str!("../../../fixtures/i18n/plural.json");
        let fixture: serde_json::Value = serde_json::from_str(raw).unwrap();
        let numbers: Vec<u64> = serde_json::from_value(fixture["numbers"].clone()).unwrap();
        let expected: Expected = serde_json::from_value(fixture["categories"].clone()).unwrap();
        let mut checked = 0;
        for locale in Locale::ALL {
            let by_type = &expected[locale.code()];
            for (kind, select) in [
                ("cardinal", cardinal as fn(Locale, u64) -> Category),
                ("ordinal", ordinal),
            ] {
                let by_category = &by_type[kind];
                for &n in &numbers {
                    let want = by_category
                        .iter()
                        .find(|(_, ns)| ns.contains(&n))
                        .map(|(c, _)| c.as_str())
                        .unwrap_or("other");
                    assert_eq!(select(locale, n).name(), want, "{locale:?} {kind} {n}");
                    checked += 1;
                }
            }
        }
        assert!(checked > 8000);
    }
}
