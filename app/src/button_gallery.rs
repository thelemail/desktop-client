use gpui_kit::{
    AnyElement, App, Context, ElementId, IntoElement, ParentElement, Render, SharedString, Styled,
    WeakEntity, Window, div, px,
};
use thelemail_ui::button::{Button, Size, SplitButton, Variant};
use thelemail_ui::icons::IconName;
use thelemail_ui::theme::{ActiveTokens, Appearance, Theme};
use thelemail_ui::tokens::{ColorToken, RADIUS_LG};

use crate::gallery::{caption, frame, section};
use crate::language::Language;
use crate::router::GalleryTab;

pub struct ButtonGallery {
    busy: bool,
    pressed: bool,
    starred: bool,
    expanded: bool,
}

impl ButtonGallery {
    pub fn new() -> Self {
        Self {
            busy: false,
            pressed: false,
            starred: false,
            expanded: false,
        }
    }
}

fn variant_name(variant: Variant) -> &'static str {
    match variant {
        Variant::Primary => "primary",
        Variant::Tonal => "tonal",
        Variant::Neutral => "neutral",
        Variant::Ghost => "ghost",
        Variant::GhostDanger => "ghostDanger",
        Variant::Danger => "danger",
        Variant::DangerSolid => "dangerSolid",
        Variant::Caution => "caution",
    }
}

fn size_name(size: Size) -> &'static str {
    match size {
        Size::Sm => "sm",
        Size::Md => "md",
        Size::Lg => "lg",
        Size::IconXs => "icon-xs",
        Size::IconSm => "icon-sm",
        Size::Icon => "icon",
        Size::IconLg => "icon-lg",
    }
}

fn side(theme: &Theme) -> &'static str {
    match theme.appearance {
        Appearance::Light => "light",
        Appearance::Dark => "dark",
    }
}

fn id(theme: &Theme, name: impl Into<SharedString>) -> ElementId {
    ElementId::Name(format!("{}-{}", side(theme), name.into()).into())
}

fn row() -> gpui_kit::Div {
    div().flex().flex_wrap().items_center().gap(px(10.0))
}

fn update(
    this: &WeakEntity<ButtonGallery>,
    f: impl Fn(&mut ButtonGallery) + 'static,
) -> impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static {
    let this = this.clone();
    move |_, _, cx: &mut App| {
        this.update(cx, |this, cx| {
            f(this);
            cx.notify();
        })
        .ok();
    }
}

impl Render for ButtonGallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = *cx.tokens();
        let language = *cx.global::<Language>();
        let this = cx.entity().downgrade();
        frame(GalleryTab::Buttons, &current, language, |theme| {
            self.column(theme, &this)
        })
    }
}

impl ButtonGallery {
    fn column(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let theme = *theme;
        div()
            .flex()
            .flex_col()
            .gap(px(28.0))
            .child(section("Variants and sizes", self.grid(&theme), &theme))
            .child(section("Icon sizes", self.icons(&theme), &theme))
            .child(section("States", self.states(&theme, this), &theme))
            .child(section("Toggles", self.toggles(&theme, this), &theme))
            .child(section("Block and split", self.block(&theme), &theme))
            .child(section("On inverse", self.inverse(&theme), &theme))
            .into_any_element()
    }

    fn grid(&self, theme: &Theme) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .children(Variant::ALL.into_iter().map(|variant| {
                let name = variant_name(variant);
                row()
                    .child(div().w(px(96.0)).child(caption(name.into(), theme)))
                    .children([Size::Sm, Size::Md, Size::Lg].into_iter().map(|size| {
                        Button::new(id(theme, format!("{name}-{}", size_name(size))))
                            .variant(variant)
                            .size(size)
                            .label("Continue")
                            .theme(*theme)
                    }))
                    .child(
                        Button::new(id(theme, format!("{name}-icon-label")))
                            .variant(variant)
                            .icon(IconName::PaperPlaneTop)
                            .label("Send")
                            .theme(*theme),
                    )
            }))
            .into_any_element()
    }

    fn icons(&self, theme: &Theme) -> AnyElement {
        row()
            .children(
                [Size::IconXs, Size::IconSm, Size::Icon, Size::IconLg]
                    .into_iter()
                    .flat_map(|size| {
                        [Variant::Ghost, Variant::Neutral, Variant::Primary]
                            .into_iter()
                            .map(move |variant| (size, variant))
                    })
                    .map(|(size, variant)| {
                        Button::new(id(
                            theme,
                            format!("{}-{}", size_name(size), variant_name(variant)),
                        ))
                        .variant(variant)
                        .size(size)
                        .icon(IconName::BoxArchive)
                        .tooltip("Archive")
                        .shortcut("E")
                        .theme(*theme)
                    }),
            )
            .into_any_element()
    }

    fn states(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let busy = self.busy;
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                row()
                    .child(
                        Button::new(id(theme, "toggle-busy"))
                            .variant(Variant::Neutral)
                            .size(Size::Sm)
                            .label(if busy { "Stop" } else { "Make busy" })
                            .theme(*theme)
                            .on_click(update(this, |g| g.busy = !g.busy)),
                    )
                    .child(caption("busy keeps focus and width".into(), theme)),
            )
            .children(Variant::ALL.into_iter().map(|variant| {
                let name = variant_name(variant);
                row()
                    .child(div().w(px(96.0)).child(caption(name.into(), theme)))
                    .child(
                        Button::new(id(theme, format!("{name}-busy")))
                            .variant(variant)
                            .label("Save changes")
                            .busy(busy)
                            .busy_label("Saving…")
                            .theme(*theme),
                    )
                    .child(
                        Button::new(id(theme, format!("{name}-spin")))
                            .variant(variant)
                            .label("Delete")
                            .busy(busy)
                            .theme(*theme),
                    )
                    .child(
                        Button::new(id(theme, format!("{name}-disabled")))
                            .variant(variant)
                            .label("Disabled")
                            .disabled(true)
                            .theme(*theme),
                    )
            }))
            .into_any_element()
    }

    fn toggles(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        row()
            .child(
                Button::new(id(theme, "pressed-ghost"))
                    .ghost()
                    .size(Size::Icon)
                    .icon(IconName::Bold)
                    .pressed(self.pressed)
                    .tooltip("Bold")
                    .shortcut("⌘B")
                    .theme(*theme)
                    .on_click(update(this, |g| g.pressed = !g.pressed)),
            )
            .child(
                Button::new(id(theme, "pressed-danger"))
                    .variant(Variant::GhostDanger)
                    .size(Size::Icon)
                    .icon(IconName::TrashCan)
                    .pressed(self.pressed)
                    .tooltip("Delete")
                    .theme(*theme)
                    .on_click(update(this, |g| g.pressed = !g.pressed)),
            )
            .child(
                Button::new(id(theme, "star"))
                    .ghost()
                    .size(Size::Icon)
                    .icon(if self.starred {
                        IconName::StarSolid
                    } else {
                        IconName::Star
                    })
                    .star()
                    .pressed(self.starred)
                    .tooltip("Star")
                    .shortcut("S")
                    .theme(*theme)
                    .on_click(update(this, |g| g.starred = !g.starred)),
            )
            .child(
                Button::new(id(theme, "expanded"))
                    .ghost()
                    .label("Move to")
                    .trailing(IconName::ChevronDown)
                    .expanded(self.expanded)
                    .tooltip("Tooltips hide while open")
                    .theme(*theme)
                    .on_click(update(this, |g| g.expanded = !g.expanded)),
            )
            .into_any_element()
    }

    fn block(&self, theme: &Theme) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div().w(px(320.0)).child(
                    Button::new(id(theme, "block"))
                        .primary()
                        .size(Size::Lg)
                        .label("Sign in")
                        .block()
                        .theme(*theme),
                ),
            )
            .child(
                row()
                    .child(
                        SplitButton::new(
                            id(theme, "split"),
                            Button::new(id(theme, "split-main")).primary().label("Send"),
                            Button::new(id(theme, "split-menu"))
                                .primary()
                                .size(Size::Icon)
                                .icon(IconName::ChevronDown)
                                .tooltip("Send later"),
                        )
                        .theme(*theme),
                    )
                    .child(
                        SplitButton::new(
                            id(theme, "split-neutral"),
                            Button::new(id(theme, "split-neutral-main"))
                                .label("Reply")
                                .size(Size::Sm),
                            Button::new(id(theme, "split-neutral-menu"))
                                .size(Size::IconSm)
                                .icon(IconName::ChevronDown)
                                .tooltip("Reply all"),
                        )
                        .theme(*theme),
                    ),
            )
            .into_any_element()
    }

    fn inverse(&self, theme: &Theme) -> AnyElement {
        row()
            .p(px(16.0))
            .rounded(px(RADIUS_LG))
            .bg(theme.color(ColorToken::BgInverse))
            .children(
                [Variant::Primary, Variant::Neutral, Variant::Ghost]
                    .into_iter()
                    .map(|variant| {
                        Button::new(id(theme, format!("inverse-{}", variant_name(variant))))
                            .variant(variant)
                            .label("Upgrade")
                            .on_inverse()
                            .theme(*theme)
                    }),
            )
            .into_any_element()
    }
}
