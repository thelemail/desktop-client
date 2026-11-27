use gpui_kit::component::{Theme as KitTheme, ThemeMode};
use gpui_kit::{
    App, BoxShadow, Context, Global, Hsla, Subscription, Window, WindowAppearance, point, px, rgba,
};

use crate::tokens::{
    ColorToken, DARK, DARK_SHADOWS, FONT_MONO, FONT_SANS, LIGHT, LIGHT_SHADOWS, Palette,
    ShadowLayer, ShadowToken, Shadows,
};

const CSS_BLUR_TO_SIGMA: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

impl Appearance {
    pub fn of(window: WindowAppearance) -> Self {
        match window {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::Dark,
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::Light,
        }
    }

    pub fn palette(self) -> &'static Palette {
        match self {
            Self::Light => &LIGHT,
            Self::Dark => &DARK,
        }
    }

    pub fn shadows(self) -> &'static Shadows {
        match self {
            Self::Light => &LIGHT_SHADOWS,
            Self::Dark => &DARK_SHADOWS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub choice: ThemeChoice,
    pub appearance: Appearance,
}

impl Global for Theme {}

impl Theme {
    pub fn apply(choice: ThemeChoice, cx: &mut App) {
        cx.set_window_appearance(match choice {
            ThemeChoice::Light => Some(WindowAppearance::Light),
            ThemeChoice::Dark => Some(WindowAppearance::Dark),
            ThemeChoice::System => None,
        });
        let appearance = match choice {
            ThemeChoice::Light => Appearance::Light,
            ThemeChoice::Dark => Appearance::Dark,
            ThemeChoice::System => Appearance::of(cx.window_appearance()),
        };
        Self::set(Self { choice, appearance }, cx);
    }

    pub fn follow_system<V: 'static>(window: &mut Window, cx: &mut Context<V>) -> Subscription {
        cx.observe_window_appearance(window, |_, window, cx| {
            let theme = *cx.tokens();
            if theme.choice != ThemeChoice::System {
                return;
            }
            let appearance = Appearance::of(window.appearance());
            if appearance != theme.appearance {
                Self::set(
                    Self {
                        appearance,
                        ..theme
                    },
                    cx,
                );
            }
        })
    }

    fn set(theme: Self, cx: &mut App) {
        let changed = cx.try_global::<Self>() != Some(&theme);
        cx.set_global(theme);
        let mode = match theme.appearance {
            Appearance::Light => ThemeMode::Light,
            Appearance::Dark => ThemeMode::Dark,
        };
        let mode_changed = !cx.has_global::<KitTheme>() || KitTheme::global(cx).mode != mode;
        if mode_changed {
            KitTheme::change(mode, None, cx);
        }
        if mode_changed || changed {
            theme.project_onto_kit(cx);
        }
        if changed {
            cx.refresh_windows();
        }
    }

    fn project_onto_kit(self, cx: &mut App) {
        KitTheme::update(cx, |kit| {
            kit.font_family = FONT_SANS.name.into();
            kit.mono_font_family = FONT_MONO.name.into();
            let colors = &mut kit.colors;
            colors.background = self.color(ColorToken::Surface);
            colors.foreground = self.color(ColorToken::FgStrong);
            colors.muted_foreground = self.color(ColorToken::FgMuted);
            colors.selection = self.color(ColorToken::Selection);
            colors.caret = self.color(ColorToken::FgStrong);
            colors.border = self.color(ColorToken::BorderSoft);
            colors.input = self.color(ColorToken::BorderControl);
            colors.ring = self.color(ColorToken::FocusRing);
        });
    }

    pub fn palette(&self) -> &'static Palette {
        self.appearance.palette()
    }

    pub fn color(&self, token: ColorToken) -> Hsla {
        hsla(self.palette().get(token))
    }

    pub fn shadow(&self, token: ShadowToken) -> Vec<BoxShadow> {
        box_shadows(self.appearance.shadows().get(token))
    }
}

pub trait ActiveTokens {
    fn tokens(&self) -> &Theme;
}

impl ActiveTokens for App {
    fn tokens(&self) -> &Theme {
        self.global::<Theme>()
    }
}

pub fn hsla(color: u32) -> Hsla {
    rgba(color).into()
}

pub fn box_shadows(layers: &[ShadowLayer]) -> Vec<BoxShadow> {
    layers
        .iter()
        .map(|layer| BoxShadow {
            color: hsla(layer.color),
            offset: point(px(layer.x), px(layer.y)),
            blur_radius: px(layer.blur * CSS_BLUR_TO_SIGMA),
            spread_radius: px(layer.spread),
            inset: layer.inset,
        })
        .collect()
}
