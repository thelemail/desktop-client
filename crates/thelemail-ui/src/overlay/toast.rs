use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::base::{
    Root, RootPlugin, ToastManager, ToastMotion, ToastOptions, ToastTransitionStatus,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, ElementId, Entity, FocusHandle, FontWeight, Hsla, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Task, Window, accesskit, div, px, relative,
};
use thelemail_i18n::m;

use super::dialog::Action;
use super::layer::{self, Layer};
use crate::a11y::A11yStateExt as _;
use crate::icons::{IconName, glyph};
use crate::motion::{Moving as _, SLOW, TOAST_IN};
use crate::theme::ActiveTokens;
use crate::tokens::{ColorToken, FONT_SANS, RADIUS_MENU, RADIUS_SM, ShadowToken};

pub const TOAST_DEFAULT: Duration = Duration::from_millis(2600);
pub const TOAST_WITH_ACTION: Duration = Duration::from_millis(7000);
pub const TOAST_ERROR: Duration = Duration::from_millis(5000);
pub const TOAST_VISIBLE: usize = 3;
const TICK: Duration = Duration::from_millis(100);
const BOTTOM: f32 = 24.0;
const MAX_WIDTH: f32 = 560.0;
const SIDE_GAP: f32 = 16.0;
const STACK_GAP: f32 = 8.0;
const DISMISS: f32 = 30.0;
const HOVER_FILL: f32 = 0.12;
const DISMISS_FG: f32 = 0.7;
const RING: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastTone {
    #[default]
    Success,
    Info,
    Error,
}

impl ToastTone {
    pub const ALL: [Self; 3] = [Self::Success, Self::Info, Self::Error];

    pub fn icon(self) -> Option<IconName> {
        match self {
            Self::Error => Some(IconName::CircleExclamation),
            Self::Success | Self::Info => None,
        }
    }
}

pub fn duration_for(tone: ToastTone, has_action: bool) -> Duration {
    if has_action {
        TOAST_WITH_ACTION
    } else if tone == ToastTone::Error {
        TOAST_ERROR
    } else {
        TOAST_DEFAULT
    }
}

#[derive(Clone)]
pub struct Toast {
    text: SharedString,
    tone: ToastTone,
    action: Option<(SharedString, Action)>,
    dismissible: bool,
    duration: Option<Option<Duration>>,
}

impl Toast {
    pub fn new(tone: ToastTone, text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            tone,
            action: None,
            dismissible: false,
            duration: None,
        }
    }

    pub fn success(text: impl Into<SharedString>) -> Self {
        Self::new(ToastTone::Success, text)
    }

    pub fn info(text: impl Into<SharedString>) -> Self {
        Self::new(ToastTone::Info, text)
    }

    pub fn error(text: impl Into<SharedString>) -> Self {
        Self::new(ToastTone::Error, text)
    }

    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(handler)));
        self
    }

    pub fn undo(self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.action(m::ui_toast_undo(), handler)
    }

    pub fn dismissible(mut self) -> Self {
        self.dismissible = true;
        self
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(Some(duration));
        self
    }

    pub fn sticky(mut self) -> Self {
        self.duration = Some(None);
        self
    }

    fn timeout(&self) -> Option<Duration> {
        self.duration
            .unwrap_or_else(|| Some(duration_for(self.tone, self.action.is_some())))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToastId(u64);

struct Live {
    toast: Toast,
    action_focus: FocusHandle,
    dismiss_focus: FocusHandle,
}

pub struct Toasts {
    manager: ToastManager<ToastId, Live>,
    next: u64,
    hovered: HashSet<ToastId>,
    ticker: Option<Task<()>>,
}

impl RootPlugin for Toasts {}

fn motion() -> ToastMotion {
    ToastMotion {
        duration: SLOW,
        exit_duration: SLOW,
        ..ToastMotion::sonner()
    }
}

impl Toasts {
    fn new() -> Self {
        Self {
            manager: ToastManager::new(motion()),
            next: 0,
            hovered: HashSet::new(),
            ticker: None,
        }
    }

    fn entity(window: &Window, cx: &App) -> Option<Entity<Self>> {
        window.root::<Root>().flatten()?;
        Root::read(window, cx).plugin::<Self>()
    }

    pub fn push(window: &mut Window, cx: &mut App, toast: Toast) -> Option<ToastId> {
        let toasts = Self::entity(window, cx)?;
        Some(toasts.update(cx, |this, cx| this.add(toast, window, cx)))
    }

    pub fn dismiss(window: &mut Window, cx: &mut App, id: ToastId) {
        if let Some(toasts) = Self::entity(window, cx) {
            toasts.update(cx, |this, cx| {
                if this.manager.dismiss(&id, Instant::now()) {
                    cx.notify();
                }
            });
        }
    }

    fn add(&mut self, toast: Toast, window: &mut Window, cx: &mut Context<Self>) -> ToastId {
        self.next += 1;
        let id = ToastId(self.next);
        let options = ToastOptions {
            timeout: toast.timeout(),
        };
        let live = Live {
            toast,
            action_focus: cx.focus_handle(),
            dismiss_focus: cx.focus_handle(),
        };
        self.manager.push(id, live, options, Instant::now());
        self.tick(window, cx);
        cx.notify();
        id
    }

    fn paused(&self, window: &Window) -> bool {
        !self.hovered.is_empty()
            || self.manager.iter().any(|(_, live, _)| {
                live.action_focus.is_focused(window) || live.dismiss_focus.is_focused(window)
            })
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ticker.is_some() {
            return;
        }
        self.ticker = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let keep = this
                    .update_in(cx, |this, window, cx| {
                        let paused = this.paused(window);
                        let step = this.manager.advance(Instant::now(), paused);
                        for (id, _) in &step.removed {
                            this.hovered.remove(id);
                        }
                        if step.changed {
                            cx.notify();
                        }
                        let keep = !this.manager.is_empty();
                        if !keep {
                            this.ticker = None;
                        }
                        keep
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        }));
    }
}

fn button_look(
    el: gpui_kit::Stateful<gpui_kit::Div>,
    focus: &FocusHandle,
    fg: Hsla,
    ring: Hsla,
    window: &Window,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let keyboard = focus.is_focused(window) && window.last_input_was_keyboard();
    el.track_focus(&focus.clone().tab_index(0))
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_SM))
        .cursor_pointer()
        .hover(move |style| style.bg(fg.alpha(HOVER_FILL)))
        .role(Role::Button)
        .when(keyboard, |el| {
            el.child(
                div()
                    .absolute()
                    .top(px(-1.0 - RING))
                    .left(px(-1.0 - RING))
                    .right(px(-1.0 - RING))
                    .bottom(px(-1.0 - RING))
                    .rounded(px(RADIUS_SM + 1.0 + RING))
                    .border(px(RING))
                    .border_color(ring),
            )
        })
}

impl Render for Toasts {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let stack_id = ElementId::Name("toast-stack".into());
        if self.manager.is_empty() {
            layer::cover(&stack_id, window, cx).clear(window, cx);
            return div().into_any_element();
        }
        let theme = *cx.tokens();
        let viewport = window.viewport_size();
        let fg = theme.color(ColorToken::FgOnPine);
        let brass = theme.color(ColorToken::BrassOnInverse);
        let entity = cx.entity();
        let bars: Vec<_> = self
            .manager
            .visible(TOAST_VISIBLE)
            .map(|(id, live, status)| {
                let toast = &live.toast;
                let key = ElementId::NamedInteger("toast".into(), id.0);
                let look = TOAST_IN.sample(
                    key.clone(),
                    status != ToastTransitionStatus::Ending,
                    window,
                    cx,
                );
                let id = *id;
                let hover_entity = entity.clone();
                let action = toast.action.clone().map(|(label, handler)| {
                    let dismiss_entity = entity.clone();
                    button_look(
                        div().id(ElementId::NamedInteger("toast-action".into(), id.0)),
                        &live.action_focus,
                        fg,
                        brass,
                        window,
                    )
                    .px(px(10.0))
                    .py(px(8.0))
                    .text_color(brass)
                    .text_size(px(13.5 * FONT_SANS.size_adjust))
                    .font_weight(FontWeight::BOLD)
                    .line_height(relative(1.0))
                    .aria_label(label.clone())
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_click(move |_, window, cx| {
                        handler(window, cx);
                        dismiss_entity.update(cx, |this, cx| {
                            this.manager.dismiss(&id, Instant::now());
                            cx.notify();
                        });
                    })
                    .child(label)
                });
                let dismiss = toast.dismissible.then(|| {
                    let dismiss_entity = entity.clone();
                    button_look(
                        div().id(ElementId::NamedInteger("toast-dismiss".into(), id.0)),
                        &live.dismiss_focus,
                        fg,
                        brass,
                        window,
                    )
                    .flex_none()
                    .size(px(DISMISS))
                    .aria_label(m::common_close())
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_click(move |_, _, cx| {
                        dismiss_entity.update(cx, |this, cx| {
                            this.manager.dismiss(&id, Instant::now());
                            cx.notify();
                        });
                    })
                    .child(glyph(
                        IconName::Xmark,
                        px(14.0),
                        fg.alpha(DISMISS_FG),
                    ))
                });
                div()
                    .id(key)
                    .occlude()
                    .look(&look)
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .max_w(px(MAX_WIDTH).min(viewport.width - px(SIDE_GAP * 2.0)))
                    .min_h(px(44.0))
                    .pl(px(14.0))
                    .pr(px(6.0))
                    .py(px(6.0))
                    .rounded(px(RADIUS_MENU))
                    .bg(theme.color(ColorToken::BgInverse))
                    .shadow(theme.shadow(ShadowToken::Lg))
                    .text_color(fg)
                    .font_family(FONT_SANS.name)
                    .text_size(px(14.0 * FONT_SANS.size_adjust))
                    .font_weight(FontWeight::MEDIUM)
                    .line_height(relative(1.35))
                    .role(Role::Status)
                    .aria_label(toast.text.clone())
                    .on_hover(move |hovered, _, cx| {
                        hover_entity.update(cx, |this, _| {
                            if *hovered {
                                this.hovered.insert(id);
                            } else {
                                this.hovered.remove(&id);
                            }
                        });
                    })
                    .when_some(toast.tone.icon(), |el, icon| {
                        el.child(glyph(icon, px(16.0), brass))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .py(px(6.0))
                            .child(toast.text.clone()),
                    )
                    .children(action)
                    .children(dismiss)
                    .a11y_state(false, false)
                    .live(accesskit::Live::Polite)
            })
            .collect();

        let cover = layer::cover(&stack_id, window, cx);
        let stack = div()
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(STACK_GAP))
            .children(bars)
            .child(layer::tracker(cover));
        layer::host(
            Layer::Toast,
            div()
                .w(viewport.width)
                .h(viewport.height)
                .flex()
                .flex_col()
                .items_center()
                .justify_end()
                .pb(px(BOTTOM))
                .child(stack),
        )
    }
}

pub(crate) fn register(cx: &mut App) {
    Root::register_plugin::<Toasts>(cx, |_, _| Toasts::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_follow_the_web() {
        assert_eq!(duration_for(ToastTone::Success, false), TOAST_DEFAULT);
        assert_eq!(duration_for(ToastTone::Info, false), TOAST_DEFAULT);
        assert_eq!(duration_for(ToastTone::Error, false), TOAST_ERROR);
        assert_eq!(duration_for(ToastTone::Error, true), TOAST_WITH_ACTION);
        assert_eq!(duration_for(ToastTone::Success, true), TOAST_WITH_ACTION);
    }

    #[test]
    fn no_tone_ever_shows_a_check() {
        for tone in ToastTone::ALL {
            assert_ne!(tone.icon(), Some(IconName::Check));
            assert_ne!(tone.icon(), Some(IconName::CircleCheck));
        }
        assert_eq!(ToastTone::Error.icon(), Some(IconName::CircleExclamation));
    }

    #[test]
    fn explicit_durations_win() {
        let toast = Toast::success("Saved").duration(Duration::from_millis(4500));
        assert_eq!(toast.timeout(), Some(Duration::from_millis(4500)));
        assert_eq!(Toast::error("Failed").sticky().timeout(), None);
        assert_eq!(
            Toast::success("Archived").undo(|_, _| {}).timeout(),
            Some(TOAST_WITH_ACTION)
        );
    }
}
