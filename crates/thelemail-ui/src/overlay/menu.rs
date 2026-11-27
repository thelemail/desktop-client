mod nav;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::base::{Align, LayoutDirection, Placement};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Bounds, ElementId, Entity, FocusHandle, FontWeight, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels, Point, RenderOnce, Role,
    SharedString, StatefulInteractiveElement, Styled, Toggled, Window, div, px, relative,
};

use self::nav::{Sideways, Typeahead, find, sideways};
use super::anchor::{self, Place};
use super::dialog::Action;
use super::layer::{self, Layer};
use super::popover::surface_colors;
use super::{
    CONTEXT_TARGET, MENU_CONTEXT, MenuActivate, MenuBack, MenuExit, MenuFirst, MenuLast, MenuLeft,
    MenuNext, MenuPrev, MenuRight, OpenContextMenu, focus,
};
use crate::choice::roving::{Nav, step};
use crate::icons::{IconName, glyph};
use crate::motion::{MENU, Moving as _};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_MONO, FONT_SANS, RADIUS_MENU, ShadowToken};

const MIN_WIDTH: f32 = 200.0;
const PAD: f32 = 4.0;
const ITEM_HEIGHT: f32 = 34.0;
const ITEM_PAD: f32 = 10.0;
const ITEM_RADIUS: f32 = 5.0;
const ITEM_GAP: f32 = 10.0;
const ICON: f32 = 16.0;
const INSET: f32 = 36.0;
const CHECK_ROOM: f32 = 34.0;
const OFFSET: f32 = 4.0;
const MAX_HEIGHT: f32 = 0.7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Off,
    On,
    Mixed,
}

#[derive(Clone)]
enum Kind {
    Action,
    Checkbox(Check),
    Radio(bool),
    Submenu(Rc<Vec<MenuEntry>>),
}

#[derive(Clone)]
pub struct MenuItem {
    label: SharedString,
    icon: Option<IconName>,
    shortcut: Option<SharedString>,
    meta: Option<SharedString>,
    danger: bool,
    disabled: bool,
    inset: bool,
    keep_open: bool,
    kind: Kind,
    on_select: Option<Action>,
}

impl MenuItem {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            shortcut: None,
            meta: None,
            danger: false,
            disabled: false,
            inset: false,
            keep_open: false,
            kind: Kind::Action,
            on_select: None,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    pub fn meta(mut self, text: impl Into<SharedString>) -> Self {
        self.meta = Some(text.into());
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn inset(mut self) -> Self {
        self.inset = true;
        self
    }

    pub fn keep_open(mut self) -> Self {
        self.keep_open = true;
        self
    }

    pub fn checkbox(mut self, state: Check) -> Self {
        self.kind = Kind::Checkbox(state);
        self
    }

    pub fn radio(mut self, on: bool) -> Self {
        self.kind = Kind::Radio(on);
        self
    }

    pub fn submenu(mut self, entries: Vec<MenuEntry>) -> Self {
        self.kind = Kind::Submenu(Rc::new(entries));
        self
    }

    pub fn on_select(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    fn submenu_entries(&self) -> Option<Rc<Vec<MenuEntry>>> {
        match &self.kind {
            Kind::Submenu(entries) if !self.disabled => Some(entries.clone()),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub enum MenuEntry {
    Item(MenuItem),
    Separator,
    Label(SharedString),
}

impl From<MenuItem> for MenuEntry {
    fn from(item: MenuItem) -> Self {
        Self::Item(item)
    }
}

impl MenuEntry {
    pub fn label(text: impl Into<SharedString>) -> Self {
        Self::Label(text.into())
    }

    fn item(&self) -> Option<&MenuItem> {
        match self {
            Self::Item(item) => Some(item),
            _ => None,
        }
    }

    fn enabled(&self) -> bool {
        self.item().is_some_and(|item| !item.disabled)
    }
}

#[derive(Default)]
struct MenuState {
    open: bool,
    at: Option<Point<Pixels>>,
    path: Vec<Option<usize>>,
    typed: Typeahead,
}

impl MenuState {
    fn show(&mut self, at: Option<Point<Pixels>>) {
        self.open = true;
        self.at = at;
        self.path = vec![None];
        self.typed.reset();
    }

    fn hide(&mut self) {
        self.open = false;
        self.path.clear();
        self.typed.reset();
    }
}

fn level_entries(
    root: &Rc<Vec<MenuEntry>>,
    path: &[Option<usize>],
    level: usize,
) -> Rc<Vec<MenuEntry>> {
    let mut entries = root.clone();
    for step in path.iter().take(level) {
        let next = step
            .and_then(|ix| entries.get(ix))
            .and_then(MenuEntry::item)
            .and_then(MenuItem::submenu_entries);
        match next {
            Some(next) => entries = next,
            None => break,
        }
    }
    entries
}

fn enabled(entries: &[MenuEntry]) -> Vec<bool> {
    entries.iter().map(MenuEntry::enabled).collect()
}

#[derive(Clone)]
struct Ctx {
    id: ElementId,
    state: Entity<MenuState>,
    entries: Rc<Vec<MenuEntry>>,
    hits: Rc<Vec<Rc<Cell<Bounds<Pixels>>>>>,
    trigger: Rc<Cell<Bounds<Pixels>>>,
    theme: Theme,
    width: Option<f32>,
}

const MAX_DEPTH: usize = 6;

impl Ctx {
    fn deepest(&self, cx: &App) -> (usize, Rc<Vec<MenuEntry>>) {
        let state = self.state.read(cx);
        let level = state.path.len().saturating_sub(1);
        (level, level_entries(&self.entries, &state.path, level))
    }

    fn close(&self, window: &mut Window, cx: &mut App) {
        self.state.update(cx, |state, _| state.hide());
        window.refresh();
    }

    fn mv(&self, nav: Nav, window: &mut Window, cx: &mut App) {
        let (level, entries) = self.deepest(cx);
        let enabled = enabled(&entries);
        self.state.update(cx, |state, _| {
            let current = state.path.get(level).copied().flatten();
            if let Some(slot) = state.path.get_mut(level) {
                *slot = step(current, nav, &enabled, true);
            }
            state.typed.reset();
        });
        window.refresh();
    }

    fn open_sub(&self, window: &mut Window, cx: &mut App) -> bool {
        let (level, entries) = self.deepest(cx);
        let highlighted = self.state.read(cx).path.get(level).copied().flatten();
        let Some(sub) = highlighted
            .and_then(|ix| entries.get(ix))
            .and_then(MenuEntry::item)
            .and_then(MenuItem::submenu_entries)
        else {
            return false;
        };
        if level + 1 >= MAX_DEPTH {
            return false;
        }
        let first = step(None, Nav::First, &enabled(&sub), true);
        self.state.update(cx, |state, _| state.path.push(first));
        window.refresh();
        true
    }

    fn close_sub(&self, window: &mut Window, cx: &mut App) -> bool {
        let closed = self.state.update(cx, |state, _| {
            if state.path.len() > 1 {
                state.path.pop();
                true
            } else {
                false
            }
        });
        if closed {
            window.refresh();
        }
        closed
    }

    fn sideways(&self, right: bool, window: &mut Window, cx: &mut App) {
        let rtl = LayoutDirection::global(cx).is_rtl();
        match sideways(right, rtl) {
            Sideways::Open => {
                self.open_sub(window, cx);
            }
            Sideways::Close => {
                self.close_sub(window, cx);
            }
        }
    }

    fn activate(&self, window: &mut Window, cx: &mut App) {
        let (level, entries) = self.deepest(cx);
        let highlighted = self.state.read(cx).path.get(level).copied().flatten();
        let Some(item) = highlighted
            .and_then(|ix| entries.get(ix))
            .and_then(MenuEntry::item)
            .cloned()
        else {
            return;
        };
        self.select(&item, window, cx);
    }

    fn select(&self, item: &MenuItem, window: &mut Window, cx: &mut App) {
        if item.disabled {
            return;
        }
        if item.submenu_entries().is_some() {
            self.open_sub(window, cx);
            return;
        }
        if !item.keep_open {
            self.close(window, cx);
        }
        if let Some(handler) = item.on_select.as_ref() {
            handler(window, cx);
        }
        window.refresh();
    }

    fn typeahead(&self, event: &KeyDownEvent, window: &mut Window, cx: &mut App) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.control || keystroke.modifiers.platform || keystroke.modifiers.alt {
            return;
        }
        let Some(typed) = keystroke.key_char.clone() else {
            return;
        };
        let now = Instant::now();
        if typed.chars().count() != 1
            || (typed.trim().is_empty() && self.state.read(cx).typed.is_empty(now))
        {
            return;
        }
        let (level, entries) = self.deepest(cx);
        let labels: Vec<Option<&str>> = entries
            .iter()
            .map(|entry| {
                entry
                    .item()
                    .filter(|item| !item.disabled)
                    .map(|item| item.label.as_ref())
            })
            .collect();
        let found = self.state.update(cx, |state, _| {
            let current = state.path.get(level).copied().flatten();
            let query = state.typed.push(&typed, now).to_string();
            let found = find(&labels, current, &query);
            if let (Some(ix), Some(slot)) = (found, state.path.get_mut(level)) {
                *slot = Some(ix);
            }
            found
        });
        if found.is_some() {
            window.refresh();
        }
        cx.stop_propagation();
    }

    fn hover(&self, level: usize, ix: usize, window: &mut Window, cx: &mut App) {
        let entries = level_entries(&self.entries, &self.state.read(cx).path, level);
        let sub = entries
            .get(ix)
            .and_then(MenuEntry::item)
            .and_then(MenuItem::submenu_entries)
            .is_some();
        let changed = self.state.update(cx, |state, _| {
            let before = state.path.clone();
            state.path.truncate(level + 1);
            if let Some(slot) = state.path.get_mut(level) {
                *slot = Some(ix);
            }
            if sub && level + 1 < MAX_DEPTH {
                state.path.push(None);
            }
            before != state.path
        });
        if changed {
            window.refresh();
        }
    }

    fn inside(&self, point: Point<Pixels>, levels: usize) -> bool {
        self.trigger.get().contains(&point)
            || self
                .hits
                .iter()
                .take(levels)
                .any(|bounds| bounds.get().contains(&point))
    }
}

fn hits(id: &ElementId, window: &mut Window, cx: &mut App) -> Rc<Vec<Rc<Cell<Bounds<Pixels>>>>> {
    window
        .use_keyed_state(
            ElementId::NamedChild(id.clone().into(), "level-bounds".into()),
            cx,
            |_, _| {
                Rc::new(
                    (0..MAX_DEPTH)
                        .map(|_| Rc::new(Cell::new(Bounds::default())))
                        .collect::<Vec<_>>(),
                )
            },
        )
        .read(cx)
        .clone()
}

fn item_slot(
    id: &ElementId,
    level: usize,
    ix: usize,
    window: &mut Window,
    cx: &mut App,
) -> Rc<Cell<Bounds<Pixels>>> {
    let key = ElementId::NamedChild(
        id.clone().into(),
        SharedString::from(format!("item-{level}-{ix}")),
    );
    window
        .use_keyed_state(key, cx, |_, _| Rc::new(Cell::new(Bounds::default())))
        .read(cx)
        .clone()
}

fn level_id(id: &ElementId, level: usize) -> ElementId {
    ElementId::NamedChild(
        id.clone().into(),
        SharedString::from(format!("level-{level}")),
    )
}

fn item_row(
    ctx: &Ctx,
    level: usize,
    ix: usize,
    item: &MenuItem,
    highlighted: bool,
    expanded: bool,
    slot: Option<Rc<Cell<Bounds<Pixels>>>>,
) -> AnyElement {
    let theme = &ctx.theme;
    let fg = if item.danger {
        theme.color(ColorToken::Danger700)
    } else if highlighted {
        theme.color(ColorToken::Pine700)
    } else {
        theme.color(ColorToken::Fg)
    };
    let icon_fg = if item.danger {
        theme.color(ColorToken::Danger700)
    } else if highlighted {
        theme.color(ColorToken::Pine700)
    } else {
        theme.color(ColorToken::FgMuted)
    };
    let bg = highlighted.then(|| {
        theme.color(if item.danger {
            ColorToken::Danger100
        } else {
            ColorToken::Pine100
        })
    });
    let checked = match item.kind {
        Kind::Checkbox(Check::On) | Kind::Radio(true) => Some(IconName::Check),
        Kind::Checkbox(Check::Mixed) => Some(IconName::Minus),
        _ => None,
    };
    let check_kind = matches!(item.kind, Kind::Checkbox(_) | Kind::Radio(_));
    let strong = check_kind && checked.is_some();
    let role = match item.kind {
        Kind::Checkbox(_) => Role::MenuItemCheckBox,
        Kind::Radio(_) => Role::MenuItemRadio,
        _ => Role::MenuItem,
    };
    let toggled = match item.kind {
        Kind::Checkbox(Check::On) | Kind::Radio(true) => Some(Toggled::True),
        Kind::Checkbox(Check::Mixed) => Some(Toggled::Mixed),
        Kind::Checkbox(Check::Off) | Kind::Radio(false) => Some(Toggled::False),
        _ => None,
    };
    let submenu = matches!(item.kind, Kind::Submenu(_));
    let hover_ctx = ctx.clone();
    let click_ctx = ctx.clone();
    let clicked = item.clone();
    let row_id = ElementId::NamedChild(
        level_id(&ctx.id, level).into(),
        SharedString::from(format!("row-{ix}")),
    );
    div()
        .id(row_id)
        .relative()
        .flex()
        .items_center()
        .gap(px(ITEM_GAP))
        .min_h(px(ITEM_HEIGHT))
        .px(px(ITEM_PAD))
        .when(item.inset, |el| el.pl(px(INSET)))
        .when(check_kind, |el| el.pr(px(CHECK_ROOM)))
        .rounded(px(ITEM_RADIUS))
        .when_some(bg, |el, bg| el.bg(bg))
        .text_color(if strong && !highlighted && !item.danger {
            theme.color(ColorToken::FgStrong)
        } else {
            fg
        })
        .font_weight(if strong {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .when(item.disabled, |el| el.opacity(0.5))
        .when(!item.disabled, |el| el.cursor_pointer())
        .role(role)
        .aria_label(item.label.clone())
        .when_some(toggled, |el, toggled| el.aria_toggled(toggled))
        .when(submenu, |el| el.aria_expanded(expanded))
        .when_some(item.shortcut.clone(), |el, keys| el.aria_keyshortcuts(keys))
        .when(highlighted, |el| el.aria_active_descendant())
        .on_hover(move |hovered, window, cx| {
            if *hovered {
                hover_ctx.hover(level, ix, window, cx);
            }
        })
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .on_click(move |_, window, cx| click_ctx.select(&clicked, window, cx))
        .when_some(item.icon, |el, icon| {
            el.child(glyph(icon, px(ICON), icon_fg))
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(item.label.clone()),
        )
        .when_some(item.meta.clone(), |el, meta| {
            el.child(
                div()
                    .ml_auto()
                    .pl(px(16.0))
                    .text_color(theme.color(ColorToken::FgFaint))
                    .text_size(px(12.5 * FONT_SANS.size_adjust))
                    .font_weight(FontWeight::NORMAL)
                    .child(meta),
            )
        })
        .when_some(item.shortcut.clone(), |el, keys| {
            el.child(
                div()
                    .ml_auto()
                    .pl(px(16.0))
                    .text_color(theme.color(ColorToken::FgFaint))
                    .font_family(FONT_MONO.name)
                    .text_size(px(11.0 * FONT_MONO.size_adjust))
                    .font_weight(FontWeight::MEDIUM)
                    .line_height(relative(1.0))
                    .child(keys),
            )
        })
        .when(submenu, |el| {
            el.child(
                div()
                    .ml_auto()
                    .child(glyph(IconName::ChevronRight, px(ICON), icon_fg)),
            )
        })
        .when_some(checked, |el, icon| {
            el.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(ITEM_PAD))
                    .flex()
                    .items_center()
                    .child(glyph(icon, px(ICON), theme.color(ColorToken::Pine600))),
            )
        })
        .when_some(slot, |el, slot| el.child(anchor::bounds_of(slot)))
        .into_any_element()
}

fn level_panel(
    ctx: &Ctx,
    level: usize,
    place: Place,
    container: Option<&FocusHandle>,
    look: &crate::motion::Look,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state = ctx.state.read(cx);
    let path = state.path.clone();
    let entries = level_entries(&ctx.entries, &path, level);
    let highlighted = path.get(level).copied().flatten();
    let open_child = path.len() > level + 1;
    let theme = ctx.theme;
    let (bg, border) = surface_colors(&theme);
    let viewport = window.viewport_size();
    let rtl = LayoutDirection::global(cx).is_rtl();

    let mut sub_place = None;
    let mut rows: Vec<AnyElement> = Vec::with_capacity(entries.len());
    for (ix, entry) in entries.iter().enumerate() {
        match entry {
            MenuEntry::Separator => rows.push(
                div()
                    .id(ElementId::NamedChild(
                        level_id(&ctx.id, level).into(),
                        SharedString::from(format!("sep-{ix}")),
                    ))
                    .h(px(1.0))
                    .mx(px(6.0))
                    .my(px(4.0))
                    .bg(theme.color(ColorToken::BorderSoft))
                    .role(Role::Splitter)
                    .into_any_element(),
            ),
            MenuEntry::Label(text) => rows.push(
                div()
                    .id(ElementId::NamedChild(
                        level_id(&ctx.id, level).into(),
                        SharedString::from(format!("label-{ix}")),
                    ))
                    .px(px(ITEM_PAD))
                    .pt(px(8.0))
                    .pb(px(4.0))
                    .text_size(px(11.0 * FONT_SANS.size_adjust))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(relative(1.0))
                    .text_color(theme.color(ColorToken::FgFaint))
                    .role(Role::Label)
                    .child(SharedString::from(text.to_uppercase()))
                    .into_any_element(),
            ),
            MenuEntry::Item(item) => {
                let is_highlighted = highlighted == Some(ix);
                let is_sub = matches!(item.kind, Kind::Submenu(_));
                let slot = is_sub.then(|| item_slot(&ctx.id, level, ix, window, cx));
                if is_highlighted && is_sub && open_child {
                    sub_place = slot.clone().map(|slot| Place::Side {
                        trigger: slot.get(),
                        placement: if rtl {
                            Placement::Left
                        } else {
                            Placement::Right
                        },
                        align: Align::Start,
                        offset: px(2.0),
                    });
                }
                rows.push(item_row(
                    ctx,
                    level,
                    ix,
                    item,
                    is_highlighted,
                    is_highlighted && open_child,
                    slot,
                ));
            }
        }
    }

    let child_panel = sub_place.map(|place| {
        let look = MENU.sample(level_id(&ctx.id, level + 1), true, window, cx);
        level_panel(ctx, level + 1, place, None, &look, window, cx)
    });

    let key_ctx = ctx.clone();
    let out_ctx = ctx.clone();
    let open_levels = path.len();
    let panel = div()
        .id(level_id(&ctx.id, level))
        .role(Role::Menu)
        .when_some(container, |el, container| {
            let next = key_ctx.clone();
            let prev = key_ctx.clone();
            let first = key_ctx.clone();
            let last = key_ctx.clone();
            let right = key_ctx.clone();
            let left = key_ctx.clone();
            let go = key_ctx.clone();
            let back = key_ctx.clone();
            let exit = key_ctx.clone();
            let typed = key_ctx.clone();
            el.track_focus(container)
                .key_context(MENU_CONTEXT)
                .on_action(move |_: &MenuNext, window, cx| next.mv(Nav::Next, window, cx))
                .on_action(move |_: &MenuPrev, window, cx| prev.mv(Nav::Prev, window, cx))
                .on_action(move |_: &MenuFirst, window, cx| first.mv(Nav::First, window, cx))
                .on_action(move |_: &MenuLast, window, cx| last.mv(Nav::Last, window, cx))
                .on_action(move |_: &MenuRight, window, cx| right.sideways(true, window, cx))
                .on_action(move |_: &MenuLeft, window, cx| left.sideways(false, window, cx))
                .on_action(move |_: &MenuActivate, window, cx| go.activate(window, cx))
                .on_action(move |_: &MenuBack, window, cx| {
                    if !back.close_sub(window, cx) {
                        back.close(window, cx);
                    }
                })
                .on_action(move |_: &MenuExit, window, cx| exit.close(window, cx))
                .on_key_down(move |event, window, cx| typed.typeahead(event, window, cx))
                .on_mouse_down_out(move |event, window, cx| {
                    if !out_ctx.inside(event.position, open_levels) {
                        out_ctx.close(window, cx);
                    }
                })
        })
        .look(look)
        .when_some(ctx.width, |el, width| el.w(px(width)))
        .min_w(px(MIN_WIDTH))
        .max_h(viewport.height * MAX_HEIGHT)
        .overflow_y_scroll()
        .p(px(PAD))
        .rounded(px(RADIUS_MENU))
        .border_1()
        .border_color(theme.color(border))
        .bg(theme.color(bg))
        .shadow(theme.shadow(ShadowToken::Md))
        .font_family(FONT_SANS.name)
        .text_size(px(14.0 * FONT_SANS.size_adjust))
        .line_height(relative(1.2))
        .text_color(theme.color(ColorToken::Fg))
        .flex()
        .flex_col()
        .children(rows)
        .children(child_panel);

    let hit = ctx.hits.get(level).cloned();
    anchor::anchored(
        &level_id(&ctx.id, level),
        Layer::Popover,
        level,
        place,
        hit,
        panel,
        window,
        cx,
    )
}

fn render_menu(
    ctx: &Ctx,
    place: Place,
    return_to: Option<FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let open = ctx.state.read(cx).open;
    let container = focus::handle(&ctx.id, "menu", window, cx);
    focus::follow_to(&ctx.id, open, &container, None, return_to, window, cx);
    let look = MENU.sample(level_id(&ctx.id, 0), open, window, cx);
    if !look.present {
        for level in 0..MAX_DEPTH {
            layer::cover(&level_id(&ctx.id, level), window, cx).clear(window, cx);
        }
        return None;
    }
    Some(level_panel(
        ctx,
        0,
        place,
        Some(&container),
        &look,
        window,
        cx,
    ))
}

type Trigger = Box<dyn FnOnce(bool, Action, FocusHandle) -> AnyElement>;

#[derive(IntoElement)]
pub struct DropdownMenu {
    id: ElementId,
    trigger: Option<Trigger>,
    entries: Vec<MenuEntry>,
    width: Option<f32>,
    placement: Placement,
    align: Align,
    theme: Option<Theme>,
}

impl DropdownMenu {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            trigger: None,
            entries: Vec::new(),
            width: None,
            placement: Placement::Bottom,
            align: Align::Start,
            theme: None,
        }
    }

    pub fn trigger<E: IntoElement>(
        mut self,
        build: impl FnOnce(bool, Action, FocusHandle) -> E + 'static,
    ) -> Self {
        self.trigger = Some(Box::new(move |open, toggle, focus| {
            build(open, toggle, focus).into_any_element()
        }));
        self
    }

    pub fn entries(mut self, entries: impl IntoIterator<Item = MenuEntry>) -> Self {
        self.entries.extend(entries);
        self
    }

    pub fn entry(mut self, entry: impl Into<MenuEntry>) -> Self {
        self.entries.push(entry.into());
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

fn state(id: &ElementId, window: &mut Window, cx: &mut App) -> Entity<MenuState> {
    window.use_keyed_state(
        ElementId::NamedChild(id.clone().into(), "menu-state".into()),
        cx,
        |_, _| MenuState::default(),
    )
}

impl RenderOnce for DropdownMenu {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let state = state(&self.id, window, cx);
        let open = state.read(cx).open;
        let trigger_bounds = anchor::slot(&self.id, "trigger", window, cx);
        let toggle: Action = {
            let state = state.clone();
            Rc::new(move |window, cx| {
                state.update(cx, |state, _| {
                    if state.open {
                        state.hide();
                    } else {
                        state.show(None);
                    }
                });
                window.refresh();
            })
        };
        let ctx = Ctx {
            id: self.id.clone(),
            state,
            entries: Rc::new(std::mem::take(&mut self.entries)),
            hits: hits(&self.id, window, cx),
            trigger: trigger_bounds.clone(),
            theme,
            width: self.width,
        };
        let trigger_focus = focus::handle(&self.id, "trigger-focus", window, cx);
        let menu = render_menu(
            &ctx,
            Place::Side {
                trigger: trigger_bounds.get(),
                placement: self.placement,
                align: self.align,
                offset: px(OFFSET),
            },
            Some(trigger_focus.clone()),
            window,
            cx,
        );
        let trigger = self
            .trigger
            .take()
            .map(|build| build(open, toggle, trigger_focus))
            .unwrap_or_else(|| div().into_any_element());
        div()
            .relative()
            .flex()
            .flex_none()
            .child(trigger)
            .child(anchor::bounds_of(trigger_bounds))
            .children(menu)
    }
}

#[derive(IntoElement)]
pub struct ContextMenu {
    id: ElementId,
    target: AnyElement,
    entries: Vec<MenuEntry>,
    width: Option<f32>,
    theme: Option<Theme>,
}

impl ContextMenu {
    pub fn new(id: impl Into<ElementId>, target: impl IntoElement) -> Self {
        Self {
            id: id.into(),
            target: target.into_any_element(),
            entries: Vec::new(),
            width: None,
            theme: None,
        }
    }

    pub fn entries(mut self, entries: impl IntoIterator<Item = MenuEntry>) -> Self {
        self.entries.extend(entries);
        self
    }

    pub fn entry(mut self, entry: impl Into<MenuEntry>) -> Self {
        self.entries.push(entry.into());
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl RenderOnce for ContextMenu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let state = state(&self.id, window, cx);
        let target_bounds = anchor::slot(&self.id, "target", window, cx);
        let at = state.read(cx).at;
        let place = match at {
            Some(point) => Place::At(point),
            None => Place::Side {
                trigger: target_bounds.get(),
                placement: Placement::Bottom,
                align: Align::Start,
                offset: px(OFFSET),
            },
        };
        let ctx = Ctx {
            id: self.id.clone(),
            state: state.clone(),
            entries: Rc::new(self.entries),
            hits: hits(&self.id, window, cx),
            trigger: Rc::new(Cell::new(Bounds::default())),
            theme,
            width: self.width,
        };
        let menu = render_menu(&ctx, place, None, window, cx);
        let by_mouse = state.clone();
        let by_key = state;
        div()
            .id(ElementId::NamedChild(
                self.id.clone().into(),
                "target".into(),
            ))
            .relative()
            .key_context(CONTEXT_TARGET)
            .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                by_mouse.update(cx, |state, _| state.show(Some(event.position)));
                window.refresh();
                cx.stop_propagation();
            })
            .on_action(move |_: &OpenContextMenu, window, cx| {
                by_key.update(cx, |state, _| state.show(None));
                window.refresh();
            })
            .child(self.target)
            .child(anchor::bounds_of(target_bounds))
            .children(menu)
    }
}
