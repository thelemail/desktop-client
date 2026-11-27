use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled, Window, div, px, relative,
};

use crate::icons::{IconName, glyph};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_SANS, RADIUS_PILL};

const GAP: f32 = 8.0;
const ICON_FILL: f32 = 0.06;
const ICON_BELOW: f32 = 4.0;
const TITLE_FONT: f32 = 16.0;
const TITLE_LINE: f32 = 1.3;
const TEXT_FONT: f32 = 13.5;
const TEXT_LINE: f32 = 1.5;
const TEXT_MEASURE: f32 = 36.0 * TEXT_FONT * 0.55;
const ACTION_ABOVE: f32 = 6.0;

struct Metrics {
    pad_y: f32,
    pad_x: f32,
    disc: f32,
    glyph: f32,
}

const REGULAR: Metrics = Metrics {
    pad_y: 40.0,
    pad_x: 20.0,
    disc: 48.0,
    glyph: 22.0,
};

const COMPACT: Metrics = Metrics {
    pad_y: 20.0,
    pad_x: 16.0,
    disc: 36.0,
    glyph: 17.0,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmptyTone {
    #[default]
    Neutral,
    Error,
}

impl EmptyTone {
    pub fn role(self) -> Option<Role> {
        match self {
            Self::Error => Some(Role::Alert),
            Self::Neutral => None,
        }
    }
}

#[derive(IntoElement)]
pub struct EmptyState {
    id: ElementId,
    icon: Option<IconName>,
    title: SharedString,
    description: Option<SharedString>,
    tone: EmptyTone,
    compact: bool,
    action: Option<AnyElement>,
    theme: Option<Theme>,
}

impl EmptyState {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            icon: None,
            title: title.into(),
            description: None,
            tone: EmptyTone::default(),
            compact: false,
            action: None,
            theme: None,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    pub fn tone(mut self, tone: EmptyTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let m = if self.compact { &COMPACT } else { &REGULAR };
        let (disc, ink) = match self.tone {
            EmptyTone::Neutral => {
                let fg = theme.color(ColorToken::Fg);
                (fg.alpha(fg.a * ICON_FILL), theme.color(ColorToken::FgMuted))
            }
            EmptyTone::Error => (
                theme.color(ColorToken::Danger100),
                theme.color(ColorToken::Danger700),
            ),
        };
        let label = match &self.description {
            Some(text) => SharedString::from(format!("{}. {text}", self.title)),
            None => self.title.clone(),
        };
        div()
            .id(self.id)
            .when_some(self.tone.role(), |el, role| el.role(role).aria_label(label))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(GAP))
            .py(px(m.pad_y))
            .px(px(m.pad_x))
            .font_family(FONT_SANS.name)
            .text_center()
            .when_some(self.icon, |el, icon| {
                el.child(
                    div()
                        .flex_none()
                        .size(px(m.disc))
                        .mb(px(ICON_BELOW))
                        .rounded(px(RADIUS_PILL))
                        .bg(disc)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(glyph(icon, px(m.glyph), ink)),
                )
            })
            .child(
                div()
                    .text_size(px(TITLE_FONT * FONT_SANS.size_adjust))
                    .line_height(relative(TITLE_LINE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.color(ColorToken::FgStrong))
                    .child(self.title),
            )
            .when_some(self.description, |el, text| {
                el.child(
                    div()
                        .max_w(px(TEXT_MEASURE))
                        .text_size(px(TEXT_FONT * FONT_SANS.size_adjust))
                        .line_height(relative(TEXT_LINE))
                        .text_color(theme.color(ColorToken::FgMuted))
                        .child(text),
                )
            })
            .when_some(self.action, |el, action| {
                el.child(div().mt(px(ACTION_ABOVE)).child(action))
            })
    }
}
