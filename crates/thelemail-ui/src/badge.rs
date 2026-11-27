mod look;

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, CursorStyle, ElementId, FontFeatures, FontWeight, InteractiveElement,
    IntoElement, ParentElement, RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled,
    Window, div, px,
};

pub use look::{BadgeStatus, BadgeTone, look};

use crate::icons::{IconName, glyph};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{FONT_SANS, RADIUS_PILL};

const ICON: f32 = 12.0;
const GAP: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeSize {
    Sm,
    #[default]
    Md,
}

struct Metrics {
    height: f32,
    pad: f32,
    font: f32,
}

const fn metrics(size: BadgeSize, count: bool) -> Metrics {
    match (size, count) {
        (BadgeSize::Sm, _) => Metrics {
            height: 18.0,
            pad: 7.0,
            font: 11.0,
        },
        (BadgeSize::Md, true) => Metrics {
            height: 20.0,
            pad: 6.0,
            font: 11.5,
        },
        (BadgeSize::Md, false) => Metrics {
            height: 22.0,
            pad: 9.0,
            font: 12.0,
        },
    }
}

const COUNT_MIN: f32 = 20.0;

fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

type Activate = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Badge {
    id: ElementId,
    label: SharedString,
    tone: BadgeTone,
    size: BadgeSize,
    icon: Option<IconName>,
    count: bool,
    theme: Option<Theme>,
    on_click: Option<Activate>,
}

impl Badge {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            tone: BadgeTone::default(),
            size: BadgeSize::default(),
            icon: None,
            count: false,
            theme: None,
            on_click: None,
        }
    }

    pub fn count(id: impl Into<ElementId>, count: usize) -> Self {
        Self {
            count: true,
            ..Self::new(id, count.to_string())
        }
    }

    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn status(mut self, status: BadgeStatus) -> Self {
        self.tone = status.tone();
        self.icon = Some(status.icon());
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn size(mut self, size: BadgeSize) -> Self {
        self.size = size;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let (bg, fg) = look(self.tone);
        let fg = fg.resolve(&theme);
        let m = metrics(self.size, self.count);
        let clickable = self.on_click.clone();
        div()
            .id(self.id)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(GAP))
            .h(px(m.height))
            .px(px(m.pad))
            .when(self.count, |el| el.min_w(px(COUNT_MIN)))
            .rounded(px(RADIUS_PILL))
            .bg(bg.resolve(&theme))
            .text_color(fg)
            .font_family(FONT_SANS.name)
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(m.font * FONT_SANS.size_adjust))
            .line_height(px(m.height))
            .whitespace_nowrap()
            .when(self.count, |el| el.font_features(tabular()))
            .children(self.icon.map(|icon| glyph(icon, px(ICON), fg)))
            .child(self.label.clone())
            .when_some(clickable, |el, handler| {
                el.role(Role::Link)
                    .aria_label(self.label.clone())
                    .cursor(CursorStyle::PointingHand)
                    .on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}
