use crate::button::{Fill, Phase};
use crate::tokens::ColorToken;

use ColorToken as T;
use Fill::{Alpha, Token};

pub const DISABLED_OPACITY: f32 = 0.5;
pub const TILE_DISABLED_OPACITY: f32 = 0.55;
const CHIP_ON_BORDER: f32 = 0.35;
const TILE_ON_BORDER: f32 = 0.45;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paint {
    pub bg: Fill,
    pub border: Fill,
    pub fg: Fill,
}

const fn paint(bg: Fill, border: Fill, fg: Fill) -> Paint {
    Paint { bg, border, fg }
}

pub fn chip(on: bool, interactive: bool, phase: Phase) -> Paint {
    if on {
        return paint(
            Token(T::Pine100),
            Alpha(T::Pine700, CHIP_ON_BORDER),
            Token(T::Pine700),
        );
    }
    if interactive && phase != Phase::Rest {
        return paint(
            Token(T::NeutralHover),
            Token(T::BorderSoft),
            Token(T::FgStrong),
        );
    }
    paint(Token(T::NeutralFill), Token(T::BorderSoft), Token(T::Fg))
}

pub fn segment(checked: bool, interactive: bool, phase: Phase) -> Fill {
    if checked || (interactive && phase != Phase::Rest) {
        Token(T::FgStrong)
    } else {
        Token(T::FgMuted)
    }
}

pub const TRACK: ColorToken = T::BgSunken;
pub const THUMB: ColorToken = T::Surface;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Off,
    On,
    Mixed,
}

pub fn checkbox(mark: Mark, invalid: bool) -> Paint {
    let on = mark != Mark::Off;
    let bg = if on { T::Primary } else { T::Surface };
    let border = match (invalid, on) {
        (true, _) => T::Danger500,
        (false, true) => T::Primary,
        (false, false) => T::BorderControl,
    };
    paint(Token(bg), Token(border), Token(T::PrimaryFg))
}

pub fn radio(checked: bool, invalid: bool) -> Paint {
    let border = match (invalid, checked) {
        (true, _) => T::Danger500,
        (false, true) => T::Primary,
        (false, false) => T::BorderControl,
    };
    paint(Token(T::Surface), Token(border), Token(T::Primary))
}

pub fn switch(on: bool) -> Paint {
    if on {
        paint(Token(T::Primary), Token(T::Primary), Token(T::PrimaryFg))
    } else {
        paint(
            Token(T::BgSunken),
            Token(T::BorderControl),
            Token(T::BorderControl),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TilePaint {
    pub bg: Fill,
    pub border: Fill,
    pub title: Fill,
    pub icon: Fill,
    pub mark: Fill,
}

pub fn tile(checked: bool, interactive: bool, phase: Phase) -> TilePaint {
    if checked {
        return TilePaint {
            bg: Token(T::Pine100),
            border: Alpha(T::Pine700, TILE_ON_BORDER),
            title: Token(T::Pine700),
            icon: Token(T::Pine700),
            mark: Token(T::Primary),
        };
    }
    let lit = interactive && phase != Phase::Rest;
    TilePaint {
        bg: Token(if lit { T::BgRaised } else { T::Surface }),
        border: Token(if lit { T::BorderStrong } else { T::BorderSoft }),
        title: Token(T::FgStrong),
        icon: Token(T::FgMuted),
        mark: Token(T::BorderControl),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_pine(fill: Fill) -> bool {
        matches!(
            fill,
            Token(T::Pine100 | T::Pine700) | Alpha(T::Pine100 | T::Pine700, _)
        )
    }

    #[test]
    fn a_chip_that_is_on_is_pale_green_and_ignores_hover() {
        for phase in [Phase::Rest, Phase::Hover, Phase::Active] {
            let on = chip(true, true, phase);
            assert!(is_pine(on.bg) && is_pine(on.border) && is_pine(on.fg));
        }
    }

    #[test]
    fn a_chip_at_rest_uses_the_translucent_neutral_fills() {
        assert_eq!(chip(false, true, Phase::Rest).bg, Token(T::NeutralFill));
        assert_eq!(chip(false, true, Phase::Hover).bg, Token(T::NeutralHover));
        assert_eq!(chip(false, false, Phase::Hover).bg, Token(T::NeutralFill));
    }

    #[test]
    fn a_view_switch_never_turns_green() {
        for checked in [false, true] {
            for phase in [Phase::Rest, Phase::Hover, Phase::Active] {
                assert!(!is_pine(segment(checked, true, phase)));
            }
        }
        assert!(!is_pine(Token(THUMB)) && !is_pine(Token(TRACK)));
    }

    #[test]
    fn a_checked_segment_reads_strong_even_without_hover() {
        assert_eq!(segment(true, true, Phase::Rest), Token(T::FgStrong));
        assert_eq!(segment(false, true, Phase::Rest), Token(T::FgMuted));
        assert_eq!(segment(false, false, Phase::Hover), Token(T::FgMuted));
    }

    #[test]
    fn invalid_keeps_the_danger_border_when_checked() {
        assert_eq!(checkbox(Mark::On, true).border, Token(T::Danger500));
        assert_eq!(checkbox(Mark::Mixed, true).bg, Token(T::Primary));
        assert_eq!(radio(true, true).border, Token(T::Danger500));
        assert_eq!(radio(true, false).border, Token(T::Primary));
        assert_eq!(checkbox(Mark::Off, false).border, Token(T::BorderControl));
    }

    #[test]
    fn a_switch_fills_with_primary_when_on() {
        assert_eq!(switch(true).bg, Token(T::Primary));
        assert_eq!(switch(true).fg, Token(T::PrimaryFg));
        assert_eq!(switch(false).bg, Token(T::BgSunken));
    }

    #[test]
    fn a_checked_tile_is_pale_green_and_hover_only_lifts_the_rest() {
        let on = tile(true, true, Phase::Hover);
        assert!(is_pine(on.bg) && is_pine(on.border) && is_pine(on.title));
        let lit = tile(false, true, Phase::Hover);
        assert_eq!(lit.bg, Token(T::BgRaised));
        assert_eq!(lit.border, Token(T::BorderStrong));
        assert_eq!(tile(false, false, Phase::Hover).bg, Token(T::Surface));
    }
}
