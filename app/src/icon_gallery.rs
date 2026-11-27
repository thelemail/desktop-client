use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, Context, ElementId, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window, div, px,
};
use thelemail_ui::icons::{Gesture, Icon, IconName, IconStyle, StateIcon};
use thelemail_ui::text::TextTokens;
use thelemail_ui::theme::{ActiveTokens, Theme};
use thelemail_ui::tokens::{ColorToken, RADIUS_SM, T_SMALL};

use crate::gallery::{caption, frame, section};
use crate::language::Language;
use crate::motion_gallery::FrameMeter;
use crate::router::GalleryTab;

const FOLDERS: [(&str, IconName, IconName, Gesture); 10] = [
    (
        "Inbox",
        IconName::Envelope,
        IconName::EnvelopeSolid,
        Gesture::Drop,
    ),
    (
        "Starred",
        IconName::Star,
        IconName::StarSolid,
        Gesture::Spin,
    ),
    (
        "Sent",
        IconName::PaperPlane,
        IconName::PaperPlaneSolid,
        Gesture::Fly,
    ),
    (
        "Drafts",
        IconName::FileLines,
        IconName::FileLinesSolid,
        Gesture::Tilt,
    ),
    (
        "Archive",
        IconName::BoxArchive,
        IconName::BoxArchiveSolid,
        Gesture::Drop,
    ),
    (
        "Snoozed",
        IconName::Clock,
        IconName::ClockSolid,
        Gesture::Ring,
    ),
    (
        "Scheduled",
        IconName::PaperPlaneTop,
        IconName::PaperPlaneTopSolid,
        Gesture::FlyLevel,
    ),
    (
        "Spam",
        IconName::ShieldExclamation,
        IconName::ShieldExclamationSolid,
        Gesture::Pulse,
    ),
    (
        "Trash",
        IconName::TrashCan,
        IconName::TrashCanSolid,
        Gesture::Shake,
    ),
    (
        "Folder",
        IconName::Folder,
        IconName::FolderOpenSolid,
        Gesture::Drop,
    ),
];

const STAR_SIZES: [f32; 3] = [13.0, 15.0, 20.0];
const SIZES: [f32; 6] = [12.0, 13.0, 15.0, 16.0, 20.0, 24.0];
const SIZE_SAMPLES: [IconName; 4] = [
    IconName::Envelope,
    IconName::MagnifyingGlass,
    IconName::PaperPlaneTop,
    IconName::Apple,
];
const TINTS: [ColorToken; 8] = [
    ColorToken::FgStrong,
    ColorToken::Fg,
    ColorToken::FgMuted,
    ColorToken::FgFaint,
    ColorToken::Primary,
    ColorToken::Link,
    ColorToken::Warning500,
    ColorToken::Danger500,
];

pub struct IconGallery {
    meter: FrameMeter,
    folder: usize,
    starred: [bool; STAR_SIZES.len()],
}

impl IconGallery {
    pub fn new() -> Self {
        Self {
            meter: FrameMeter::default(),
            folder: 0,
            starred: [false; STAR_SIZES.len()],
        }
    }
}

impl Render for IconGallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.meter.tick(cx.background_executor().now());
        let current = *cx.tokens();
        let this = cx.entity().downgrade();
        let language = *cx.global::<Language>();
        frame(GalleryTab::Icons, &current, language, |theme| {
            self.column(theme, &this, cx)
        })
    }
}

fn update(
    this: &WeakEntity<IconGallery>,
    f: impl Fn(&mut IconGallery) + 'static,
) -> impl Fn(&mut App) + 'static {
    let this = this.clone();
    move |cx: &mut App| {
        this.update(cx, |this, cx| {
            f(this);
            cx.notify();
        })
        .ok();
    }
}

impl IconGallery {
    fn column(&self, theme: &Theme, this: &WeakEntity<Self>, cx: &App) -> AnyElement {
        let side: SharedString = format!("{:?}", theme.appearance).into();
        div()
            .flex()
            .flex_col()
            .gap(px(28.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(caption(self.meter.summary(), theme))
                    .child(caption(
                        if cx.reduce_motion() {
                            "reduced motion: gestures skipped, crossfade instant".to_string()
                        } else {
                            "motion allowed".to_string()
                        },
                        theme,
                    )),
            )
            .child(section(
                "State icons",
                self.folders(theme, &side, this),
                theme,
            ))
            .child(section("Star", self.stars(theme, &side, this), theme))
            .child(section("Sizes", sizes(theme), theme))
            .child(section("Colours", tints(theme), theme))
            .child(section(
                &format!("Every icon ({})", IconName::ALL.len()),
                every(theme),
                theme,
            ))
            .into_any_element()
    }

    fn folders(&self, theme: &Theme, side: &SharedString, this: &WeakEntity<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .w(px(220.0))
            .gap(px(2.0))
            .children(
                FOLDERS
                    .iter()
                    .enumerate()
                    .map(|(index, &(label, icon, lit, gesture))| {
                        let on = self.folder == index;
                        let pick = update(this, move |g| g.folder = index);
                        let fg = if on {
                            ColorToken::FgStrong
                        } else {
                            ColorToken::FgMuted
                        };
                        div()
                            .id(ElementId::Name(format!("{side}-folder-{index}").into()))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .px(px(10.0))
                            .py(px(6.0))
                            .rounded(px(RADIUS_SM))
                            .when(on, |row| row.bg(theme.color(ColorToken::NeutralFill)))
                            .cursor_pointer()
                            .on_click(move |_, _, cx| pick(cx))
                            .child(
                                StateIcon::new(
                                    ElementId::Name(format!("{side}-folder-icon-{index}").into()),
                                    icon,
                                )
                                .active_icon(lit)
                                .active(on)
                                .gesture(gesture)
                                .size(16.0)
                                .color(theme.color(fg)),
                            )
                            .child(
                                div()
                                    .text_token(&T_SMALL, theme)
                                    .text_color(theme.color(fg))
                                    .child(label),
                            )
                            .child(div().flex_1())
                            .child(caption(gesture.name().to_string(), theme))
                    }),
            )
            .into_any_element()
    }

    fn stars(&self, theme: &Theme, side: &SharedString, this: &WeakEntity<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(20.0))
            .children(STAR_SIZES.iter().enumerate().map(|(index, &size)| {
                let on = self.starred[index];
                let flip = update(this, move |g| g.starred[index] = !g.starred[index]);
                let tint = if on {
                    ColorToken::Brass500
                } else {
                    ColorToken::FgMuted
                };
                div()
                    .id(ElementId::Name(format!("{side}-star-{index}").into()))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| flip(cx))
                    .child(
                        StateIcon::new(
                            ElementId::Name(format!("{side}-star-icon-{index}").into()),
                            IconName::Star,
                        )
                        .active_icon(IconName::StarSolid)
                        .active(on)
                        .gesture(Gesture::Spin)
                        .size(size)
                        .color(theme.color(tint)),
                    )
                    .child(caption(format!("{size}px"), theme))
            }))
            .into_any_element()
    }
}

fn sizes(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .children(SIZE_SAMPLES.iter().map(|&icon| {
            div()
                .flex()
                .items_end()
                .gap(px(14.0))
                .children(SIZES.iter().map(move |&size| {
                    Icon::hidden(icon)
                        .size(size)
                        .color(theme.color(ColorToken::Fg))
                }))
                .child(caption(icon.name().to_string(), theme))
        }))
        .into_any_element()
}

fn tints(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .gap(px(16.0))
        .children(TINTS.iter().map(|&token| {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    Icon::hidden(IconName::Envelope)
                        .size(16.0)
                        .color(theme.color(token)),
                )
                .child(caption(format!("{token:?}"), theme))
        }))
        .into_any_element()
}

fn every(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .gap_y(px(10.0))
        .children(IconName::ALL.iter().map(|&icon| {
            let style = match icon.style() {
                IconStyle::Regular => "",
                IconStyle::Solid => " (solid)",
                IconStyle::Brands => " (brands)",
            };
            div()
                .w(px(200.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    Icon::hidden(icon)
                        .size(16.0)
                        .color(theme.color(ColorToken::Fg)),
                )
                .child(caption(format!("{}{style}", icon.name()), theme))
        }))
        .into_any_element()
}
