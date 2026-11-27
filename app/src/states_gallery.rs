use std::time::Duration;

use gpui_kit::{
    AnyElement, Context, ElementId, IntoElement, ParentElement, Render, SharedString, Styled,
    WeakEntity, Window, div, px,
};
use thelemail_ui::badge::{Badge, BadgeSize, BadgeStatus, BadgeTone};
use thelemail_ui::button::{Button, Size, Variant};
use thelemail_ui::empty_state::{EmptyState, EmptyTone};
use thelemail_ui::icons::IconName;
use thelemail_ui::load_state::LoadState;
use thelemail_ui::scroll_area::ScrollArea;
use thelemail_ui::separator::Separator;
use thelemail_ui::skeleton::{BodyPending, ListSkeleton};
use thelemail_ui::text::TextTokens;
use thelemail_ui::theme::{ActiveTokens, Appearance, Theme};
use thelemail_ui::tokens::{ColorToken, RADIUS_LG, T_SMALL};

use crate::gallery::{caption, frame, section};
use crate::language::Language;
use crate::router::GalleryTab;

const SLOW_LOAD: Duration = Duration::from_millis(1600);
const FAST_LOAD: Duration = Duration::from_millis(200);
const RETRY: Duration = Duration::from_millis(1500);
const PANEL: f32 = 420.0;
const SCROLL_HEIGHT: f32 = 180.0;
const SCROLL_ROWS: usize = 30;

#[derive(Default)]
pub struct StatesGallery {
    loading: bool,
    runs: usize,
    retrying: bool,
    retries: usize,
}

impl StatesGallery {
    pub fn new() -> Self {
        Self::default()
    }

    fn load(&mut self, wait: Duration, cx: &mut Context<Self>) {
        self.loading = true;
        self.runs += 1;
        let run = self.runs;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |this, cx| {
                if this.runs == run {
                    this.loading = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.retrying = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RETRY).await;
            let _ = this.update(cx, |this, cx| {
                this.retrying = false;
                this.retries += 1;
                cx.notify();
            });
        })
        .detach();
    }
}

fn side(theme: &Theme) -> &'static str {
    match theme.appearance {
        Appearance::Light => "light",
        Appearance::Dark => "dark",
    }
}

fn id(theme: &Theme, name: impl Into<SharedString>) -> ElementId {
    ElementId::Name(format!("{}-states-{}", side(theme), name.into()).into())
}

fn panel(theme: &Theme) -> gpui_kit::Div {
    div()
        .w(px(PANEL))
        .rounded(px(RADIUS_LG))
        .overflow_hidden()
        .border_1()
        .border_color(theme.color(ColorToken::BorderSoft))
        .bg(theme.color(ColorToken::Surface))
}

fn row() -> gpui_kit::Div {
    div().flex().flex_wrap().items_center().gap(px(8.0))
}

impl Render for StatesGallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = *cx.tokens();
        let language = *cx.global::<Language>();
        let this = cx.entity().downgrade();
        frame(GalleryTab::States, &current, language, |theme| {
            self.column(theme, &this)
        })
    }
}

impl StatesGallery {
    fn column(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let theme = *theme;
        div()
            .flex()
            .flex_col()
            .gap(px(28.0))
            .child(section(
                "Load state",
                self.load_states(&theme, this),
                &theme,
            ))
            .child(section("List skeleton", self.list(&theme), &theme))
            .child(section("Body pending", self.body(&theme), &theme))
            .child(section("Empty state", self.empty(&theme), &theme))
            .child(section("Badge", self.badges(&theme), &theme))
            .child(section("Separator", self.separators(&theme), &theme))
            .child(section("Scroll area", self.scroll(&theme), &theme))
            .into_any_element()
    }

    fn load_states(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let start = |name: &'static str, label: &'static str, wait: Duration| {
            let this = this.clone();
            Button::new(id(theme, name))
                .size(Size::Sm)
                .variant(Variant::Neutral)
                .label(label)
                .theme(*theme)
                .on_click(move |_, _, cx| {
                    this.update(cx, |this, cx| this.load(wait, cx)).ok();
                })
        };
        let retry = this.clone();
        let status = if self.loading {
            "loading"
        } else if self.runs == 0 {
            "not started"
        } else {
            "loaded"
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                row()
                    .child(start("slow", "Slow load", SLOW_LOAD))
                    .child(start("fast", "Fast load", FAST_LOAD)),
            )
            .child(caption(
                format!(
                    "{status}, run {}: nothing shows before 300 ms, then it stays at least 250 ms",
                    self.runs
                ),
                theme,
            ))
            .child(panel(theme).min_h(px(96.0)).child(if self.loading {
                LoadState::loading(
                    id(theme, format!("loading-{}", self.runs)),
                    "Loading messages",
                )
                .theme(*theme)
                .into_any_element()
            } else {
                div()
                    .p(px(16.0))
                    .text_token(&T_SMALL, theme)
                    .child(if self.runs == 0 {
                        "Press a load button"
                    } else {
                        "Content arrived"
                    })
                    .into_any_element()
            }))
            .child(
                LoadState::error(id(theme, "error"), "Messages could not be loaded.")
                    .retrying(self.retrying)
                    .on_retry(move |_, _, cx| {
                        retry.update(cx, |this, cx| this.retry(cx)).ok();
                    })
                    .theme(*theme),
            )
            .child(caption(
                format!("retries finished: {}", self.retries),
                theme,
            ))
            .into_any_element()
    }

    fn list(&self, theme: &Theme) -> AnyElement {
        panel(theme)
            .child(ListSkeleton::new(id(theme, "list")).rows(4).theme(*theme))
            .into_any_element()
    }

    fn body(&self, theme: &Theme) -> AnyElement {
        panel(theme)
            .p(px(20.0))
            .child(BodyPending::new(id(theme, "body")).theme(*theme))
            .into_any_element()
    }

    fn empty(&self, theme: &Theme) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                panel(theme).child(
                    EmptyState::new(id(theme, "empty"), "No messages here")
                        .icon(IconName::Inbox)
                        .description("Mail you archive or move here will show up in this folder.")
                        .theme(*theme),
                ),
            )
            .child(
                panel(theme).child(
                    EmptyState::new(id(theme, "empty-compact"), "No drafts")
                        .icon(IconName::FileLines)
                        .compact()
                        .theme(*theme),
                ),
            )
            .child(
                panel(theme).child(
                    EmptyState::new(id(theme, "empty-error"), "Search is unavailable")
                        .icon(IconName::CircleExclamation)
                        .description("The search index could not be opened.")
                        .tone(EmptyTone::Error)
                        .action(
                            Button::new(id(theme, "empty-retry"))
                                .size(Size::Sm)
                                .variant(Variant::Neutral)
                                .label("Try again")
                                .theme(*theme),
                        )
                        .theme(*theme),
                ),
            )
            .into_any_element()
    }

    fn badges(&self, theme: &Theme) -> AnyElement {
        let label = |tone: BadgeTone| match tone {
            BadgeTone::Neutral => "Neutral",
            BadgeTone::Success => "Verified",
            BadgeTone::Warning => "Pending",
            BadgeTone::Danger => "Failed",
            BadgeTone::Info => "Scheduled",
            BadgeTone::Brand => "Business",
            BadgeTone::Caution => "Trial",
        };
        let status = |s: BadgeStatus| match s {
            BadgeStatus::Success => "Active",
            BadgeStatus::Warning => "Check DNS",
            BadgeStatus::Danger => "Missing",
            BadgeStatus::Info => "Waiting",
            BadgeStatus::Neutral => "Off",
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(row().children(BadgeTone::ALL.map(|tone| {
                Badge::new(id(theme, format!("tone-{tone:?}")), label(tone))
                    .tone(tone)
                    .theme(*theme)
            })))
            .child(row().children(BadgeStatus::ALL.map(|s| {
                Badge::new(id(theme, format!("status-{s:?}")), status(s))
                    .status(s)
                    .theme(*theme)
            })))
            .child(
                row()
                    .child(
                        Badge::count(id(theme, "count-1"), 3)
                            .tone(BadgeTone::Brand)
                            .theme(*theme),
                    )
                    .child(Badge::count(id(theme, "count-2"), 128).theme(*theme))
                    .child(
                        Badge::new(id(theme, "sm"), "Small")
                            .size(BadgeSize::Sm)
                            .tone(BadgeTone::Info)
                            .theme(*theme),
                    )
                    .child(
                        Badge::new(id(theme, "sm-status"), "Owner")
                            .size(BadgeSize::Sm)
                            .status(BadgeStatus::Success)
                            .theme(*theme),
                    ),
            )
            .into_any_element()
    }

    fn separators(&self, theme: &Theme) -> AnyElement {
        div()
            .w(px(PANEL))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(div().text_token(&T_SMALL, theme).child("Above"))
            .child(Separator::horizontal().theme(*theme))
            .child(
                div()
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .text_token(&T_SMALL, theme)
                    .child("Left")
                    .child(Separator::vertical().theme(*theme))
                    .child("Right"),
            )
            .into_any_element()
    }

    fn scroll(&self, theme: &Theme) -> AnyElement {
        panel(theme)
            .h(px(SCROLL_HEIGHT))
            .child(
                ScrollArea::vertical(id(theme, "scroll"))
                    .size_full()
                    .theme(*theme)
                    .children((1..=SCROLL_ROWS).map(|n| {
                        div()
                            .px(px(16.0))
                            .py(px(9.0))
                            .text_token(&T_SMALL, theme)
                            .child(format!("Row {n}"))
                    })),
            )
            .into_any_element()
    }
}
