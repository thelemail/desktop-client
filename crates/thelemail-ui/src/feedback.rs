use std::rc::Rc;

use gpui_kit::base::Link as LinkRoot;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, FocusHandle, FontWeight, InteractiveElement,
    IntoElement, ParentElement, RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled,
    Window, div, px, relative,
};

use crate::icons::{IconName, glyph};
use crate::motion::{BASE, EASE_OUT, MotionHandle, Spec, ease_color};
use crate::spinner::{STATUS_PERIOD, Spinner};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_SANS, RADIUS_LG};

const STATUS_FONT: f32 = 13.0;
const STATUS_LINE: f32 = 1.45;
const STATUS_LINES: f32 = 2.0;
const STATUS_GAP: f32 = 7.0;
const STATUS_ICON: f32 = 14.0;
const RISE: f32 = 3.0;
const NOTE_FONT: f32 = 13.5;
const NOTE_GAP: f32 = 9.0;
const NOTE_PAD_Y: f32 = 12.0;
const NOTE_PAD_X: f32 = 13.0;
const NOTE_ICON: f32 = 16.0;
const NOTE_BORDER_ERROR: f32 = 0.3;
const NOTE_BORDER_OK: f32 = 0.35;
const FINE_FONT: f32 = 13.0;
const LINK_RING: f32 = 2.0;
const LINK_RADIUS: f32 = 3.0;
const DISABLED_OPACITY: f32 = 0.5;
const COLOR: Spec = Spec::new(BASE, EASE_OUT);
const ARRIVE: Spec = Spec::new(BASE, EASE_OUT);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Hint,
    Ok,
    Bad,
    Busy,
}

impl Tone {
    pub const ALL: [Self; 4] = [Self::Hint, Self::Ok, Self::Bad, Self::Busy];

    pub fn color(self) -> ColorToken {
        match self {
            Self::Ok => ColorToken::Success700,
            Self::Bad => ColorToken::Danger700,
            Self::Hint | Self::Busy => ColorToken::FgMuted,
        }
    }

    pub fn emphasised(self) -> bool {
        matches!(self, Self::Ok | Self::Bad)
    }

    pub fn role(self) -> Role {
        match self {
            Self::Bad => Role::Alert,
            _ => Role::Status,
        }
    }
}

pub fn status_height() -> f32 {
    STATUS_FONT * STATUS_LINE * STATUS_LINES
}

#[derive(Clone)]
struct Arrival {
    text: SharedString,
    rise: MotionHandle,
}

#[derive(IntoElement)]
pub struct Status {
    id: ElementId,
    tone: Tone,
    text: SharedString,
    theme: Option<Theme>,
}

impl Status {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            tone: Tone::default(),
            text: SharedString::default(),
            theme: None,
        }
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub fn text(mut self, text: impl Into<SharedString>) -> Self {
        self.text = text.into();
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    fn child_id(&self, name: &'static str) -> ElementId {
        ElementId::NamedChild(self.id.clone().into(), name.into())
    }
}

impl RenderOnce for Status {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let color = ease_color(
            self.child_id("ink"),
            theme.color(self.tone.color()),
            COLOR,
            window,
            cx,
        );
        let text = self.text.clone();
        let arrival = window.use_keyed_state(self.child_id("arrive"), cx, |_, _| Arrival {
            text: text.clone(),
            rise: MotionHandle::at(1.0),
        });
        let changed = arrival.read(cx).text != text;
        if changed {
            arrival.update(cx, |arrival, _| {
                arrival.text = text.clone();
                arrival.rise.jump(0.0);
            });
        }
        let rise = arrival.read(cx).rise.clone();
        let shown = if text.is_empty() {
            1.0
        } else {
            rise.sample(1.0, ARRIVE, window, cx).value.clamp(0.0, 1.0)
        };
        let lifted = if cx.reduce_motion() { 0.0 } else { RISE };
        let mark = match self.tone {
            Tone::Bad => {
                Some(glyph(IconName::CircleExclamation, px(STATUS_ICON), color).into_any_element())
            }
            Tone::Ok => {
                Some(glyph(IconName::CircleCheck, px(STATUS_ICON), color).into_any_element())
            }
            Tone::Busy => Some(
                Spinner::new(self.child_id("spin"), STATUS_ICON, color.opacity(0.85))
                    .period(STATUS_PERIOD)
                    .into_any_element(),
            ),
            Tone::Hint => None,
        };
        div()
            .id(self.id.clone())
            .role(self.tone.role())
            .when(!text.is_empty(), |el| el.aria_label(text.clone()))
            .flex()
            .items_start()
            .gap(px(STATUS_GAP))
            .min_h(px(status_height()))
            .font_family(FONT_SANS.name)
            .text_size(px(STATUS_FONT * FONT_SANS.size_adjust))
            .line_height(relative(STATUS_LINE))
            .text_color(color)
            .when(self.tone.emphasised(), |el| {
                el.font_weight(FontWeight::MEDIUM)
            })
            .when(!text.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(STATUS_GAP))
                        .relative()
                        .top(px(lifted * (1.0 - shown)))
                        .opacity(shown)
                        .children(mark.map(|mark| div().flex_none().mt(px(2.0)).child(mark)))
                        .child(div().flex_1().min_w_0().child(text)),
                )
            })
    }
}

type Activate = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Link {
    id: ElementId,
    label: SharedString,
    muted: bool,
    disabled: bool,
    theme: Option<Theme>,
    on_click: Option<Activate>,
}

impl Link {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            muted: false,
            disabled: false,
            theme: None,
            on_click: None,
        }
    }

    pub fn muted(mut self) -> Self {
        self.muted = true;
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

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Link {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let focus: FocusHandle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let ring = !self.disabled && focus.is_focused(window) && window.last_input_was_keyboard();
        let ink = theme.color(if self.muted {
            ColorToken::FgMuted
        } else {
            ColorToken::Link
        });
        let on_click = self.on_click.filter(|_| !self.disabled);
        LinkRoot::new(self.id)
            .disabled(self.disabled)
            .accessibility_label(self.label.clone())
            .when_some(on_click, |link, on_click| {
                link.on_activate(move |event, window, cx| on_click(event, window, cx))
            })
            .relative()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(ink)
            .when(self.disabled, |link| link.opacity(DISABLED_OPACITY))
            .when(!self.disabled, |link| {
                link.cursor_pointer().hover(|style| style.underline())
            })
            .child(self.label)
            .when(ring, |link| {
                link.child(
                    div()
                        .absolute()
                        .top(px(-LINK_RING))
                        .left(px(-LINK_RING))
                        .right(px(-LINK_RING))
                        .bottom(px(-LINK_RING))
                        .rounded(px(LINK_RADIUS))
                        .border(px(LINK_RING))
                        .border_color(theme.color(ColorToken::FocusRing)),
                )
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoteKind {
    Plain,
    Ok,
    Error,
}

#[derive(IntoElement)]
pub struct Note {
    id: ElementId,
    kind: NoteKind,
    children: Vec<AnyElement>,
    theme: Option<Theme>,
}

impl Note {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            kind: NoteKind::Plain,
            children: Vec::new(),
            theme: None,
        }
    }

    pub fn error(id: impl Into<ElementId>) -> Self {
        Self {
            kind: NoteKind::Error,
            ..Self::new(id)
        }
    }

    pub fn ok(mut self) -> Self {
        self.kind = NoteKind::Ok;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl ParentElement for Note {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Note {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let (bg, border, fg, line) = match self.kind {
            NoteKind::Plain => (
                theme.color(ColorToken::BgRaised),
                theme.color(ColorToken::BorderSoft),
                theme.color(ColorToken::FgMuted),
                1.5,
            ),
            NoteKind::Ok => {
                let edge = theme.color(ColorToken::Success500);
                (
                    theme.color(ColorToken::Success100),
                    edge.alpha(edge.a * NOTE_BORDER_OK),
                    theme.color(ColorToken::Success700),
                    1.5,
                )
            }
            NoteKind::Error => {
                let edge = theme.color(ColorToken::Danger500);
                (
                    theme.color(ColorToken::Danger100),
                    edge.alpha(edge.a * NOTE_BORDER_ERROR),
                    theme.color(ColorToken::Danger700),
                    1.45,
                )
            }
        };
        div()
            .id(self.id)
            .when(self.kind == NoteKind::Error, |el| el.role(Role::Alert))
            .flex()
            .items_start()
            .gap(px(NOTE_GAP))
            .px(px(NOTE_PAD_X))
            .py(px(NOTE_PAD_Y))
            .rounded(px(RADIUS_LG))
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_color(fg)
            .font_family(FONT_SANS.name)
            .text_size(px(NOTE_FONT * FONT_SANS.size_adjust))
            .line_height(relative(line))
            .when(self.kind == NoteKind::Error, |el| {
                el.child(div().flex_none().mt(px(2.0)).child(glyph(
                    IconName::CircleExclamation,
                    px(NOTE_ICON),
                    fg,
                )))
            })
            .child(div().flex_1().min_w_0().children(self.children))
    }
}

#[derive(IntoElement)]
pub struct Fine {
    children: Vec<AnyElement>,
    centered: bool,
    theme: Option<Theme>,
}

impl Fine {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            centered: false,
            theme: None,
        }
    }

    pub fn center(mut self) -> Self {
        self.centered = true;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl Default for Fine {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for Fine {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Fine {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        div()
            .font_family(FONT_SANS.name)
            .text_size(px(FINE_FONT * FONT_SANS.size_adjust))
            .line_height(relative(1.5))
            .text_color(theme.color(ColorToken::FgFaint))
            .when(self.centered, |el| el.text_center())
            .children(self.children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slot_reserves_two_lines() {
        assert!((status_height() - 37.7).abs() < 0.01);
    }

    #[test]
    fn only_a_failure_interrupts() {
        for tone in Tone::ALL {
            let want = if tone == Tone::Bad {
                Role::Alert
            } else {
                Role::Status
            };
            assert_eq!(tone.role(), want, "{tone:?}");
        }
    }
}
