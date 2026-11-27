use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled, Window, div,
    px, relative,
};
use thelemail_i18n::m;

use crate::a11y::A11yStateExt as _;
use crate::button::{Button, Size, Variant};
use crate::icons::{IconName, glyph};
use crate::skeleton::Skeleton;
use crate::state::{Delayed, INDICATOR_DELAY};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_SANS, RADIUS_MD};

const ERROR_FILL: f32 = 0.45;
const ERROR_GAP: f32 = 10.0;
const ERROR_PAD_Y: f32 = 12.0;
const ERROR_PAD_X: f32 = 16.0;
const ERROR_ICON: f32 = 16.0;
const ERROR_FONT: f32 = 13.5;
const ERROR_LINE: f32 = 1.45;
const ROW_PAD_Y: f32 = 14.0;
const ROW_PAD_X: f32 = 16.0;
const ROW_TITLE: (f32, f32, f32) = (0.44, 200.0, 11.0);
const ROW_DETAIL: (f32, f32, f32) = (0.72, 320.0, 8.0);
const ROW_DETAIL_GAP: f32 = 9.0;
const ROW_DETAIL_OPACITY: f32 = 0.65;
const DEFAULT_ROWS: usize = 2;

pub fn indicator<K: PartialEq + Clone + 'static>(
    id: ElementId,
    active: bool,
    key: K,
    min_visible: Duration,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let delayed = window.use_keyed_state(id, cx, |_, _| {
        Delayed::<K>::with_timing(INDICATOR_DELAY, min_visible)
    });
    let now = Instant::now();
    let wake = delayed.update(cx, |delayed, _| delayed.update(active, key, now));
    if let Some(at) = wake {
        let wait = at.saturating_duration_since(now);
        window
            .spawn(cx, async move |cx| {
                cx.background_executor().timer(wait).await;
                let _ = cx.update(|window, _| window.refresh());
            })
            .detach();
    }
    delayed.read(cx).visible()
}

type Retry = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

enum Kind {
    Loading {
        rows: usize,
        skeleton: Option<AnyElement>,
    },
    Error {
        retry: Option<Retry>,
        retrying: bool,
    },
}

#[derive(IntoElement)]
pub struct LoadState {
    id: ElementId,
    kind: Kind,
    label: SharedString,
    theme: Option<Theme>,
}

impl LoadState {
    pub fn loading(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            kind: Kind::Loading {
                rows: DEFAULT_ROWS,
                skeleton: None,
            },
            label: label.into(),
            theme: None,
        }
    }

    pub fn error(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            kind: Kind::Error {
                retry: None,
                retrying: false,
            },
            label: label.into(),
            theme: None,
        }
    }

    pub fn rows(mut self, count: usize) -> Self {
        if let Kind::Loading { rows, .. } = &mut self.kind {
            *rows = count;
        }
        self
    }

    pub fn skeleton(mut self, element: impl IntoElement) -> Self {
        if let Kind::Loading { skeleton, .. } = &mut self.kind {
            *skeleton = Some(element.into_any_element());
        }
        self
    }

    pub fn on_retry(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        if let Kind::Error { retry, .. } = &mut self.kind {
            *retry = Some(Rc::new(handler));
        }
        self
    }

    pub fn retrying(mut self, busy: bool) -> Self {
        if let Kind::Error { retrying, .. } = &mut self.kind {
            *retrying = busy;
        }
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

fn part(id: &ElementId, name: impl Into<SharedString>) -> ElementId {
    ElementId::NamedChild(id.clone().into(), name.into())
}

impl RenderOnce for LoadState {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            id,
            kind,
            label,
            theme,
        } = self;
        let theme = theme.unwrap_or(*cx.tokens());
        match kind {
            Kind::Loading { rows, skeleton } => {
                let shown = indicator(part(&id, "delayed"), true, (), Duration::ZERO, window, cx);
                let body = shown.then(|| match skeleton {
                    Some(skeleton) => skeleton,
                    None => div()
                        .flex()
                        .flex_col()
                        .children((0..rows).map(|row| placeholder(&id, row, &theme)))
                        .into_any_element(),
                });
                div()
                    .id(id)
                    .w_full()
                    .role(Role::Status)
                    .aria_label(label)
                    .children(body)
                    .a11y_state(true, false)
                    .into_any_element()
            }
            Kind::Error { retry, retrying } => {
                let fill = theme.color(ColorToken::Danger100);
                let button = retry.map(|retry| {
                    Button::new(part(&id, "retry"))
                        .variant(Variant::Neutral)
                        .size(Size::Sm)
                        .icon(IconName::ArrowsRotate)
                        .label(m::common_retry())
                        .busy(retrying)
                        .theme(theme)
                        .on_click(move |event, window, cx| retry(event, window, cx))
                });
                div()
                    .id(id)
                    .role(Role::Alert)
                    .aria_label(label.clone())
                    .flex()
                    .items_center()
                    .gap(px(ERROR_GAP))
                    .py(px(ERROR_PAD_Y))
                    .px(px(ERROR_PAD_X))
                    .rounded(px(RADIUS_MD))
                    .bg(fill.alpha(fill.a * ERROR_FILL))
                    .child(glyph(
                        IconName::CircleExclamation,
                        px(ERROR_ICON),
                        theme.color(ColorToken::Danger500),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_family(FONT_SANS.name)
                            .text_size(px(ERROR_FONT * FONT_SANS.size_adjust))
                            .line_height(relative(ERROR_LINE))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.color(ColorToken::Danger700))
                            .child(label),
                    )
                    .children(button)
                    .into_any_element()
            }
        }
    }
}

fn placeholder(id: &ElementId, row: usize, theme: &Theme) -> AnyElement {
    let bar = |name: &'static str, (share, cap, height): (f32, f32, f32)| {
        div().w(relative(share)).max_w(px(cap)).child(
            Skeleton::new(part(id, format!("row-{row}-{name}")))
                .h(height)
                .theme(*theme),
        )
    };
    div()
        .py(px(ROW_PAD_Y))
        .px(px(ROW_PAD_X))
        .child(bar("title", ROW_TITLE))
        .child(
            div()
                .mt(px(ROW_DETAIL_GAP))
                .opacity(ROW_DETAIL_OPACITY)
                .child(bar("detail", ROW_DETAIL)),
        )
        .into_any_element()
}
