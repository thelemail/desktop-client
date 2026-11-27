use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::base::FocusTrapElement as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ElementId, FocusHandle, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, Pixels, RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled,
    Window, canvas, div, px, relative,
};
use thelemail_i18n::m;

use super::layer::{self, Layer};
use super::{Dismiss, MODAL, focus};
use crate::button::{Button, Size};
use crate::icons::{IconName, glyph};
use crate::motion::{DIALOG, Look, Moving as _, OVERLAY, SHEET};
use crate::theme::{ActiveTokens, Appearance, Theme};
use crate::tokens::{ColorToken, FONT_MONO, FONT_SANS, RADIUS_PANEL};

pub const NARROW: f32 = 560.0;
const SIDE_GAP: f32 = 16.0;
const TOP_GAP: f32 = 24.0;
const SHEET_TOP: f32 = 32.0;
const PAD_X: f32 = 22.0;
const PAD_TOP: f32 = 22.0;
const PAD_BOTTOM: f32 = 20.0;
const GAP: f32 = 16.0;
const ICON_BOX: f32 = 36.0;
const ICON_GLYPH: f32 = 18.0;
const CLOSE_INSET: f32 = 14.0;
const HEADER_CLEAR: f32 = 28.0;
const GRAB_WIDTH: f32 = 36.0;
const GRAB_HEIGHT: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DialogSize {
    Sm,
    #[default]
    Md,
    Wide,
    Lg,
    Xl,
}

impl DialogSize {
    pub const ALL: [Self; 5] = [Self::Sm, Self::Md, Self::Wide, Self::Lg, Self::Xl];

    pub const fn width(self) -> f32 {
        match self {
            Self::Sm => 400.0,
            Self::Md => 440.0,
            Self::Wide => 560.0,
            Self::Lg => 640.0,
            Self::Xl => 940.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Presentation {
    #[default]
    Auto,
    Sheet,
}

impl Presentation {
    pub fn sheet(self, viewport_width: f32) -> bool {
        self == Self::Sheet || viewport_width <= NARROW
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Brand,
    Danger,
    Warning,
    Success,
    Neutral,
}

impl Tone {
    pub const ALL: [Self; 5] = [
        Self::Brand,
        Self::Danger,
        Self::Warning,
        Self::Success,
        Self::Neutral,
    ];

    fn icon_colors(self, theme: &Theme) -> (Hsla, Hsla) {
        match self {
            Self::Brand => (
                theme.color(ColorToken::Pine100),
                theme.color(ColorToken::Pine700),
            ),
            Self::Danger => (
                theme.color(ColorToken::Danger100),
                theme.color(ColorToken::Danger700),
            ),
            Self::Warning => (
                theme.color(ColorToken::Warning100),
                theme.color(ColorToken::Warning700),
            ),
            Self::Success => (
                theme.color(ColorToken::Success100),
                theme.color(ColorToken::Success700),
            ),
            Self::Neutral => (
                theme.color(ColorToken::NeutralFill),
                theme.color(ColorToken::FgMuted),
            ),
        }
    }

    fn eyebrow(self, theme: &Theme) -> Hsla {
        match self {
            Self::Danger => theme.color(ColorToken::Danger700),
            _ => theme.color(ColorToken::Brass700),
        }
    }
}

pub(crate) type Action = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Dialog {
    id: ElementId,
    open: bool,
    size: DialogSize,
    presentation: Presentation,
    alert: bool,
    tone: Tone,
    icon: Option<IconName>,
    eyebrow: Option<SharedString>,
    title: Option<SharedString>,
    description: Option<SharedString>,
    sub: bool,
    body: Vec<AnyElement>,
    footer: Vec<AnyElement>,
    dismissible: bool,
    close_button: bool,
    initial_focus: Option<FocusHandle>,
    on_close: Option<Action>,
    theme: Option<Theme>,
}

pub struct Sheet;

impl Sheet {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: impl Into<ElementId>) -> Dialog {
        Dialog::new(id).presentation(Presentation::Sheet)
    }
}

impl Dialog {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            open: false,
            size: DialogSize::default(),
            presentation: Presentation::default(),
            alert: false,
            tone: Tone::default(),
            icon: None,
            eyebrow: None,
            title: None,
            description: None,
            sub: false,
            body: Vec::new(),
            footer: Vec::new(),
            dismissible: true,
            close_button: true,
            initial_focus: None,
            on_close: None,
            theme: None,
        }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn size(mut self, size: DialogSize) -> Self {
        self.size = size;
        self
    }

    pub fn presentation(mut self, presentation: Presentation) -> Self {
        self.presentation = presentation;
        self
    }

    pub fn alert(mut self, alert: bool) -> Self {
        self.alert = alert;
        self
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn eyebrow(mut self, text: impl Into<SharedString>) -> Self {
        self.eyebrow = Some(text.into());
        self
    }

    pub fn title(mut self, text: impl Into<SharedString>) -> Self {
        self.title = Some(text.into());
        self
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    pub fn subtitle(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self.sub = true;
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.body.push(child.into_any_element());
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.body
            .extend(children.into_iter().map(IntoElement::into_any_element));
        self
    }

    pub fn footer(mut self, child: impl IntoElement) -> Self {
        self.footer.push(child.into_any_element());
        self
    }

    pub fn dismissible(mut self, dismissible: bool) -> Self {
        self.dismissible = dismissible;
        self
    }

    pub fn close_button(mut self, shown: bool) -> Self {
        self.close_button = shown;
        self
    }

    pub fn initial_focus(mut self, handle: &FocusHandle) -> Self {
        self.initial_focus = Some(handle.clone());
        self
    }

    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    fn child_id(&self, name: &'static str) -> ElementId {
        ElementId::NamedChild(self.id.clone().into(), name.into())
    }

    fn header(&mut self, theme: &Theme) -> Option<AnyElement> {
        if self.title.is_none() && self.eyebrow.is_none() && self.icon.is_none() {
            return None;
        }
        let (icon_bg, icon_fg) = self.tone.icon_colors(theme);
        let text = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .when_some(self.eyebrow.take(), |el, eyebrow| {
                el.child(
                    div()
                        .mb(px(-2.0))
                        .font_family(FONT_MONO.name)
                        .text_size(px(10.5 * FONT_MONO.size_adjust))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(relative(1.2))
                        .text_color(self.tone.eyebrow(theme))
                        .child(SharedString::from(eyebrow.to_uppercase())),
                )
            })
            .when_some(self.title.clone(), |el, title| {
                el.child(
                    div()
                        .font_family(FONT_SANS.name)
                        .text_size(px(19.0 * FONT_SANS.size_adjust))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(relative(1.3))
                        .text_color(theme.color(ColorToken::FgStrong))
                        .child(title),
                )
            })
            .when_some(self.description.take(), |el, description| {
                let sub = self.sub;
                el.child(
                    div()
                        .font_family(FONT_SANS.name)
                        .text_size(px(if sub { 12.5 } else { 14.0 } * FONT_SANS.size_adjust))
                        .line_height(relative(1.5))
                        .text_color(theme.color(ColorToken::FgMuted))
                        .when(sub, |el| {
                            el.whitespace_nowrap().overflow_hidden().text_ellipsis()
                        })
                        .child(description),
                )
            });
        Some(
            div()
                .flex()
                .items_start()
                .gap(px(12.0))
                .when(self.close_button, |el| el.pr(px(HEADER_CLEAR)))
                .when_some(self.icon, |el, icon| {
                    el.child(
                        div()
                            .flex_none()
                            .size(px(ICON_BOX))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(icon_bg)
                            .child(glyph(icon, px(ICON_GLYPH), icon_fg)),
                    )
                })
                .child(text)
                .into_any_element(),
        )
    }
}

#[derive(IntoElement)]
pub struct DialogField {
    label: SharedString,
    control: AnyElement,
    theme: Option<Theme>,
}

impl DialogField {
    pub fn new(label: impl Into<SharedString>, control: impl IntoElement) -> Self {
        Self {
            label: label.into(),
            control: control.into_any_element(),
            theme: None,
        }
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl RenderOnce for DialogField {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .font_family(FONT_SANS.name)
                    .text_size(px(12.5 * FONT_SANS.size_adjust))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.color(ColorToken::FgMuted))
                    .child(self.label),
            )
            .child(self.control)
    }
}

fn measured(height: Rc<Cell<Pixels>>) -> impl IntoElement {
    div().absolute().top_0().left_0().size_full().child(
        canvas(
            move |bounds, _, _| height.set(bounds.size.height),
            |_, _, _, _| {},
        )
        .size_full(),
    )
}

impl RenderOnce for Dialog {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let viewport = window.viewport_size();
        let sheet = self.presentation.sheet(f32::from(viewport.width));
        let container = focus::handle(&self.id, "panel", window, cx);
        focus::follow(
            &self.id,
            self.open,
            &container,
            self.initial_focus.clone(),
            window,
            cx,
        );
        let scrim = OVERLAY.sample(self.child_id("scrim"), self.open, window, cx);
        let panel: Look = if sheet {
            SHEET.sample(self.child_id("sheet"), self.open, window, cx)
        } else {
            DIALOG.sample(self.child_id("dialog"), self.open, window, cx)
        };
        let cover = layer::cover(&self.id, window, cx);
        if !scrim.present && !panel.present {
            cover.clear(window, cx);
            return div().into_any_element();
        }

        let height = window
            .use_keyed_state(self.child_id("height"), cx, |window, _| {
                Rc::new(Cell::new(window.viewport_size().height))
            })
            .read(cx)
            .clone();
        let close: Option<Action> = self.on_close.clone().filter(|_| self.dismissible);
        let dark = theme.appearance == Appearance::Dark;
        let described = self.description.clone();
        let header = self.header(&theme);
        let footer = std::mem::take(&mut self.footer);
        let body = std::mem::take(&mut self.body);
        let has_footer = !footer.is_empty();

        let close_button = (self.close_button && self.dismissible).then(|| {
            let close = close.clone();
            div()
                .absolute()
                .top(px(CLOSE_INSET))
                .right(px(CLOSE_INSET))
                .child(
                    Button::new(self.child_id("close"))
                        .ghost()
                        .size(Size::IconSm)
                        .icon(IconName::Xmark)
                        .accessibility_label(m::common_close())
                        .theme(theme)
                        .on_click(move |_, window, cx| {
                            if let Some(close) = close.as_ref() {
                                close(window, cx);
                            }
                        }),
                )
        });

        let width = if sheet {
            viewport.width
        } else {
            px(self.size.width()).min(viewport.width - px(SIDE_GAP * 2.0))
        };
        let max_height = if sheet {
            viewport.height - px(SHEET_TOP)
        } else {
            viewport.height - px(TOP_GAP * 2.0)
        };
        let escape = close.clone();
        let surface = div()
            .id(self.child_id("surface"))
            .key_context(MODAL)
            .role(if self.alert {
                Role::AlertDialog
            } else {
                Role::Dialog
            })
            .when_some(self.title.clone(), |el, title| el.aria_label(title))
            .when_some(described, |el, text| el.aria_description(text))
            .on_action(move |_: &Dismiss, window, cx| {
                if let Some(close) = escape.as_ref() {
                    close(window, cx);
                }
            })
            .occlude()
            .relative()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .w(width)
            .max_h(max_height)
            .px(px(PAD_X))
            .pt(px(PAD_TOP))
            .pb(px(PAD_BOTTOM))
            .bg(theme.color(if dark {
                ColorToken::BgRaised
            } else {
                ColorToken::Surface
            }))
            .border_color(theme.color(if dark {
                ColorToken::BorderStrong
            } else {
                ColorToken::BorderSoft
            }))
            .map(|el| {
                if sheet {
                    el.border_t_1()
                        .border_l_1()
                        .border_r_1()
                        .rounded_t(px(RADIUS_PANEL))
                        .look_within(&panel, height.get())
                } else {
                    el.border_1().rounded(px(RADIUS_PANEL)).look(&panel)
                }
            })
            .font_family(FONT_SANS.name)
            .text_color(theme.color(ColorToken::Fg))
            .child(measured(height))
            .when(sheet, |el| {
                el.child(
                    div()
                        .flex()
                        .justify_center()
                        .mt(px(-10.0))
                        .mb(px(2.0) - px(GAP))
                        .child(
                            div()
                                .w(px(GRAB_WIDTH))
                                .h(px(GRAB_HEIGHT))
                                .rounded(px(2.0))
                                .bg(theme.color(ColorToken::BorderStrong)),
                        ),
                )
            })
            .children(header)
            .when(!body.is_empty(), |el| {
                el.child(
                    div()
                        .id(self.child_id("body"))
                        .flex_shrink(1.0)
                        .min_h_0()
                        .overflow_y_scroll()
                        .mx(px(-PAD_X))
                        .my(px(-4.0))
                        .px(px(PAD_X))
                        .py(px(4.0))
                        .flex()
                        .flex_col()
                        .children(body),
                )
            })
            .when(has_footer, |el| {
                el.child(div().mt(px(4.0)).flex().gap(px(8.0)).map(|row| {
                    if sheet {
                        row.flex_col()
                            .items_stretch()
                            .children(footer.into_iter().rev())
                    } else {
                        row.flex_wrap().justify_end().children(footer)
                    }
                }))
            })
            .children(close_button)
            .focus_trap(self.child_id("trap"), &container);

        let backdrop_close = close;
        let host = div()
            .id(self.child_id("host"))
            .relative()
            .w(viewport.width)
            .h(viewport.height)
            .flex()
            .flex_col()
            .items_center()
            .map(|el| {
                if sheet {
                    el.justify_end()
                } else {
                    el.justify_center()
                }
            })
            .child(
                div()
                    .id(self.child_id("scrim-hit"))
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .occlude()
                    .bg(theme.color(ColorToken::Scrim))
                    .opacity(scrim.opacity)
                    .on_mouse_down(gpui_kit::MouseButton::Left, move |_, window, cx| {
                        if let Some(close) = backdrop_close.as_ref() {
                            close(window, cx);
                        }
                    }),
            )
            .child(surface)
            .child(layer::tracker(cover));

        layer::host(Layer::Dialog, host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_match_the_web() {
        let widths: Vec<f32> = DialogSize::ALL.iter().map(|s| s.width()).collect();
        assert_eq!(widths, [400.0, 440.0, 560.0, 640.0, 940.0]);
    }

    #[test]
    fn narrow_windows_get_the_sheet() {
        assert!(Presentation::Auto.sheet(560.0));
        assert!(!Presentation::Auto.sheet(561.0));
        assert!(Presentation::Sheet.sheet(1400.0));
    }
}
