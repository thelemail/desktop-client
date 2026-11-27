use gpui_kit::{
    AnyElement, App, Div, FontWeight, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px, relative,
};
use thelemail_ui::text::{TextTokens, cased};
use thelemail_ui::theme::{ActiveTokens, Appearance, Theme, ThemeChoice};
use thelemail_ui::tokens::{
    CONTROL_HEIGHTS, ColorToken, FONT_MONO, FONT_SANS, Family, RADII, RADIUS_AVATAR_FRACTION,
    RADIUS_MD, RADIUS_SM, SOURCE_TAG, SPACING, ShadowToken, T_H2, T_LABEL, T_MONO_XS, T_SMALL,
    TEXT_STYLES, TRACKING, TYPE_SCALE, Z_LAYERS,
};

use crate::language::Language;
use crate::nav::navigator;
use crate::router::{DebugPage, GalleryTab, Route};
use crate::{appearance, language_gallery};

const SAMPLE: &str = "Sphinx of black quartz, judge my vow 0123456789";

pub fn render(tab: GalleryTab, cx: &App) -> impl IntoElement {
    let current = *cx.tokens();
    let language = *cx.global::<Language>();
    frame(tab, &current, language, |theme| tokens(theme, tab))
}

pub fn frame(
    tab: GalleryTab,
    current: &Theme,
    language: Language,
    content: impl FnMut(&Theme) -> AnyElement,
) -> Div {
    frame_in(
        tab,
        current,
        language,
        &[Appearance::Light, Appearance::Dark],
        content,
    )
}

pub fn frame_in(
    tab: GalleryTab,
    current: &Theme,
    language: Language,
    columns: &[Appearance],
    mut content: impl FnMut(&Theme) -> AnyElement,
) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(current.color(ColorToken::BgSunken))
        .child(header(current, tab, language))
        .child(
            div()
                .id("token-gallery")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(
                    div().flex().w_full().children(
                        columns
                            .iter()
                            .map(|appearance| column(theme(*appearance, current), &mut content)),
                    ),
                ),
        )
}

fn tab_label(tab: GalleryTab) -> &'static str {
    match tab {
        GalleryTab::Colours => "Colours",
        GalleryTab::Shadows => "Shadows",
        GalleryTab::Shapes => "Shapes",
        GalleryTab::Type => "Type",
        GalleryTab::Motion => "Motion",
        GalleryTab::Icons => "Icons",
        GalleryTab::Language => "Language",
        GalleryTab::Buttons => "Buttons",
        GalleryTab::Fields => "Fields",
    }
}

fn theme(appearance: Appearance, current: &Theme) -> Theme {
    Theme {
        appearance,
        ..*current
    }
}

fn header(current: &Theme, tab: GalleryTab, language: Language) -> impl IntoElement {
    let choices = [
        (ThemeChoice::Light, "Light"),
        (ThemeChoice::Dark, "Dark"),
        (ThemeChoice::System, "System"),
    ];
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .px(px(24.0))
        .py(px(12.0))
        .border_b_1()
        .border_color(current.color(ColorToken::BorderSoft))
        .bg(current.color(ColorToken::Bg))
        .child(
            div()
                .text_token(&T_LABEL, current)
                .child(format!("Tokens from web-client {SOURCE_TAG}")),
        )
        .child(segmented(
            GalleryTab::ALL.into_iter().map(|(_, each)| {
                (tab_label(each), each == tab, move |cx: &mut App| {
                    navigator(cx).update(cx, |nav, cx| {
                        nav.push(Route::Debug(DebugPage::Tokens(each)), cx)
                    })
                })
            }),
            current,
        ))
        .child(segmented(
            choices.into_iter().map(|(choice, label)| {
                (label, current.choice == choice, move |cx: &mut App| {
                    appearance::set(choice, cx)
                })
            }),
            current,
        ))
        .child(segmented(
            language_gallery::choices()
                .into_iter()
                .map(|(choice, label)| {
                    (label, language.choice == choice, move |cx: &mut App| {
                        language_gallery::pick(choice, cx)
                    })
                }),
            current,
        ))
}

pub fn segmented<F: Fn(&mut App) + 'static>(
    items: impl Iterator<Item = (&'static str, bool, F)>,
    current: &Theme,
) -> impl IntoElement {
    div()
        .flex()
        .gap(px(4.0))
        .p(px(2.0))
        .rounded(px(RADIUS_MD))
        .bg(current.color(ColorToken::NeutralFill))
        .children(items.map(|(label, active, pick)| {
            div()
                .id(label)
                .px(px(12.0))
                .py(px(4.0))
                .rounded(px(RADIUS_MD - 2.0))
                .cursor_pointer()
                .when_active(active, current)
                .text_token(&T_LABEL, current)
                .child(label)
                .on_click(move |_, _, cx| pick(cx))
        }))
}

trait Segment {
    fn when_active(self, active: bool, theme: &Theme) -> Self;
}

impl<E: Styled> Segment for E {
    fn when_active(self, active: bool, theme: &Theme) -> Self {
        if active {
            self.bg(theme.color(ColorToken::Surface))
                .shadow(theme.shadow(ShadowToken::Sm))
        } else {
            self
        }
    }
}

fn column(theme: Theme, content: &mut impl FnMut(&Theme) -> AnyElement) -> Div {
    let title = match theme.appearance {
        Appearance::Light => "Light",
        Appearance::Dark => "Dark",
    };
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(28.0))
        .p(px(24.0))
        .bg(theme.color(ColorToken::Bg))
        .child(div().text_token(&T_H2, &theme).child(title))
        .child(content(&theme))
}

fn tokens(theme: &Theme, tab: GalleryTab) -> AnyElement {
    let theme = *theme;
    let body = div().flex().flex_col().gap(px(28.0));
    match tab {
        GalleryTab::Colours => body.child(colours(&theme)),
        GalleryTab::Shadows => body.child(shadows(&theme)),
        GalleryTab::Shapes => body
            .child(section("Radii", radii(&theme), &theme))
            .child(section("Controls", controls(&theme), &theme))
            .child(section("Spacing", spacing(&theme), &theme))
            .child(section("Layers and tracking", layers(&theme), &theme)),
        GalleryTab::Type => body
            .child(section("Text styles", text_styles(&theme), &theme))
            .child(section("Type scale", type_scale(&theme), &theme))
            .child(section("Weights", weights(&theme), &theme)),
        GalleryTab::Language => body.child(language_gallery::render(&theme)),
        GalleryTab::Motion | GalleryTab::Icons | GalleryTab::Buttons | GalleryTab::Fields => body,
    }
    .into_any_element()
}

pub fn section(title: &str, body: AnyElement, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(
            div()
                .text_token(&T_SMALL, theme)
                .font_weight(FontWeight(600.0))
                .child(title.to_string()),
        )
        .child(body)
}

pub fn caption(text: String, theme: &Theme) -> Div {
    div().text_token(&T_MONO_XS, theme).child(text)
}

fn colours(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .gap_y(px(8.0))
        .children(ColorToken::ALL.into_iter().map(|token| {
            div()
                .w(relative(1.0 / 3.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_none()
                        .size(px(28.0))
                        .rounded(px(RADIUS_SM))
                        .border_1()
                        .border_color(theme.color(ColorToken::BorderSoft))
                        .bg(theme.color(token)),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(caption(token.name().to_string(), theme))
                        .child(caption(
                            format!("#{:08X}", theme.palette().get(token)),
                            theme,
                        )),
                )
        }))
        .into_any_element()
}

fn shadows(theme: &Theme) -> AnyElement {
    let row = |ground: ColorToken| {
        div()
            .flex()
            .flex_wrap()
            .gap(px(24.0))
            .p(px(24.0))
            .rounded(px(RADIUS_MD))
            .bg(theme.color(ground))
            .children(ShadowToken::ALL.into_iter().map(|token| {
                div()
                    .w(px(112.0))
                    .h(px(72.0))
                    .rounded(px(RADIUS_MD))
                    .bg(theme.color(ColorToken::Surface))
                    .shadow(theme.shadow(token))
                    .flex()
                    .items_end()
                    .p(px(8.0))
                    .child(caption(token.name().to_string(), theme))
            }))
    };
    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(row(ColorToken::Bg))
        .child(row(ColorToken::BgRaised))
        .into_any_element()
}

fn radii(theme: &Theme) -> AnyElement {
    let boxed = |label: String, radius: f32| {
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .size(px(64.0))
                    .rounded(px(radius))
                    .border_1()
                    .border_color(theme.color(ColorToken::BorderStrong))
                    .bg(theme.color(ColorToken::Surface)),
            )
            .child(caption(label, theme))
    };
    div()
        .flex()
        .flex_wrap()
        .gap(px(16.0))
        .children(
            RADII
                .into_iter()
                .map(|(name, radius)| boxed(format!("{name} {radius}"), radius)),
        )
        .child(boxed(
            format!("avatar {}%", RADIUS_AVATAR_FRACTION * 100.0),
            64.0 * RADIUS_AVATAR_FRACTION,
        ))
        .into_any_element()
}

fn controls(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .items_end()
        .gap(px(16.0))
        .children(CONTROL_HEIGHTS.into_iter().map(|control| {
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .w(px(120.0))
                        .h(px(control.height))
                        .rounded(px(control.radius))
                        .border_1()
                        .border_color(theme.color(ColorToken::BorderControl))
                        .bg(theme.color(ColorToken::Surface)),
                )
                .child(caption(
                    format!("{} {}/{}", control.name, control.height, control.radius),
                    theme,
                ))
        }))
        .into_any_element()
}

fn spacing(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .children(SPACING.into_iter().map(|(name, value)| {
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .w(px(80.0))
                        .child(caption(format!("{name} {value}"), theme)),
                )
                .child(
                    div()
                        .w(px(value))
                        .h(px(12.0))
                        .rounded(px(2.0))
                        .bg(theme.color(ColorToken::Pine500)),
                )
        }))
        .into_any_element()
}

fn layers(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .children(
            Z_LAYERS
                .into_iter()
                .map(|(name, z)| caption(format!("z-{name} {z}"), theme)),
        )
        .children(
            TRACKING
                .into_iter()
                .map(|(name, em)| caption(format!("track-{name} {em}em"), theme)),
        )
        .into_any_element()
}

fn text_styles(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(16.0))
        .children(TEXT_STYLES.into_iter().map(|style| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(caption(
                    format!(
                        "{} {}/{} w{} {}em {}",
                        style.name,
                        style.size,
                        style.line_height,
                        style.weight,
                        style.tracking,
                        style.color.name()
                    ),
                    theme,
                ))
                .child(
                    div()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_token(&style, theme)
                        .child(cased(&style, SAMPLE)),
                )
        }))
        .into_any_element()
}

fn type_scale(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .children(TYPE_SCALE.into_iter().map(|size| {
            div()
                .flex()
                .items_baseline()
                .gap(px(12.0))
                .child(div().w(px(120.0)).child(caption(
                    format!("{} {}/{}", size.name, size.size, size.line_height),
                    theme,
                )))
                .child(
                    div()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .family(&FONT_SANS)
                        .text_size(px(size.size * FONT_SANS.size_adjust))
                        .line_height(px(size.line_height))
                        .text_color(theme.color(ColorToken::Fg))
                        .child("Ag"),
                )
        }))
        .into_any_element()
}

fn weights(theme: &Theme) -> AnyElement {
    let ladder = |family: &Family| {
        let mut weight = family.min_weight;
        let mut rows = Vec::new();
        while weight <= family.max_weight {
            rows.push(
                div()
                    .flex()
                    .items_baseline()
                    .overflow_hidden()
                    .gap(px(12.0))
                    .child(div().w(px(40.0)).child(caption(format!("{weight}"), theme)))
                    .child(
                        div()
                            .whitespace_nowrap()
                            .family(family)
                            .text_size(px(17.0 * family.size_adjust))
                            .line_height(px(28.0))
                            .font_weight(FontWeight(weight))
                            .text_color(theme.color(ColorToken::Fg))
                            .child(format!("{} {SAMPLE}", family.name)),
                    ),
            );
            weight += 100.0;
        }
        div().flex().flex_col().children(rows)
    };
    div()
        .flex()
        .flex_col()
        .gap(px(16.0))
        .child(ladder(&FONT_SANS))
        .child(ladder(&FONT_MONO))
        .into_any_element()
}
