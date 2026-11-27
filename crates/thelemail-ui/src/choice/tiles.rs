use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement, RenderOnce, Role,
    SharedString, StatefulInteractiveElement, Styled, Toggled, Window, accesskit, canvas, div, px,
    relative,
};

use super::look::{self, TILE_DISABLED_OPACITY};
use super::roving::{self, Pick};
use super::{Handler, child, hovered, is_enter, ring, set_hover};
use crate::a11y::A11yStateExt as _;
use crate::button::Phase;
use crate::icons::{IconName, glyph};
use crate::motion::{BASE, EASE, EASE_OUT, FAST, Spec, animate, ease_color};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_SANS, RADIUS_PILL, RADIUS_SURFACE};

const GAP: f32 = 10.0;
const MIN_WIDTH: f32 = 170.0;
const PAD_Y: f32 = 13.0;
const PAD_X: f32 = 14.0;
const HEAD_GAP: f32 = 11.0;
const TEXT_GAP: f32 = 3.0;
const ICON: f32 = 18.0;
const NUDGE: f32 = 1.0;
const TITLE: f32 = 14.0;
const DESCRIPTION: f32 = 12.5;
const MARK: f32 = 18.0;
const MARK_BORDER: f32 = 1.5;
const DOT: f32 = 8.0;
const SQUARE_MARK_RADIUS: f32 = 5.0;
const SQUARE_DOT: f32 = 10.0;
const SQUARE_DOT_RADIUS: f32 = 2.0;
const BADGE_HEIGHT: f32 = 20.0;
const BADGE_PAD: f32 = 7.0;
const BADGE_FONT: f32 = 11.0;
const COLOR: Spec = Spec::new(BASE, EASE);
const GROW: Spec = Spec::new(FAST, EASE_OUT);

pub fn columns(width: f32, min: f32, gap: f32, count: usize) -> usize {
    if count == 0 {
        return 1;
    }
    let fit = if width > 0.0 && min > 0.0 {
        ((width + gap) / (min + gap)).floor() as usize
    } else {
        1
    };
    fit.clamp(1, count)
}

#[derive(Debug, Clone)]
pub struct Tile {
    value: SharedString,
    title: SharedString,
    description: Option<SharedString>,
    icon: Option<IconName>,
    badge: Option<SharedString>,
    disabled: bool,
}

impl Tile {
    pub fn new(value: impl Into<SharedString>, title: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            title: title.into(),
            description: None,
            icon: None,
            badge: None,
            disabled: false,
        }
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn badge(mut self, text: impl Into<SharedString>) -> Self {
        self.badge = Some(text.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(IntoElement)]
pub struct OptionTiles {
    id: ElementId,
    tiles: Vec<Tile>,
    value: Option<SharedString>,
    columns: Option<usize>,
    min_width: f32,
    square: bool,
    label: Option<SharedString>,
    disabled: bool,
    theme: Option<Theme>,
    on_change: Option<Handler<SharedString>>,
}

impl OptionTiles {
    pub fn new(id: impl Into<ElementId>, tiles: Vec<Tile>) -> Self {
        Self {
            id: id.into(),
            tiles,
            value: None,
            columns: None,
            min_width: MIN_WIDTH,
            square: false,
            label: None,
            disabled: false,
            theme: None,
            on_change: None,
        }
    }

    pub fn value(mut self, value: Option<impl Into<SharedString>>) -> Self {
        self.value = value.map(Into::into);
        self
    }

    pub fn columns(mut self, columns: usize) -> Self {
        self.columns = Some(columns.max(1));
        self
    }

    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = width;
        self
    }

    pub fn square_mark(mut self) -> Self {
        self.square = true;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn on_change(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

fn badge(text: SharedString, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(BADGE_HEIGHT))
        .px(px(BADGE_PAD))
        .rounded(px(RADIUS_PILL))
        .bg(theme.color(ColorToken::Pine100))
        .text_color(theme.color(ColorToken::Pine700))
        .text_size(px(BADGE_FONT * FONT_SANS.size_adjust))
        .font_weight(FontWeight::SEMIBOLD)
        .line_height(relative(1.0))
        .whitespace_nowrap()
        .child(text)
}

impl RenderOnce for OptionTiles {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let count = self.tiles.len();
        let width = window
            .use_keyed_state(child(&self.id, "width"), cx, |_, _| Rc::new(Cell::new(0.0f32)))
            .read(cx)
            .clone();
        let cols = self
            .columns
            .unwrap_or_else(|| columns(width.get(), self.min_width, GAP, count));
        let selected = self
            .tiles
            .iter()
            .position(|tile| self.value.as_ref() == Some(&tile.value));
        let enabled: Vec<bool> = self
            .tiles
            .iter()
            .map(|tile| !tile.disabled && !self.disabled)
            .collect();
        let handles = roving::handles(&self.id, count, window, cx);
        let stop = roving::stop(selected, &enabled);
        let values: Vec<SharedString> = self.tiles.iter().map(|t| t.value.clone()).collect();
        let on_change = self.on_change.clone();
        let pick: Pick = Rc::new(move |ix, window, cx| {
            if let (Some(on_change), Some(value)) = (&on_change, values.get(ix)) {
                on_change(value.clone(), window, cx);
            }
        });
        let ring_color = theme.color(ColorToken::FocusRing);
        let square = self.square;

        let mut tiles = Vec::with_capacity(count);
        for (ix, tile) in self.tiles.into_iter().enumerate() {
            let tile_id = ElementId::NamedChild(self.id.clone().into(), tile.value.clone());
            let checked = selected == Some(ix);
            let interactive = enabled[ix];
            let hover = hovered(&tile_id, window, cx);
            let phase = if *hover.read(cx) {
                Phase::Hover
            } else {
                Phase::Rest
            };
            let paint = look::tile(checked, interactive, phase);
            let ink = |name: &'static str, fill: crate::button::Fill, window: &mut Window, cx: &mut App| {
                ease_color(child(&tile_id, name), fill.resolve(&theme), COLOR, window, cx)
            };
            let bg = ink("bg", paint.bg, window, cx);
            let border = ink("border", paint.border, window, cx);
            let title_color = ink("title", paint.title, window, cx);
            let icon_color = ink("icon", paint.icon, window, cx);
            let mark_color = ink("mark", paint.mark, window, cx);
            let dot = animate(
                child(&tile_id, "dot"),
                if checked { 1.0 } else { 0.0 },
                GROW,
                window,
                cx,
            )
            .value
            .max(0.0);
            let handle = handles[ix].clone();
            let focused = handle.is_focused(window);
            let keyboard = focused && window.last_input_was_keyboard();
            let focus_on_click = handle.clone();
            let pick = pick.clone();
            let (mark_radius, dot_size, dot_radius) = if square {
                (SQUARE_MARK_RADIUS, SQUARE_DOT, SQUARE_DOT_RADIUS)
            } else {
                (MARK / 2.0, DOT, DOT / 2.0)
            };

            let mark = div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .mt(px(NUDGE))
                .size(px(MARK))
                .rounded(px(mark_radius))
                .border(px(MARK_BORDER))
                .border_color(mark_color)
                .bg(theme.color(ColorToken::Surface))
                .child(
                    div()
                        .flex_none()
                        .size(px(dot_size * dot))
                        .rounded(px(dot_radius * dot))
                        .bg(theme.color(ColorToken::Primary)),
                );

            let text = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(TEXT_GAP))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .text_color(title_color)
                        .text_size(px(TITLE * FONT_SANS.size_adjust))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(relative(1.3))
                        .child(tile.title.clone())
                        .when_some(tile.badge, |el, text| el.child(badge(text, &theme))),
                )
                .when_some(tile.description.clone(), |el, text| {
                    el.child(
                        div()
                            .text_color(theme.color(ColorToken::FgMuted))
                            .text_size(px(DESCRIPTION * FONT_SANS.size_adjust))
                            .line_height(relative(1.45))
                            .child(text),
                    )
                });

            tiles.push(
                div()
                    .id(tile_id)
                    .relative()
                    .flex()
                    .items_start()
                    .gap(px(HEAD_GAP))
                    .py(px(PAD_Y))
                    .px(px(PAD_X))
                    .rounded(px(RADIUS_SURFACE))
                    .border_1()
                    .border_color(border)
                    .bg(bg)
                    .font_family(FONT_SANS.name)
                    .text_color(theme.color(ColorToken::Fg))
                    .when(interactive, |el| el.cursor_pointer())
                    .when(!interactive, |el| {
                        el.cursor_not_allowed().opacity(TILE_DISABLED_OPACITY)
                    })
                    .role(Role::RadioButton)
                    .aria_label(tile.title)
                    .when_some(tile.description, |el, text| el.aria_description(text))
                    .aria_toggled(if checked {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .aria_selected(checked)
                    .aria_position_in_set(ix + 1)
                    .aria_size_of_set(count)
                    .when(interactive || focused, |el| {
                        el.track_focus(&handle.tab_index(0).tab_stop(stop == Some(ix)))
                    })
                    .on_hover(set_hover(hover))
                    .on_click(move |event, window, cx| {
                        if !interactive || is_enter(event) {
                            return;
                        }
                        window.focus(&focus_on_click, cx);
                        if !checked {
                            pick(ix, window, cx);
                        }
                    })
                    .when_some(tile.icon, |el, icon| {
                        el.child(glyph(icon, px(ICON), icon_color).mt(px(NUDGE)))
                    })
                    .child(text)
                    .child(mark)
                    .when(keyboard, |el| el.child(ring(1.0, RADIUS_SURFACE, ring_color)))
                    .a11y_state(false, !interactive),
            );
        }

        let measured = width.clone();
        div()
            .id(self.id.clone())
            .relative()
            .w_full()
            .grid()
            .grid_cols(cols as u16)
            .gap(px(GAP))
            .role(Role::RadioGroup)
            .aria_orientation(accesskit::Orientation::Horizontal)
            .when_some(self.label, |el, label| el.aria_label(label))
            .on_key_down(roving::on_key(handles, enabled, true, pick))
            .children(tiles)
            .child(
                canvas(
                    move |bounds, window, _| {
                        let now = f32::from(bounds.size.width);
                        if (measured.get() - now).abs() > 0.5 {
                            measured.set(now);
                            window.refresh();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0(),
            )
            .a11y_state(false, self.disabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_fit_matches_the_css_track_count() {
        assert_eq!(columns(360.0, 170.0, 10.0, 4), 2);
        assert_eq!(columns(349.0, 170.0, 10.0, 4), 1);
        assert_eq!(columns(350.0, 170.0, 10.0, 4), 2);
        assert_eq!(columns(1000.0, 170.0, 10.0, 4), 4);
    }

    #[test]
    fn empty_tracks_collapse_so_few_tiles_stretch() {
        assert_eq!(columns(1000.0, 170.0, 10.0, 2), 2);
        assert_eq!(columns(1000.0, 170.0, 10.0, 0), 1);
    }

    #[test]
    fn an_unmeasured_grid_starts_with_one_column() {
        assert_eq!(columns(0.0, 170.0, 10.0, 3), 1);
    }
}
