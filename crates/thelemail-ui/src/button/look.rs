use gpui_kit::Hsla;

use super::Variant;
use crate::theme::Theme;
use crate::tokens::ColorToken;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fill {
    Clear,
    Token(ColorToken),
    Alpha(ColorToken, f32),
}

impl Fill {
    pub fn resolve(self, theme: &Theme) -> Hsla {
        match self {
            Self::Clear => theme.color(ColorToken::Fg).alpha(0.0),
            Self::Token(token) => theme.color(token),
            Self::Alpha(token, alpha) => {
                let color = theme.color(token);
                color.alpha(color.a * alpha)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Rest,
    Hover,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags {
    pub on: bool,
    pub star: bool,
    pub busy: bool,
    pub disabled: bool,
    pub inverse: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub bg: Fill,
    pub fg: Fill,
}

const fn paint(bg: Fill, fg: Fill) -> Look {
    Look { bg, fg }
}

use ColorToken as T;
use Fill::{Alpha, Clear, Token};

const BUSY_FILL: f32 = 0.85;

fn rest(variant: Variant) -> Look {
    match variant {
        Variant::Primary => paint(Token(T::Primary), Token(T::PrimaryFg)),
        Variant::Tonal => paint(Token(T::Pine100), Token(T::Pine700)),
        Variant::Neutral => paint(Token(T::NeutralFill), Token(T::Fg)),
        Variant::Ghost | Variant::GhostDanger => paint(Clear, Token(T::FgMuted)),
        Variant::Danger => paint(Token(T::Danger100), Token(T::Danger700)),
        Variant::DangerSolid => paint(Token(T::Danger500), Token(T::PrimaryFg)),
        Variant::Caution => paint(Token(T::Brass600), Token(T::PrimaryFg)),
    }
}

fn hover(variant: Variant) -> Look {
    let base = rest(variant);
    match variant {
        Variant::Primary => paint(Token(T::PrimaryHover), base.fg),
        Variant::Tonal => paint(Token(T::TonalHover), base.fg),
        Variant::Neutral => paint(Token(T::NeutralHover), Token(T::FgStrong)),
        Variant::Ghost => paint(Token(T::GhostHover), Token(T::FgStrong)),
        Variant::GhostDanger => paint(Token(T::Danger100), Token(T::Danger700)),
        Variant::Danger => paint(Token(T::DangerSoftHover), base.fg),
        Variant::DangerSolid => paint(Token(T::DangerHover), base.fg),
        Variant::Caution => paint(Token(T::Brass500), base.fg),
    }
}

fn inverse(variant: Variant, phase: Phase) -> Option<Look> {
    let lit = phase != Phase::Rest;
    Some(match (variant, lit) {
        (Variant::Primary, false) => paint(Token(T::FgOnPine), Token(T::BgInverse)),
        (Variant::Primary, true) => paint(Alpha(T::FgOnPine, 0.88), Token(T::BgInverse)),
        (Variant::Neutral, false) => paint(Alpha(T::FgOnPine, 0.12), Token(T::FgOnPine)),
        (Variant::Ghost, false) => paint(Clear, Alpha(T::FgOnPine, 0.72)),
        (Variant::Neutral | Variant::Ghost, true) => {
            paint(Alpha(T::FgOnPine, 0.18), Token(T::FgOnPine))
        }
        _ => return None,
    })
}

fn busy(variant: Variant) -> Option<Look> {
    Some(match variant {
        Variant::Primary => paint(Alpha(T::Primary, BUSY_FILL), Token(T::PrimaryFg)),
        Variant::Tonal | Variant::Neutral => rest(variant),
        Variant::DangerSolid => paint(Alpha(T::Danger500, BUSY_FILL), Token(T::PrimaryFg)),
        _ => return None,
    })
}

fn disabled(variant: Variant) -> Look {
    if variant.is_ghost() {
        paint(Clear, Token(T::FgFaint))
    } else {
        paint(Token(T::DisabledFill), Token(T::FgFaint))
    }
}

pub fn look(variant: Variant, flags: Flags, phase: Phase) -> Look {
    if flags.busy {
        return busy(variant).unwrap_or_else(|| disabled(variant));
    }
    if flags.disabled {
        return disabled(variant);
    }
    if flags.star {
        return paint(Token(T::Brass100), Token(T::Brass700));
    }
    if flags.on && variant.is_ghost() {
        return paint(Token(T::Pine100), Token(T::Pine700));
    }
    if flags.inverse
        && let Some(look) = inverse(variant, phase)
    {
        return look;
    }
    match phase {
        Phase::Active if variant == Variant::Primary => paint(Token(T::PrimaryPress), rest(variant).fg),
        Phase::Hover | Phase::Active => hover(variant),
        Phase::Rest => rest(variant),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags() -> Flags {
        Flags::default()
    }

    #[test]
    fn disabled_wins_over_every_other_state() {
        for variant in Variant::ALL {
            let look = look(
                variant,
                Flags {
                    disabled: true,
                    on: true,
                    star: true,
                    inverse: true,
                    ..flags()
                },
                Phase::Hover,
            );
            assert_eq!(look.fg, Token(T::FgFaint), "{variant:?}");
            let want = if variant.is_ghost() { Clear } else { Token(T::DisabledFill) };
            assert_eq!(look.bg, want, "{variant:?}");
        }
    }

    #[test]
    fn busy_keeps_the_filled_roles_and_dims_the_rest() {
        let busy = Flags { busy: true, ..flags() };
        assert_eq!(
            look(Variant::Primary, busy, Phase::Hover).bg,
            Alpha(T::Primary, BUSY_FILL)
        );
        assert_eq!(look(Variant::Tonal, busy, Phase::Rest), rest(Variant::Tonal));
        assert_eq!(look(Variant::Neutral, busy, Phase::Rest), rest(Variant::Neutral));
        assert_eq!(
            look(Variant::DangerSolid, busy, Phase::Rest).bg,
            Alpha(T::Danger500, BUSY_FILL)
        );
        assert_eq!(look(Variant::Ghost, busy, Phase::Rest), disabled(Variant::Ghost));
        assert_eq!(look(Variant::Caution, busy, Phase::Rest), disabled(Variant::Caution));
    }

    #[test]
    fn pressed_ghosts_turn_pine_and_beat_hover() {
        let on = Flags { on: true, ..flags() };
        for variant in [Variant::Ghost, Variant::GhostDanger] {
            assert_eq!(
                look(variant, on, Phase::Hover),
                paint(Token(T::Pine100), Token(T::Pine700))
            );
        }
        assert_eq!(look(Variant::Neutral, on, Phase::Rest), rest(Variant::Neutral));
    }

    #[test]
    fn a_pressed_star_is_brass() {
        let star = Flags { on: true, star: true, ..flags() };
        assert_eq!(
            look(Variant::Ghost, star, Phase::Hover),
            paint(Token(T::Brass100), Token(T::Brass700))
        );
    }

    #[test]
    fn only_primary_has_a_press_colour() {
        assert_eq!(look(Variant::Primary, flags(), Phase::Active).bg, Token(T::PrimaryPress));
        assert_eq!(look(Variant::Tonal, flags(), Phase::Active), hover(Variant::Tonal));
    }

    #[test]
    fn inverse_covers_primary_neutral_and_ghost_only() {
        let inv = Flags { inverse: true, ..flags() };
        assert_eq!(look(Variant::Primary, inv, Phase::Rest).bg, Token(T::FgOnPine));
        assert_eq!(look(Variant::Ghost, inv, Phase::Hover).bg, Alpha(T::FgOnPine, 0.18));
        assert_eq!(look(Variant::Danger, inv, Phase::Rest), rest(Variant::Danger));
    }
}
