use gpui_kit::component::input::InputState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext, Context, ElementId, Entity, IntoElement, ParentElement, Render,
    SharedString, Styled, WeakEntity, Window, div, px,
};
use thelemail_ui::button::{Button, Size};
use thelemail_ui::choice::{
    Checkbox, Chip, ChipRow, ChipSize, Mark, OptionTiles, Radio, RadioGroup, RadioItem,
    SegmentOption, Segmented, SegmentedSize, Switch, SwitchSize, Tile,
};
use thelemail_ui::field::{FieldSize, SearchField};
use thelemail_ui::icons::IconName;
use thelemail_ui::theme::{ActiveTokens, Appearance, Theme};
use thelemail_ui::tokens::{ColorToken, RADIUS_LG};

use crate::gallery::{caption, frame, section};
use crate::language::Language;
use crate::router::GalleryTab;

const NARROW: f32 = 280.0;
const TILES_WIDTH: f32 = 560.0;
const RECIPIENTS: [&str; 4] = [
    "Orla Finch",
    "Juniper Calloway",
    "Bram Calloway",
    "accounts@calloway.thelemail.test",
];
const FILTERS: [&str; 7] = [
    "Unread",
    "Starred",
    "Attachments",
    "From people",
    "This week",
    "Newsletters",
    "Encrypted",
];

pub struct ChoiceGallery {
    view: Option<SharedString>,
    range: Option<SharedString>,
    layout: Option<SharedString>,
    rsvp: Option<SharedString>,
    filters: [bool; FILTERS.len()],
    recipients: Vec<SharedString>,
    marks: [Mark; 3],
    agree: bool,
    radio: Option<SharedString>,
    lone_radio: bool,
    switches: [bool; 3],
    plan: Option<SharedString>,
    seats: Option<SharedString>,
    searches: [Entity<InputState>; 3],
}

impl ChoiceGallery {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut search = |placeholder: &'static str| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .clean_on_escape()
            })
        };
        let searches = [
            search("Search mail"),
            search("Search mail"),
            search("Search mail"),
        ];
        Self {
            view: Some("all".into()),
            range: Some("week".into()),
            layout: None,
            rsvp: Some("yes".into()),
            filters: [true, false, false, false, true, false, false],
            recipients: RECIPIENTS.iter().map(|r| SharedString::from(*r)).collect(),
            marks: [Mark::Off, Mark::On, Mark::Mixed],
            agree: false,
            radio: Some("forward".into()),
            lone_radio: false,
            switches: [true, false, true],
            plan: Some("household".into()),
            seats: None,
            searches,
        }
    }
}

fn side(theme: &Theme) -> &'static str {
    match theme.appearance {
        Appearance::Light => "light",
        Appearance::Dark => "dark",
    }
}

fn id(theme: &Theme, name: impl Into<SharedString>) -> ElementId {
    ElementId::Name(format!("{}-choice-{}", side(theme), name.into()).into())
}

fn row() -> gpui_kit::Div {
    div().flex().flex_wrap().items_center().gap(px(12.0))
}

fn labelled(label: &str, body: impl IntoElement, theme: &Theme) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(caption(label.to_string(), theme))
        .child(body)
}

fn set<T: 'static>(
    this: &WeakEntity<ChoiceGallery>,
    f: impl Fn(&mut ChoiceGallery, T) + 'static,
) -> impl Fn(T, &mut Window, &mut App) + 'static {
    let this = this.clone();
    move |value, _, cx| {
        this.update(cx, |this, cx| {
            f(this, value);
            cx.notify();
        })
        .ok();
    }
}

impl Render for ChoiceGallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = *cx.tokens();
        let language = *cx.global::<Language>();
        let this = cx.entity().downgrade();
        frame(GalleryTab::Choices, &current, language, |theme| {
            self.column(theme, &this, theme.appearance == current.appearance)
        })
    }
}

impl ChoiceGallery {
    fn column(&self, theme: &Theme, this: &WeakEntity<Self>, live: bool) -> AnyElement {
        let theme = *theme;
        div()
            .flex()
            .flex_col()
            .gap(px(28.0))
            .child(section("Segmented", self.segmented(&theme, this), &theme))
            .child(section("Chips", self.chips(&theme, this), &theme))
            .child(section("Chip row", self.chip_row(&theme, this), &theme))
            .child(section("Checkbox", self.checkboxes(&theme, this), &theme))
            .child(section("Radio", self.radios(&theme, this), &theme))
            .child(section("Switch", self.switches(&theme, this), &theme))
            .child(section("Search field", self.search(&theme, live), &theme))
            .child(section("Option tiles", self.tiles(&theme, this), &theme))
            .into_any_element()
    }

    fn segmented(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let views = || {
            vec![
                SegmentOption::new("all", "All"),
                SegmentOption::new("unread", "Unread"),
                SegmentOption::new("starred", "Starred"),
            ]
        };
        let ranges = vec![
            SegmentOption::new("day", "Day"),
            SegmentOption::new("week", "Week"),
            SegmentOption::new("month", "Month").disabled(true),
            SegmentOption::new("agenda", "Agenda"),
        ];
        let layouts = vec![
            SegmentOption::new("list", "List")
                .icon(IconName::ListUl)
                .hide_label(),
            SegmentOption::new("calendar", "Calendar")
                .icon(IconName::CalendarDays)
                .hide_label(),
            SegmentOption::new("inbox", "Inbox")
                .icon(IconName::Inbox)
                .hide_label(),
        ];
        let rsvp = |value: &'static str, label: &'static str| {
            let on = self.rsvp.as_deref() == Some(value);
            let this = this.clone();
            Button::new(id(theme, format!("rsvp-{value}")))
                .size(Size::Sm)
                .label(label)
                .pressed(on)
                .when(on, |button| button.tonal())
                .theme(*theme)
                .on_click(move |_, _, cx| {
                    this.update(cx, |this, cx| {
                        this.rsvp = Some(value.into());
                        cx.notify();
                    })
                    .ok();
                })
        };
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                row()
                    .child(labelled(
                        "sm",
                        Segmented::new(id(theme, "view-sm"), views())
                            .value(self.view.clone())
                            .label("Show")
                            .theme(*theme)
                            .on_change(set(this, |g, v| g.view = Some(v))),
                        theme,
                    ))
                    .child(labelled(
                        "md",
                        Segmented::new(id(theme, "view-md"), views())
                            .size(SegmentedSize::Md)
                            .value(self.view.clone())
                            .label("Show")
                            .theme(*theme)
                            .on_change(set(this, |g, v| g.view = Some(v))),
                        theme,
                    )),
            )
            .child(
                row()
                    .child(labelled(
                        "disabled item",
                        Segmented::new(id(theme, "range"), ranges)
                            .value(self.range.clone())
                            .label("Range")
                            .theme(*theme)
                            .on_change(set(this, |g, v| g.range = Some(v))),
                        theme,
                    ))
                    .child(labelled(
                        "icons, empty until picked",
                        Segmented::new(id(theme, "layout"), layouts)
                            .value(self.layout.clone())
                            .label("Layout")
                            .theme(*theme)
                            .on_change(set(this, |g, v| g.layout = Some(v))),
                        theme,
                    ))
                    .child(labelled(
                        "disabled",
                        Segmented::new(id(theme, "off"), views())
                            .value(Some("unread"))
                            .disabled(true)
                            .theme(*theme),
                        theme,
                    )),
            )
            .child(labelled(
                "block",
                Segmented::new(id(theme, "block"), views())
                    .block()
                    .size(SegmentedSize::Md)
                    .value(self.view.clone())
                    .label("Show")
                    .theme(*theme)
                    .on_change(set(this, |g, v| g.view = Some(v))),
                theme,
            ))
            .child(labelled(
                "a choice that sends is buttons, never segmented",
                row()
                    .gap(px(8.0))
                    .child(rsvp("yes", "Yes"))
                    .child(rsvp("maybe", "Maybe"))
                    .child(rsvp("no", "No")),
                theme,
            ))
            .into_any_element()
    }

    fn chips(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let toggle = |ix: usize, size: ChipSize| {
            let name = FILTERS[ix];
            Chip::new(id(theme, format!("filter-{ix}-{size:?}")), name)
                .size(size)
                .pressed(self.filters[ix], set(this, move |g, on| g.filters[ix] = on))
                .theme(*theme)
        };
        let avatar = |initials: &'static str| {
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(22.0))
                .rounded_full()
                .bg(theme.color(ColorToken::Brass100))
                .text_color(theme.color(ColorToken::Brass700))
                .text_size(px(10.0))
                .child(initials)
        };
        let removable = self.recipients.iter().enumerate().map(|(ix, name)| {
            let name = name.clone();
            let removed = name.clone();
            let this = this.clone();
            let chip = Chip::new(id(theme, format!("to-{ix}")), name.clone())
                .on_remove(format!("Remove {name}"), move |_, cx| {
                    let removed = removed.clone();
                    this.update(cx, |this, cx| {
                        this.recipients.retain(|r| *r != removed);
                        cx.notify();
                    })
                    .ok();
                })
                .theme(*theme);
            if ix == 0 {
                chip.leading(avatar("OF"))
            } else {
                chip
            }
        });
        let this_reset = this.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(labelled(
                "toggle, md",
                row()
                    .gap(px(6.0))
                    .children((0..3).map(|ix| toggle(ix, ChipSize::Md))),
                theme,
            ))
            .child(labelled(
                "toggle, sm",
                row()
                    .gap(px(6.0))
                    .children((0..3).map(|ix| toggle(ix, ChipSize::Sm))),
                theme,
            ))
            .child(labelled(
                "removable",
                row().gap(px(6.0)).children(removable).child(
                    Button::new(id(theme, "reset-recipients"))
                        .size(Size::Sm)
                        .ghost()
                        .label("Reset")
                        .theme(*theme)
                        .on_click(move |_, _, cx| {
                            this_reset
                                .update(cx, |this, cx| {
                                    this.recipients =
                                        RECIPIENTS.iter().map(|r| SharedString::from(*r)).collect();
                                    cx.notify();
                                })
                                .ok();
                        }),
                ),
                theme,
            ))
            .child(labelled(
                "link, action, static, disabled",
                row()
                    .gap(px(6.0))
                    .child(
                        Chip::new(id(theme, "link"), "calloway.thelemail.test")
                            .link(|_, _| {})
                            .theme(*theme),
                    )
                    .child(
                        Chip::new(id(theme, "action"), "Add label")
                            .on_click(|_, _| {})
                            .theme(*theme),
                    )
                    .child(Chip::new(id(theme, "static"), "Work").theme(*theme))
                    .child(
                        Chip::new(id(theme, "disabled"), "Archived")
                            .pressed(false, |_, _, _| {})
                            .disabled(true)
                            .theme(*theme),
                    )
                    .child(
                        div().w(px(160.0)).child(
                            Chip::new(id(theme, "long"), "A label long enough to need truncating")
                                .theme(*theme),
                        ),
                    ),
                theme,
            ))
            .into_any_element()
    }

    fn chip_row(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        div()
            .w(px(NARROW))
            .p(px(10.0))
            .rounded(px(RADIUS_LG))
            .bg(theme.color(ColorToken::BgRaised))
            .child(
                ChipRow::new(id(theme, "filters-row"))
                    .fade(ColorToken::BgRaised)
                    .label("Filters")
                    .theme(*theme)
                    .children(FILTERS.iter().enumerate().map(|(ix, name)| {
                        Chip::new(id(theme, format!("row-{ix}")), *name)
                            .pressed(self.filters[ix], set(this, move |g, on| g.filters[ix] = on))
                            .theme(*theme)
                    })),
            )
            .into_any_element()
    }

    fn checkboxes(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let names = ["Off", "On", "Mixed"];
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(row().gap(px(20.0)).children((0..3).map(|ix| {
                Checkbox::new(id(theme, format!("check-{ix}")))
                    .mark(self.marks[ix])
                    .accessibility_label(names[ix])
                    .theme(*theme)
                    .on_change(set(this, move |g, mark| g.marks[ix] = mark))
            })))
            .child(
                row()
                    .gap(px(20.0))
                    .child(
                        Checkbox::new(id(theme, "check-disabled"))
                            .accessibility_label("Disabled")
                            .disabled(true)
                            .theme(*theme),
                    )
                    .child(
                        Checkbox::new(id(theme, "check-disabled-on"))
                            .checked(true)
                            .accessibility_label("Disabled, on")
                            .disabled(true)
                            .theme(*theme),
                    )
                    .child(
                        Checkbox::new(id(theme, "check-invalid"))
                            .checked(self.agree)
                            .accessibility_label("Invalid")
                            .invalid(!self.agree)
                            .theme(*theme)
                            .on_change(set(this, |g, mark| g.agree = mark == Mark::On)),
                    ),
            )
            .child(
                Checkbox::new(id(theme, "check-labelled"))
                    .checked(self.agree)
                    .label("I have saved my recovery phrase")
                    .invalid(!self.agree)
                    .theme(*theme)
                    .on_change(set(this, |g, mark| g.agree = mark == Mark::On)),
            )
            .into_any_element()
    }

    fn radios(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let items = vec![
            RadioItem::new("forward", "Forward to the new address"),
            RadioItem::new("bounce", "Return to sender"),
            RadioItem::new("drop", "Discard silently").disabled(true),
            RadioItem::new("keep", "Keep in this mailbox"),
        ];
        let lone = this.clone();
        row()
            .items_start()
            .gap(px(40.0))
            .child(
                RadioGroup::new(id(theme, "radio-group"), items)
                    .value(self.radio.clone())
                    .label("When mail arrives for a removed address")
                    .theme(*theme)
                    .on_change(set(this, |g, v| g.radio = Some(v))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(
                        Radio::new(id(theme, "radio-lone"))
                            .checked(self.lone_radio)
                            .label("Standalone")
                            .theme(*theme)
                            .on_select(move |_, cx| {
                                lone.update(cx, |this, cx| {
                                    this.lone_radio = true;
                                    cx.notify();
                                })
                                .ok();
                            }),
                    )
                    .child(
                        Radio::new(id(theme, "radio-invalid"))
                            .label("Invalid")
                            .invalid(true)
                            .theme(*theme),
                    )
                    .child(
                        Radio::new(id(theme, "radio-disabled"))
                            .checked(true)
                            .label("Disabled")
                            .disabled(true)
                            .theme(*theme),
                    ),
            )
            .into_any_element()
    }

    fn switches(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                row()
                    .gap(px(20.0))
                    .child(
                        Switch::new(id(theme, "switch-md"))
                            .checked(self.switches[0])
                            .accessibility_label("Medium")
                            .theme(*theme)
                            .on_change(set(this, |g, on| g.switches[0] = on)),
                    )
                    .child(
                        Switch::new(id(theme, "switch-sm"))
                            .size(SwitchSize::Sm)
                            .checked(self.switches[1])
                            .accessibility_label("Small")
                            .theme(*theme)
                            .on_change(set(this, |g, on| g.switches[1] = on)),
                    )
                    .child(
                        Switch::new(id(theme, "switch-off-disabled"))
                            .accessibility_label("Disabled")
                            .disabled(true)
                            .theme(*theme),
                    )
                    .child(
                        Switch::new(id(theme, "switch-on-disabled"))
                            .checked(true)
                            .accessibility_label("Disabled, on")
                            .disabled(true)
                            .theme(*theme),
                    ),
            )
            .child(
                Switch::new(id(theme, "switch-labelled"))
                    .checked(self.switches[2])
                    .label("Show remote images")
                    .theme(*theme)
                    .on_change(set(this, |g, on| g.switches[2] = on)),
            )
            .into_any_element()
    }

    fn search(&self, theme: &Theme, live: bool) -> AnyElement {
        if !live {
            return caption("Shown in the current theme only".into(), theme).into_any_element();
        }
        let sizes = [
            (FieldSize::Sm, "sm"),
            (FieldSize::Md, "md, ⌘K"),
            (FieldSize::Lg, "lg"),
        ];
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .w(px(360.0))
            .children(sizes.into_iter().enumerate().map(|(ix, (size, name))| {
                labelled(
                    name,
                    SearchField::new(id(theme, format!("search-{ix}")), &self.searches[ix])
                        .size(size)
                        .label("Search mail")
                        .theme(*theme)
                        .when(ix == 1, |field| field.shortcut("⌘K")),
                    theme,
                )
            }))
            .into_any_element()
    }

    fn tiles(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let plans = vec![
            Tile::new("personal", "Personal")
                .icon(IconName::User)
                .description("One person, every address on your own domain."),
            Tile::new("household", "Household")
                .icon(IconName::UserGroup)
                .badge("Popular")
                .description("Up to six people sharing a domain."),
            Tile::new("team", "Team")
                .icon(IconName::Users)
                .description("Seats, shared addresses and admin controls."),
            Tile::new("archive", "Archive only")
                .icon(IconName::Lock)
                .description("Keep old mail readable without receiving new mail.")
                .disabled(true),
        ];
        let seats = vec![
            Tile::new("one", "One seat").description("Just you."),
            Tile::new("five", "Five seats").description("A small team."),
        ];
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(labelled(
                "auto-fit, 170 minimum",
                div().max_w(px(TILES_WIDTH)).child(
                    OptionTiles::new(id(theme, "plans"), plans)
                        .value(self.plan.clone())
                        .label("Plan")
                        .theme(*theme)
                        .on_change(set(this, |g, v| g.plan = Some(v))),
                ),
                theme,
            ))
            .child(labelled(
                "two columns, square mark, empty",
                div().max_w(px(TILES_WIDTH)).child(
                    OptionTiles::new(id(theme, "seats"), seats)
                        .columns(2)
                        .square_mark()
                        .value(self.seats.clone())
                        .label("Seats")
                        .theme(*theme)
                        .on_change(set(this, |g, v| g.seats = Some(v))),
                ),
                theme,
            ))
            .into_any_element()
    }
}
