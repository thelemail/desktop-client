use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Window, div, px,
};
use thelemail_ui::feedback::{Fine, Link, Note, Status, Tone};
use thelemail_ui::field::{
    FieldLabel, FieldSize, PasswordField, Select, SelectChanged, SelectOption, TextArea, TextField,
};
use thelemail_ui::theme::{ActiveTokens, Theme};

use crate::gallery::{caption, frame_in, section};
use crate::language::Language;
use crate::router::GalleryTab;

const WIDTH: f32 = 360.0;

pub struct FieldGallery {
    sizes: [Entity<InputState>; 3],
    invalid: Entity<InputState>,
    disabled: Entity<InputState>,
    read_only: Entity<InputState>,
    suffixed: Entity<InputState>,
    handle: Entity<InputState>,
    area: Entity<TextareaState>,
    password: Entity<PasswordField>,
    select: Entity<Select>,
    picked: SharedString,
    handle_tone: Tone,
    handle_text: SharedString,
    _subscriptions: Vec<Subscription>,
}

fn input(
    window: &mut Window,
    cx: &mut Context<FieldGallery>,
    placeholder: &str,
) -> Entity<InputState> {
    let placeholder = SharedString::from(placeholder.to_string());
    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
}

fn judge(handle: &str) -> (Tone, SharedString) {
    let len = handle.chars().count();
    match len {
        0 => (
            Tone::Hint,
            "Letters, digits and dots. You can't change it later.".into(),
        ),
        1..=2 => (Tone::Bad, "Use at least 3 characters.".into()),
        3..=5 => (Tone::Busy, "Checking whether this address is free…".into()),
        _ if handle.contains(' ') => (Tone::Bad, "Spaces aren't allowed in an address.".into()),
        _ => (Tone::Ok, format!("{handle}@thelemail.com is free.").into()),
    }
}

impl FieldGallery {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sizes = [
            input(window, cx, "Small, 32"),
            input(window, cx, "Default, 40"),
            input(window, cx, "Large, 48"),
        ];
        let invalid = input(window, cx, "Invalid");
        let disabled = input(window, cx, "Disabled");
        let read_only =
            cx.new(|cx| InputState::new(window, cx).default_value("orla.finch@thelemail.com"));
        let suffixed = input(window, cx, "orla.finch");
        let handle = input(window, cx, "Choose an address");
        let area = cx.new(|cx| TextareaState::new(window, cx).placeholder("Write a note"));
        let password = cx.new(|cx| {
            PasswordField::new("gallery-password", window, cx)
                .size(FieldSize::Lg)
                .label("Password")
        });
        let select = cx.new(|cx| {
            Select::new(
                "gallery-select",
                vec![
                    SelectOption::new("en", "English"),
                    SelectOption::new("de", "Deutsch"),
                    SelectOption::new("fr", "Français"),
                    SelectOption::new("pt", "Português"),
                ],
                cx,
            )
            .label("Language")
            .placeholder("Choose a language")
        });
        let (handle_tone, handle_text) = judge("");
        let subscriptions = vec![
            cx.subscribe_in(&handle, window, |this, state, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let (tone, text) = judge(&state.read(cx).value());
                    this.handle_tone = tone;
                    this.handle_text = text;
                    cx.notify();
                }
            }),
            cx.subscribe(&select, |this, _, event: &SelectChanged, cx| {
                this.picked = event.value.clone();
                cx.notify();
            }),
        ];
        Self {
            sizes,
            invalid,
            disabled,
            read_only,
            suffixed,
            handle,
            area,
            password,
            select,
            picked: SharedString::default(),
            handle_tone,
            handle_text,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for FieldGallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = *cx.tokens();
        let language = *cx.global::<Language>();
        frame_in(
            GalleryTab::Fields,
            &current,
            language,
            &[current.appearance],
            |theme| self.column(theme),
        )
    }
}

fn stack() -> gpui_kit::Div {
    div().flex().flex_col().gap(px(12.0)).w(px(WIDTH))
}

impl FieldGallery {
    fn column(&self, theme: &Theme) -> AnyElement {
        let theme = *theme;
        div()
            .flex()
            .flex_wrap()
            .gap(px(48.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(28.0))
                    .child(section("Sizes", self.sizes(&theme), &theme))
                    .child(section("States", self.states(&theme), &theme))
                    .child(section("Textarea", self.area(&theme), &theme))
                    .child(section("Select", self.select(&theme), &theme)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(28.0))
                    .child(section("Sign-in fields", self.sign_in(&theme), &theme))
                    .child(section("Status while typing", self.typing(&theme), &theme))
                    .child(section("Status tones", self.tones(&theme), &theme))
                    .child(section("Notes and links", self.notes(&theme), &theme)),
            )
            .into_any_element()
    }

    fn sizes(&self, theme: &Theme) -> AnyElement {
        stack()
            .children(
                FieldSize::ALL
                    .into_iter()
                    .zip(&self.sizes)
                    .map(|(size, state)| {
                        TextField::new(SharedString::from(format!("size-{size:?}")), state)
                            .size(size)
                            .theme(*theme)
                    }),
            )
            .into_any_element()
    }

    fn states(&self, theme: &Theme) -> AnyElement {
        stack()
            .child(
                TextField::new("state-invalid", &self.invalid)
                    .invalid(true)
                    .theme(*theme),
            )
            .child(
                TextField::new("state-disabled", &self.disabled)
                    .disabled(true)
                    .theme(*theme),
            )
            .child(
                TextField::new("state-read-only", &self.read_only)
                    .read_only(true)
                    .theme(*theme),
            )
            .into_any_element()
    }

    fn area(&self, theme: &Theme) -> AnyElement {
        stack()
            .child(
                TextArea::new("area", &self.area)
                    .label("Note")
                    .theme(*theme),
            )
            .into_any_element()
    }

    fn select(&self, theme: &Theme) -> AnyElement {
        let picked = if self.picked.is_empty() {
            "nothing picked".to_string()
        } else {
            format!("picked {}", self.picked)
        };
        stack()
            .child(self.select.clone())
            .child(caption(picked, theme))
            .into_any_element()
    }

    fn sign_in(&self, theme: &Theme) -> AnyElement {
        stack()
            .child(
                FieldLabel::new("Address")
                    .control(
                        TextField::new("suffixed", &self.suffixed)
                            .size(FieldSize::Lg)
                            .mono()
                            .suffix("@thelemail.com")
                            .label("Address")
                            .theme(*theme),
                    )
                    .theme(*theme),
            )
            .child(
                FieldLabel::new("Password")
                    .aux(Link::new("forgot", "Forgot password?").theme(*theme))
                    .control(self.password.clone())
                    .theme(*theme),
            )
            .into_any_element()
    }

    fn typing(&self, theme: &Theme) -> AnyElement {
        stack()
            .child(
                FieldLabel::new("Address")
                    .control(
                        TextField::new("handle", &self.handle)
                            .invalid(self.handle_tone == Tone::Bad)
                            .label("Address")
                            .theme(*theme),
                    )
                    .theme(*theme),
            )
            .child(
                Status::new("handle-status")
                    .tone(self.handle_tone)
                    .text(self.handle_text.clone())
                    .theme(*theme),
            )
            .child(caption("nothing below this line moves".into(), theme))
            .into_any_element()
    }

    fn tones(&self, theme: &Theme) -> AnyElement {
        stack()
            .children(Tone::ALL.into_iter().map(|tone| {
                let text = match tone {
                    Tone::Hint => "We'll send a code to this address.",
                    Tone::Ok => "Recovery phrase saved.",
                    Tone::Bad => "That code didn't match. Check it and try again.",
                    Tone::Busy => "Signing in…",
                };
                Status::new(SharedString::from(format!("tone-{tone:?}")))
                    .tone(tone)
                    .text(text)
                    .theme(*theme)
            }))
            .into_any_element()
    }

    fn notes(&self, theme: &Theme) -> AnyElement {
        stack()
            .child(
                Note::new("note").theme(*theme).child(
                    "Your mail stays encrypted on our servers. Only your devices can read it.",
                ),
            )
            .child(
                Note::new("note-ok")
                    .ok()
                    .theme(*theme)
                    .child("Domain verified. Mail to this domain now arrives here."),
            )
            .child(
                Note::error("note-error")
                    .theme(*theme)
                    .child("We couldn't reach the server. Check your connection."),
            )
            .child(
                Fine::new()
                    .theme(*theme)
                    .child("By continuing you agree to the Terms and the Privacy Policy."),
            )
            .child(
                div()
                    .flex()
                    .gap(px(14.0))
                    .child(Link::new("link", "Create an account").theme(*theme))
                    .child(
                        Link::new("link-muted", "Use a recovery phrase")
                            .muted()
                            .theme(*theme),
                    )
                    .child(
                        Link::new("link-disabled", "Resend code")
                            .disabled(true)
                            .theme(*theme),
                    ),
            )
            .into_any_element()
    }
}
