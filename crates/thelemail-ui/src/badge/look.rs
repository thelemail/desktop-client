use crate::button::Fill;
use crate::icons::IconName;
use crate::tokens::ColorToken;

use ColorToken as T;
use Fill::{Alpha, Token};

const NEUTRAL_FILL: f32 = 0.07;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeTone {
    #[default]
    Neutral,
    Success,
    Warning,
    Danger,
    Info,
    Brand,
    Caution,
}

impl BadgeTone {
    pub const ALL: [Self; 7] = [
        Self::Neutral,
        Self::Success,
        Self::Warning,
        Self::Danger,
        Self::Info,
        Self::Brand,
        Self::Caution,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeStatus {
    Neutral,
    Success,
    Warning,
    Danger,
    Info,
}

impl BadgeStatus {
    pub const ALL: [Self; 5] = [
        Self::Success,
        Self::Warning,
        Self::Danger,
        Self::Info,
        Self::Neutral,
    ];

    pub fn icon(self) -> IconName {
        match self {
            Self::Success => IconName::CircleCheck,
            Self::Warning => IconName::TriangleExclamation,
            Self::Danger => IconName::CircleExclamation,
            Self::Info => IconName::Clock,
            Self::Neutral => IconName::CircleMinus,
        }
    }

    pub fn tone(self) -> BadgeTone {
        match self {
            Self::Success => BadgeTone::Success,
            Self::Warning => BadgeTone::Warning,
            Self::Danger => BadgeTone::Danger,
            Self::Info => BadgeTone::Info,
            Self::Neutral => BadgeTone::Neutral,
        }
    }
}

pub fn look(tone: BadgeTone) -> (Fill, Fill) {
    match tone {
        BadgeTone::Neutral => (Alpha(T::Fg, NEUTRAL_FILL), Token(T::FgMuted)),
        BadgeTone::Success => (Token(T::Success100), Token(T::Success700)),
        BadgeTone::Warning => (Token(T::Warning100), Token(T::Warning700)),
        BadgeTone::Danger => (Token(T::Danger100), Token(T::Danger700)),
        BadgeTone::Info => (Token(T::Info100), Token(T::Info700)),
        BadgeTone::Brand => (Token(T::Pine100), Token(T::Pine700)),
        BadgeTone::Caution => (Token(T::Brass100), Token(T::Brass700)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tone_pairs_a_pale_fill_with_its_dark_ink() {
        for tone in BadgeTone::ALL {
            let (bg, fg) = look(tone);
            assert_ne!(bg, fg, "{tone:?}");
            assert!(matches!(fg, Token(_)), "{tone:?}");
        }
    }

    #[test]
    fn neutral_is_a_translucent_foreground() {
        assert_eq!(look(BadgeTone::Neutral).0, Alpha(T::Fg, 0.07));
    }

    #[test]
    fn a_status_never_borrows_another_status_icon() {
        let icons: Vec<_> = BadgeStatus::ALL.iter().map(|s| s.icon()).collect();
        for (i, icon) in icons.iter().enumerate() {
            assert!(!icons[i + 1..].contains(icon));
        }
        assert_eq!(BadgeStatus::Danger.icon(), IconName::CircleExclamation);
        assert_ne!(BadgeStatus::Danger.icon(), IconName::CircleCheck);
    }
}
