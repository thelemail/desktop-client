use std::time::Duration;

use gpui_kit::base::{ReduceMotionPreference, set_reduce_motion_preference};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Render, Role, SharedString, StatefulInteractiveElement, Styled, Subscription,
    WeakEntity, Window, div, px,
};
use thelemail_ui::button::{Button, Size, Variant};
use thelemail_ui::icons::IconName;
use thelemail_ui::overlay::{
    AccountTransition, Check, ConfirmDialog, ContextMenu, Dialog, DialogSize, DropdownMenu,
    LinkDialog, LinkDialogEvent, MenuEntry, MenuItem, Popover, Sheet, Tip, Toast, ToastTone,
    Toasts, Tone, Tooltipped as _, Transition, announce_arrival, covered, native_surface,
};
use thelemail_ui::text::TextTokens;
use thelemail_ui::theme::{ActiveTokens, Theme};
use thelemail_ui::tokens::{ColorToken, RADIUS_MD, T_SMALL};

use crate::gallery::{caption, frame_in, section, segmented};
use crate::language::Language;
use crate::router::GalleryTab;

const WORK: Duration = Duration::from_millis(1200);
const SWITCH: Duration = Duration::from_millis(1400);
const ARRIVING: &str = "juniper@calloway.thelemail.test";
const FOLDERS: [&str; 4] = ["Receipts", "Travel", "Family", "Archive 2025"];
const SORTS: [&str; 3] = ["Newest first", "Oldest first", "Unread first"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confirm {
    Delete,
    Send,
}

pub struct OverlayGallery {
    reduce: ReduceMotionPreference,
    dialog: Option<(DialogSize, Tone)>,
    sheet: bool,
    confirm: Option<Confirm>,
    busy: bool,
    error: Option<SharedString>,
    attempts: u8,
    link: Entity<LinkDialog>,
    link_value: Option<SharedString>,
    transition: Option<Transition>,
    unread_first: bool,
    threads: Check,
    sort: usize,
    last: SharedString,
    row_focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl OverlayGallery {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let link = cx.new(|cx| LinkDialog::new("gallery-link", window, cx));
        let subscriptions = vec![cx.subscribe(&link, |this, _, event, cx| {
            match event {
                LinkDialogEvent::Apply(value) => {
                    this.link_value = (!value.is_empty()).then(|| value.clone());
                    this.last = if value.is_empty() {
                        "Link removed".into()
                    } else {
                        format!("Link set to {value}").into()
                    };
                }
                LinkDialogEvent::Close => this.last = "Link dialog closed".into(),
            }
            cx.notify();
        })];
        Self {
            reduce: ReduceMotionPreference::System,
            dialog: None,
            sheet: false,
            confirm: None,
            busy: false,
            error: None,
            attempts: 0,
            link,
            link_value: None,
            transition: None,
            unread_first: true,
            threads: Check::Mixed,
            sort: 0,
            last: "Nothing picked yet".into(),
            row_focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    fn picked(&mut self, what: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.last = what.into();
        cx.notify();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WORK).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                this.attempts += 1;
                if this.attempts == 1 {
                    this.error = Some("Couldn't delete the folder. Try again.".into());
                } else {
                    this.last = "Folder deleted".into();
                    this.confirm = None;
                    this.attempts = 0;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn transition(&mut self, next: Transition, window: &mut Window, cx: &mut Context<Self>) {
        let arriving = next.address.clone();
        self.transition = Some(next);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SWITCH).await;
            this.update_in(cx, |this, window, cx| {
                this.transition = None;
                if let Some(address) = arriving {
                    announce_arrival(&address, window, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

fn act(
    this: &WeakEntity<OverlayGallery>,
    f: impl Fn(&mut OverlayGallery, &mut Window, &mut Context<OverlayGallery>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let this = this.clone();
    move |window, cx| {
        this.update(cx, |this, cx| f(this, window, cx)).ok();
    }
}

fn click(
    this: &WeakEntity<OverlayGallery>,
    f: impl Fn(&mut OverlayGallery, &mut Window, &mut Context<OverlayGallery>) + 'static,
) -> impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static {
    let act = act(this, f);
    move |_, window, cx| act(window, cx)
}

fn row() -> gpui_kit::Div {
    div().flex().flex_wrap().items_center().gap(px(12.0))
}

fn paragraph(text: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .text_token(&T_SMALL, theme)
        .text_color(theme.color(ColorToken::FgMuted))
        .child(text)
}

impl Render for OverlayGallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = *cx.tokens();
        let language = *cx.global::<Language>();
        let this = cx.entity().downgrade();
        let reduced = cx.reduce_motion();
        let covered = covered(window, cx);
        let surface = native_surface("gallery-native-surface", window, cx);
        let mut surface = Some(surface);
        frame_in(
            GalleryTab::Overlays,
            &current,
            language,
            &[current.appearance],
            |theme| self.column(theme, &this, reduced, covered, surface.take(), window, cx),
        )
    }
}

impl OverlayGallery {
    #[allow(clippy::too_many_arguments)]
    fn column(
        &self,
        theme: &Theme,
        this: &WeakEntity<Self>,
        reduced: bool,
        covered: bool,
        surface: Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let theme = *theme;
        div()
            .flex()
            .flex_col()
            .gap(px(28.0))
            .child(section(
                "Motion",
                self.controls(&theme, this, reduced),
                &theme,
            ))
            .child(section(
                "Dialogs and sheet",
                self.dialogs(&theme, this),
                &theme,
            ))
            .child(section(
                "Confirm and link",
                self.confirms(&theme, this),
                &theme,
            ))
            .child(section(
                "Popover and menus",
                self.menus(&theme, this),
                &theme,
            ))
            .child(section(
                "Context menu",
                self.context(&theme, this, window),
                &theme,
            ))
            .child(section("Tooltips", self.tips(&theme, window, cx), &theme))
            .child(section("Toasts", self.toasts(&theme), &theme))
            .child(section(
                "Account transition",
                self.transitions(&theme, this),
                &theme,
            ))
            .child(section(
                "Native surface",
                self.native(&theme, this, covered, surface),
                &theme,
            ))
            .child(caption(format!("Last: {}", self.last), &theme))
            .child(self.overlays(&theme, this))
            .into_any_element()
    }

    fn controls(&self, theme: &Theme, this: &WeakEntity<Self>, reduced: bool) -> AnyElement {
        let choices = [
            (ReduceMotionPreference::System, "System"),
            (ReduceMotionPreference::Reduce, "Reduce"),
            (ReduceMotionPreference::Allow, "Allow"),
        ];
        let reduce = self.reduce;
        let this = this.clone();
        row()
            .child(div().text_token(&T_SMALL, theme).child("Reduced motion"))
            .child(segmented(
                "overlay-reduce-motion",
                choices.into_iter().map(move |(choice, label)| {
                    let this = this.clone();
                    (label, reduce == choice, move |cx: &mut App| {
                        set_reduce_motion_preference(choice, cx);
                        this.update(cx, |this, cx| {
                            this.reduce = choice;
                            cx.notify();
                        })
                        .ok();
                    })
                }),
                theme,
            ))
            .child(caption(
                if reduced { "reducing" } else { "moving" }.to_string(),
                theme,
            ))
            .into_any_element()
    }

    fn dialogs(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let sizes = [
            (DialogSize::Sm, "Small", Tone::Neutral),
            (DialogSize::Md, "Medium", Tone::Brand),
            (DialogSize::Wide, "Wide", Tone::Success),
            (DialogSize::Lg, "Large", Tone::Warning),
            (DialogSize::Xl, "Extra large", Tone::Danger),
        ];
        row()
            .children(sizes.into_iter().map(|(size, label, tone)| {
                Button::new(SharedString::from(format!("open-dialog-{label}")))
                    .label(label)
                    .theme(*theme)
                    .on_click(click(this, move |this, _, cx| {
                        this.dialog = Some((size, tone));
                        cx.notify();
                    }))
            }))
            .child(
                Button::new("open-sheet")
                    .label("Sheet")
                    .theme(*theme)
                    .on_click(click(this, |this, _, cx| {
                        this.sheet = true;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn confirms(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let link = self.link.clone();
        let edit = self.link.clone();
        let current = self.link_value.clone();
        row()
            .child(
                Button::new("open-confirm-delete")
                    .variant(Variant::GhostDanger)
                    .icon(IconName::TrashCan)
                    .label("Delete folder")
                    .theme(*theme)
                    .on_click(click(this, |this, _, cx| {
                        this.confirm = Some(Confirm::Delete);
                        this.error = None;
                        this.attempts = 0;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("open-confirm-send")
                    .label("Send without subject")
                    .theme(*theme)
                    .on_click(click(this, |this, _, cx| {
                        this.confirm = Some(Confirm::Send);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("open-link-insert")
                    .icon(IconName::Link)
                    .label("Insert link")
                    .theme(*theme)
                    .on_click(move |_, window, cx| {
                        link.update(cx, |link, cx| link.show(None, window, cx));
                    }),
            )
            .child(
                Button::new("open-link-edit")
                    .icon(IconName::Pencil)
                    .label("Edit link")
                    .theme(*theme)
                    .on_click(move |_, window, cx| {
                        let value = current
                            .clone()
                            .unwrap_or_else(|| "https://calloway.thelemail.test".into());
                        edit.update(cx, |link, cx| link.show(Some(value), window, cx));
                    }),
            )
            .child(caption(
                format!("Link: {}", self.link_value.as_deref().unwrap_or("none")),
                theme,
            ))
            .into_any_element()
    }

    fn menu_entries(&self, this: &WeakEntity<Self>) -> Vec<MenuEntry> {
        let pick = |label: &'static str| {
            let this = this.clone();
            move |_: &mut Window, cx: &mut App| {
                this.update(cx, |this, cx| this.picked(label, cx)).ok();
            }
        };
        let folders = FOLDERS
            .iter()
            .map(|folder| {
                let this = this.clone();
                MenuItem::new(*folder)
                    .icon(IconName::Folder)
                    .on_select(move |_, cx| {
                        this.update(cx, |this, cx| this.picked(format!("Moved to {folder}"), cx))
                            .ok();
                    })
                    .into()
            })
            .collect::<Vec<MenuEntry>>();
        let unread = {
            let this = this.clone();
            MenuItem::new("Unread first")
                .checkbox(if self.unread_first {
                    Check::On
                } else {
                    Check::Off
                })
                .on_select(move |_, cx| {
                    this.update(cx, |this, cx| {
                        this.unread_first = !this.unread_first;
                        cx.notify();
                    })
                    .ok();
                })
        };
        let threads = {
            let this = this.clone();
            MenuItem::new("Group conversations")
                .checkbox(self.threads)
                .on_select(move |_, cx| {
                    this.update(cx, |this, cx| {
                        this.threads = match this.threads {
                            Check::On => Check::Off,
                            Check::Off | Check::Mixed => Check::On,
                        };
                        cx.notify();
                    })
                    .ok();
                })
        };
        let mut entries: Vec<MenuEntry> = vec![
            MenuItem::new("Mark unread")
                .icon(IconName::EnvelopeOpen)
                .shortcut("⇧U")
                .on_select(pick("Mark unread"))
                .into(),
            MenuItem::new("Snooze")
                .icon(IconName::Clock)
                .meta("Tomorrow")
                .on_select(pick("Snooze"))
                .into(),
            MenuItem::new("Move to")
                .icon(IconName::FolderArrowDown)
                .submenu(folders)
                .into(),
            MenuEntry::Separator,
            MenuItem::new("View headers")
                .inset()
                .on_select(pick("View headers"))
                .into(),
            MenuItem::new("Print")
                .icon(IconName::Print)
                .shortcut("⌘P")
                .on_select(pick("Print"))
                .into(),
            MenuItem::new("Create event")
                .icon(IconName::CalendarPlus)
                .disabled(true)
                .into(),
            MenuEntry::Separator,
            unread.into(),
            threads.into(),
            MenuEntry::label("Sort by"),
        ];
        entries.extend(SORTS.iter().enumerate().map(|(ix, label)| {
            let this = this.clone();
            MenuItem::new(*label)
                .radio(self.sort == ix)
                .on_select(move |_, cx| {
                    this.update(cx, |this, cx| {
                        this.sort = ix;
                        cx.notify();
                    })
                    .ok();
                })
                .into()
        }));
        entries.push(MenuEntry::Separator);
        entries.push(
            MenuItem::new("Block sender")
                .icon(IconName::Ban)
                .on_select(pick("Block sender"))
                .into(),
        );
        entries.push(
            MenuItem::new("Report")
                .icon(IconName::ShieldExclamation)
                .danger()
                .on_select(pick("Report"))
                .into(),
        );
        entries
    }

    fn menus(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let theme = *theme;
        let snooze = this.clone();
        row()
            .child(
                DropdownMenu::new("gallery-more")
                    .width(236.0)
                    .theme(theme)
                    .entries(self.menu_entries(this))
                    .trigger(move |open, toggle, focus| {
                        Button::new("gallery-more-trigger")
                            .track_focus(&focus)
                            .icon(IconName::EllipsisVertical)
                            .label("More")
                            .expanded(open)
                            .tooltip("More actions")
                            .theme(theme)
                            .on_click(move |_, window, cx| toggle(window, cx))
                    }),
            )
            .child(
                Popover::new("gallery-snooze")
                    .label("Snooze until")
                    .theme(theme)
                    .trigger(move |open, toggle, focus| {
                        Button::new("gallery-snooze-trigger")
                            .track_focus(&focus)
                            .icon(IconName::Clock)
                            .label("Snooze")
                            .expanded(open)
                            .theme(theme)
                            .on_click(move |_, window, cx| toggle(window, cx))
                    })
                    .content(move |close| {
                        let options = ["Later today", "Tomorrow", "Next week"];
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .child(caption("SNOOZE UNTIL".to_string(), &theme))
                            .children(options.into_iter().map(|label| {
                                let close = close.clone();
                                let snooze = snooze.clone();
                                Button::new(SharedString::from(format!("snooze-{label}")))
                                    .label(label)
                                    .block()
                                    .theme(theme)
                                    .on_click(move |_, window, cx| {
                                        snooze
                                            .update(cx, |this, cx| {
                                                this.picked(format!("Snoozed: {label}"), cx)
                                            })
                                            .ok();
                                        close(window, cx);
                                    })
                            }))
                    }),
            )
            .into_any_element()
    }

    fn context(&self, theme: &Theme, this: &WeakEntity<Self>, window: &Window) -> AnyElement {
        let pick = |label: &'static str| {
            let this = this.clone();
            move |_: &mut Window, cx: &mut App| {
                this.update(cx, |this, cx| this.picked(label, cx)).ok();
            }
        };
        let focused = self.row_focus.is_focused(window) && window.last_input_was_keyboard();
        let target = div()
            .id("gallery-context-row")
            .track_focus(&self.row_focus.clone().tab_index(0))
            .role(Role::Row)
            .aria_label("Receipts")
            .flex()
            .items_center()
            .gap(px(10.0))
            .h(px(40.0))
            .px(px(12.0))
            .rounded(px(RADIUS_MD))
            .bg(theme.color(ColorToken::BgRaised))
            .border_1()
            .border_color(theme.color(if focused {
                ColorToken::FocusRing
            } else {
                ColorToken::BorderSoft
            }))
            .text_token(&T_SMALL, theme)
            .child(thelemail_ui::icons::Icon::hidden(IconName::Folder).size(16.0))
            .child("Receipts")
            .child(
                div()
                    .ml_auto()
                    .text_color(theme.color(ColorToken::FgFaint))
                    .child("Right-click, Shift+F10 or the menu key"),
            );
        ContextMenu::new("gallery-context", target)
            .width(260.0)
            .theme(*theme)
            .entries([
                MenuEntry::label("Folders / Receipts"),
                MenuItem::new("Add to favourites")
                    .icon(IconName::Star)
                    .on_select(pick("Favourite"))
                    .into(),
                MenuItem::new("New subfolder")
                    .icon(IconName::FolderPlus)
                    .on_select(pick("New subfolder"))
                    .into(),
                MenuItem::new("Expand all")
                    .icon(IconName::ListTree)
                    .on_select(pick("Expand all"))
                    .into(),
                MenuEntry::Separator,
                MenuItem::new("Rename")
                    .icon(IconName::Pencil)
                    .on_select(pick("Rename"))
                    .into(),
                MenuItem::new("Move up").inset().disabled(true).into(),
                MenuItem::new("Move down")
                    .inset()
                    .on_select(pick("Move down"))
                    .into(),
                MenuEntry::Separator,
                MenuItem::new("Delete folder")
                    .icon(IconName::TrashCan)
                    .danger()
                    .on_select(pick("Delete folder"))
                    .into(),
                MenuEntry::Separator,
                MenuItem::new("Manage")
                    .icon(IconName::Sliders)
                    .on_select(pick("Manage"))
                    .into(),
            ])
            .into_any_element()
    }

    fn tips(&self, theme: &Theme, window: &mut Window, cx: &mut App) -> AnyElement {
        let plain_focus = window
            .use_keyed_state("gallery-tip-focus", cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let plain = div()
            .id("gallery-tip-plain")
            .track_focus(&plain_focus.clone().tab_index(0))
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(RADIUS_MD))
            .border_1()
            .border_color(theme.color(ColorToken::BorderSoft))
            .text_token(&T_SMALL, theme)
            .child("Plain element")
            .tip(
                Tip::new("gallery-tip-plain-tip", "Tooltips work on any element").theme(*theme),
                Some(&plain_focus),
                window,
                cx,
            );
        row()
            .child(
                Button::new("tip-archive")
                    .ghost()
                    .size(Size::Icon)
                    .icon(IconName::BoxArchive)
                    .tooltip("Archive")
                    .shortcut("E")
                    .theme(*theme),
            )
            .child(
                Button::new("tip-reply")
                    .ghost()
                    .size(Size::Icon)
                    .icon(IconName::Reply)
                    .tooltip("Reply")
                    .shortcut("R")
                    .theme(*theme),
            )
            .child(
                Button::new("tip-delete")
                    .ghost()
                    .size(Size::Icon)
                    .icon(IconName::TrashCan)
                    .tooltip("Delete")
                    .shortcut("#")
                    .theme(*theme),
            )
            .child(plain)
            .into_any_element()
    }

    fn toasts(&self, theme: &Theme) -> AnyElement {
        let theme = *theme;
        let kinds: [(&'static str, ToastTone, &'static str); 3] = [
            ("Success", ToastTone::Success, "Settings saved"),
            ("Info", ToastTone::Info, "Loading older messages"),
            (
                "Error",
                ToastTone::Error,
                "Couldn't move the message. Try again.",
            ),
        ];
        row()
            .children(kinds.into_iter().map(move |(label, tone, text)| {
                Button::new(SharedString::from(format!("toast-{label}")))
                    .label(label)
                    .theme(theme)
                    .on_click(move |_, window, cx| {
                        Toasts::push(window, cx, Toast::new(tone, text));
                    })
            }))
            .child(
                Button::new("toast-undo")
                    .label("With undo")
                    .theme(theme)
                    .on_click(|_, window, cx| {
                        Toasts::push(
                            window,
                            cx,
                            Toast::success("Conversation archived").undo(|window, cx| {
                                Toasts::push(window, cx, Toast::info("Moved back to Inbox"));
                            }),
                        );
                    }),
            )
            .child(
                Button::new("toast-dismiss")
                    .label("Dismissible")
                    .theme(theme)
                    .on_click(|_, window, cx| {
                        Toasts::push(
                            window,
                            cx,
                            Toast::error("You're offline. Changes will sync later.")
                                .dismissible()
                                .sticky(),
                        );
                    }),
            )
            .child(
                Button::new("toast-burst")
                    .label("Stack of four")
                    .theme(theme)
                    .on_click(|_, window, cx| {
                        for text in ["First", "Second", "Third", "Fourth"] {
                            Toasts::push(window, cx, Toast::info(text));
                        }
                    }),
            )
            .into_any_element()
    }

    fn transitions(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        row()
            .child(
                Button::new("transition-switch")
                    .label("Switch account")
                    .theme(*theme)
                    .on_click(click(this, |this, window, cx| {
                        this.transition(Transition::switch(ARRIVING), window, cx)
                    })),
            )
            .child(
                Button::new("transition-signout")
                    .variant(Variant::GhostDanger)
                    .label("Sign out")
                    .theme(*theme)
                    .on_click(click(this, |this, window, cx| {
                        this.transition(Transition::sign_out(), window, cx)
                    })),
            )
            .into_any_element()
    }

    fn native(
        &self,
        theme: &Theme,
        this: &WeakEntity<Self>,
        covered: bool,
        surface: Option<AnyElement>,
    ) -> AnyElement {
        let theme = *theme;
        div()
            .flex()
            .gap(px(12.0))
            .items_start()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        DropdownMenu::new("gallery-near-surface")
                            .width(236.0)
                            .theme(theme)
                            .entries(self.menu_entries(this))
                            .trigger(move |open, toggle, focus| {
                                Button::new("gallery-near-surface-trigger")
                                    .track_focus(&focus)
                                    .label("Menu next to it")
                                    .expanded(open)
                                    .theme(theme)
                                    .on_click(move |_, window, cx| toggle(window, cx))
                            }),
                    )
                    .child(caption(
                        format!("covered: {}", if covered { "yes" } else { "no" }),
                        &theme,
                    )),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .h(px(220.0))
                    .rounded(px(RADIUS_MD))
                    .border_1()
                    .border_color(theme.color(ColorToken::BorderStrong))
                    .bg(theme.color(if covered {
                        ColorToken::Warning100
                    } else {
                        ColorToken::SurfaceMuted
                    }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(paragraph(
                        "Stands in for the reader webview. Overlays keep off it or mark it covered.",
                        &theme,
                    ))
                    .children(surface),
            )
            .into_any_element()
    }

    fn overlays(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let theme = *theme;
        let (size, tone) = self.dialog.unwrap_or((DialogSize::Md, Tone::Brand));
        let dialog = Dialog::new("gallery-dialog")
            .open(self.dialog.is_some())
            .size(size)
            .tone(tone)
            .icon(match tone {
                Tone::Danger => IconName::TriangleExclamation,
                Tone::Warning => IconName::CircleExclamation,
                Tone::Success => IconName::CircleCheck,
                Tone::Neutral => IconName::CircleInfo,
                Tone::Brand => IconName::ShieldCheck,
            })
            .eyebrow("Security")
            .title("Turn on two-step sign-in")
            .description("You'll confirm new sign-ins with a code from an authenticator app.")
            .theme(theme)
            .on_close(act(this, |this, _, cx| {
                this.dialog = None;
                cx.notify();
            }))
            .child(paragraph(
                "Keep your recovery phrase somewhere safe. It's the only way back in if you lose this device.",
                &theme,
            ))
            .footer(
                Button::new("gallery-dialog-cancel")
                    .label("Not now")
                    .theme(theme)
                    .on_click(click(this, |this, _, cx| {
                        this.dialog = None;
                        cx.notify();
                    })),
            )
            .footer(
                Button::new("gallery-dialog-ok")
                    .primary()
                    .label("Continue")
                    .theme(theme)
                    .on_click(click(this, |this, _, cx| {
                        this.dialog = None;
                        this.picked("Two-step sign-in started", cx);
                    })),
            );
        let sheet = Sheet::new("gallery-sheet")
            .open(self.sheet)
            .title("Move 3 conversations")
            .subtitle("Pick a folder")
            .theme(theme)
            .on_close(act(this, |this, _, cx| {
                this.sheet = false;
                cx.notify();
            }))
            .children(FOLDERS.iter().map(|folder| {
                div()
                    .py(px(8.0))
                    .text_token(&T_SMALL, &theme)
                    .child(*folder)
            }))
            .footer(
                Button::new("gallery-sheet-cancel")
                    .label("Cancel")
                    .theme(theme)
                    .on_click(click(this, |this, _, cx| {
                        this.sheet = false;
                        cx.notify();
                    })),
            )
            .footer(
                Button::new("gallery-sheet-move")
                    .primary()
                    .label("Move")
                    .theme(theme)
                    .on_click(click(this, |this, _, cx| {
                        this.sheet = false;
                        this.picked("Moved 3 conversations", cx);
                    })),
            );
        let delete = ConfirmDialog::new(
            "gallery-confirm-delete",
            "Delete “Receipts”?",
            "Delete folder",
        )
        .open(self.confirm == Some(Confirm::Delete))
        .danger()
        .icon(IconName::TrashCan)
        .subtitle("12 conversations move to Trash")
        .busy(self.busy)
        .error(self.error.clone())
        .theme(theme)
        .child(paragraph(
            "Subfolders are deleted too. You can't undo this.",
            &theme,
        ))
        .on_confirm(act(this, |this, _, cx| this.confirm(cx)))
        .on_close(act(this, |this, _, cx| {
            this.confirm = None;
            this.error = None;
            cx.notify();
        }));
        let send = ConfirmDialog::new("gallery-confirm-send", "Send without a subject?", "Send")
            .open(self.confirm == Some(Confirm::Send))
            .icon(IconName::PaperPlane)
            .cancel_label("Add subject")
            .theme(theme)
            .on_confirm(act(this, |this, _, cx| {
                this.confirm = None;
                this.picked("Sent", cx);
            }))
            .on_close(act(this, |this, _, cx| {
                this.confirm = None;
                cx.notify();
            }));
        div()
            .child(dialog)
            .child(sheet)
            .child(delete)
            .child(send)
            .child(self.link.clone())
            .child(
                AccountTransition::new("gallery-transition", self.transition.clone()).theme(theme),
            )
            .into_any_element()
    }
}
