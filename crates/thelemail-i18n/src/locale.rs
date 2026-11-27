use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    En,
    De,
    Fr,
    Pt,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

impl Locale {
    pub const ALL: [Locale; 4] = [Locale::En, Locale::De, Locale::Fr, Locale::Pt];

    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::De => "de",
            Locale::Fr => "fr",
            Locale::Pt => "pt",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::De => "Deutsch",
            Locale::Fr => "Français",
            Locale::Pt => "Português",
        }
    }

    pub fn language_tag(self) -> &'static str {
        match self {
            Locale::Pt => "pt-PT",
            other => other.code(),
        }
    }

    pub fn parse(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.code() == code)
    }

    pub fn match_tags<S: AsRef<str>>(tags: &[S]) -> Option<Self> {
        tags.iter().find_map(|tag| {
            let tag = tag.as_ref().trim().to_lowercase().replace('_', "-");
            let mut parts = tag.split('-');
            match (parts.next(), parts.next()) {
                (Some("en"), _) => Some(Locale::En),
                (Some("de"), _) => Some(Locale::De),
                (Some("fr"), _) => Some(Locale::Fr),
                (Some("pt"), None | Some("pt")) => Some(Locale::Pt),
                _ => None,
            }
        })
    }

    fn from_index(index: u8) -> Self {
        Self::ALL
            .get(usize::from(index))
            .copied()
            .unwrap_or(Locale::En)
    }

    fn index(self) -> u8 {
        match self {
            Locale::En => 0,
            Locale::De => 1,
            Locale::Fr => 2,
            Locale::Pt => 3,
        }
    }
}

pub fn locale() -> Locale {
    Locale::from_index(CURRENT.load(Ordering::Relaxed))
}

pub fn set_locale(locale: Locale) {
    CURRENT.store(locale.index(), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_regional_german_and_french_to_the_base_language() {
        assert_eq!(Locale::match_tags(&["de-CH"]), Some(Locale::De));
        assert_eq!(Locale::match_tags(&["de-AT", "en"]), Some(Locale::De));
        assert_eq!(Locale::match_tags(&["fr-BE"]), Some(Locale::Fr));
        assert_eq!(Locale::match_tags(&["fr_CA"]), Some(Locale::Fr));
    }

    #[test]
    fn only_takes_european_portuguese() {
        assert_eq!(Locale::match_tags(&["pt-PT"]), Some(Locale::Pt));
        assert_eq!(Locale::match_tags(&["pt_PT"]), Some(Locale::Pt));
        assert_eq!(Locale::match_tags(&["pt"]), Some(Locale::Pt));
        assert_eq!(Locale::match_tags(&["pt-BR"]), None);
        assert_eq!(Locale::match_tags(&["pt-BR", "fr-FR"]), Some(Locale::Fr));
        assert_eq!(Locale::match_tags(&["pt-AO", "en-US"]), Some(Locale::En));
    }

    #[test]
    fn honours_preference_order() {
        assert_eq!(Locale::match_tags(&["en-GB", "de-DE"]), Some(Locale::En));
        assert_eq!(Locale::match_tags(&["es-ES", "it", "fr"]), Some(Locale::Fr));
        assert_eq!(Locale::match_tags(&[" DE-de "]), Some(Locale::De));
    }

    #[test]
    fn returns_nothing_for_unsupported_languages() {
        assert_eq!(Locale::match_tags(&["es", "ru-RU"]), None);
        assert_eq!(Locale::match_tags::<&str>(&[]), None);
    }

    #[test]
    fn codes_round_trip_and_portuguese_is_european() {
        for locale in Locale::ALL {
            assert_eq!(Locale::parse(locale.code()), Some(locale));
            assert_eq!(Locale::from_index(locale.index()), locale);
        }
        assert_eq!(Locale::parse("pt-BR"), None);
        assert_eq!(Locale::Pt.language_tag(), "pt-PT");
        assert_eq!(Locale::De.language_tag(), "de");
    }
}
