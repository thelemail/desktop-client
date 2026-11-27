use gpui_kit::{
    AnyElement, AppContext, Context, ElementId, Entity, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, WeakEntity, Window, div, px,
};
use thelemail_ui::auth::{CodeCells, CodeEvent, CodeState, DoneMark, StepPills, WorkSteps};
use thelemail_ui::brand::{Logo, Mark, Wordmark};
use thelemail_ui::button::{Button, Size, Variant};
use thelemail_ui::theme::{ActiveTokens, Appearance, Theme};
use thelemail_ui::tokens::{ColorToken, RADIUS_LG};

use crate::gallery::{caption, frame, section};
use crate::language::Language;
use crate::router::GalleryTab;

const STEPS: usize = 4;
const CARD: f32 = 420.0;
const WORK: [&str; 4] = [
    "Creating your keys",
    "Encrypting your vault",
    "Publishing your address",
    "Opening your mailbox",
];

pub struct BrandGallery {
    step: usize,
    fraction: f32,
    done_runs: usize,
    codes: [Entity<CodeState>; 2],
    values: [SharedString; 2],
    invalid: bool,
    entered: Option<SharedString>,
    work_at: usize,
    work_failed: bool,
    _subscriptions: Vec<Subscription>,
}

impl BrandGallery {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let codes = [
            cx.new(|cx| CodeState::new(window, cx)),
            cx.new(|cx| CodeState::new(window, cx)),
        ];
        let subscriptions = codes
            .iter()
            .enumerate()
            .map(|(slot, code)| {
                cx.subscribe(code, move |this, code, event, cx| {
                    let value = code.read(cx).value(cx);
                    match event {
                        CodeEvent::Edit => {
                            this.invalid = false;
                            this.entered = None;
                        }
                        CodeEvent::Enter => this.entered = Some(value.clone()),
                    }
                    this.values[slot] = value;
                    cx.notify();
                })
            })
            .collect();
        Self {
            step: 1,
            fraction: 1.0,
            done_runs: 0,
            codes,
            values: Default::default(),
            invalid: false,
            entered: None,
            work_at: 1,
            work_failed: false,
            _subscriptions: subscriptions,
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
    ElementId::Name(format!("{}-brand-{}", side(theme), name.into()).into())
}

fn row() -> gpui_kit::Div {
    div().flex().flex_wrap().items_center().gap(px(12.0))
}

fn card(theme: &Theme) -> gpui_kit::Div {
    div()
        .w(px(CARD))
        .p(px(28.0))
        .flex()
        .flex_col()
        .gap(px(20.0))
        .rounded(px(RADIUS_LG))
        .border_1()
        .border_color(theme.color(ColorToken::BorderSoft))
        .bg(theme.color(ColorToken::Surface))
}

fn button(
    theme: &Theme,
    this: &WeakEntity<BrandGallery>,
    name: &'static str,
    label: &'static str,
    act: impl Fn(&mut BrandGallery) + 'static,
) -> Button {
    let this = this.clone();
    Button::new(id(theme, name))
        .size(Size::Sm)
        .variant(Variant::Neutral)
        .label(label)
        .theme(*theme)
        .on_click(move |_, _, cx| {
            this.update(cx, |this, cx| {
                act(this);
                cx.notify();
            })
            .ok();
        })
}

impl Render for BrandGallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = *cx.tokens();
        let language = *cx.global::<Language>();
        let this = cx.entity().downgrade();
        frame(GalleryTab::Brand, &current, language, |theme| {
            self.column(theme, &this)
        })
    }
}

impl BrandGallery {
    fn column(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let theme = *theme;
        div()
            .flex()
            .flex_col()
            .gap(px(28.0))
            .child(section(
                "Logo, mark and wordmark",
                self.brand(&theme),
                &theme,
            ))
            .child(section("Step pills", self.steps(&theme, this), &theme))
            .child(section("Done mark", self.done(&theme, this), &theme))
            .child(section("Code cells", self.code(&theme, this), &theme))
            .child(section("Work steps", self.work(&theme, this), &theme))
            .into_any_element()
    }

    fn brand(&self, theme: &Theme) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(
                row()
                    .child(Logo::new(id(theme, "logo")).theme(*theme))
                    .child(Logo::new(id(theme, "logo-24")).height(24.0).theme(*theme)),
            )
            .child(
                row()
                    .child(Mark::new(id(theme, "mark")).theme(*theme))
                    .child(
                        Mark::new(id(theme, "mark-pine"))
                            .size(48.0)
                            .color(ColorToken::Primary)
                            .theme(*theme),
                    ),
            )
            .child(
                row()
                    .child(Wordmark::new(id(theme, "wordmark")).theme(*theme))
                    .child(
                        Wordmark::new(id(theme, "wordmark-large"))
                            .large()
                            .theme(*theme),
                    ),
            )
            .into_any_element()
    }

    fn steps(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                card(theme).child(
                    StepPills::new(id(theme, "pills"), STEPS, self.step)
                        .fraction(self.fraction)
                        .theme(*theme),
                ),
            )
            .child(
                row()
                    .child(button(theme, this, "back", "Back", |g| {
                        g.step = g.step.saturating_sub(1);
                        g.fraction = 1.0;
                    }))
                    .child(button(theme, this, "next", "Next", |g| {
                        g.step = (g.step + 1).min(STEPS);
                        g.fraction = 1.0;
                    }))
                    .child(button(theme, this, "third", "30%", |g| g.fraction = 0.3))
                    .child(button(theme, this, "most", "70%", |g| g.fraction = 0.7)),
            )
            .child(caption(
                format!(
                    "step {} of {STEPS}, fraction {:.1}",
                    (self.step + 1).min(STEPS),
                    self.fraction
                ),
                theme,
            ))
            .into_any_element()
    }

    fn done(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        row()
            .child(
                DoneMark::new(id(theme, format!("done-{}", self.done_runs)))
                    .label("Done")
                    .theme(*theme),
            )
            .child(button(theme, this, "replay", "Replay", |g| {
                g.done_runs += 1
            }))
            .into_any_element()
    }

    fn code(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let slot = match theme.appearance {
            Appearance::Light => 0,
            Appearance::Dark => 1,
        };
        let value = self.values[slot].clone();
        let code = self.codes[slot].clone();
        let cells = CodeCells::new(id(theme, "code"), &code, "Verification code")
            .invalid(self.invalid)
            .theme(*theme);
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(card(theme).child(cells))
            .child(
                row()
                    .child(button(theme, this, "invalid", "Mark invalid", |g| {
                        g.invalid = !g.invalid
                    }))
                    .child({
                        let this = this.clone();
                        Button::new(id(theme, "clear"))
                            .size(Size::Sm)
                            .variant(Variant::Neutral)
                            .label("Clear")
                            .theme(*theme)
                            .on_click(move |_, window, cx| {
                                code.update(cx, |code, cx| code.clear(window, cx));
                                this.update(cx, |this, cx| {
                                    this.entered = None;
                                    cx.notify();
                                })
                                .ok();
                            })
                    }),
            )
            .child(caption(
                match &self.entered {
                    Some(code) => format!("entered {code}"),
                    None => format!("value {value:?}: type, paste \"12 34-56\", press Enter"),
                },
                theme,
            ))
            .into_any_element()
    }

    fn work(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                card(theme).child(
                    WorkSteps::new(
                        id(theme, "work"),
                        WORK.iter().map(|l| SharedString::from(*l)).collect(),
                        self.work_at,
                    )
                    .failed(self.work_failed)
                    .theme(*theme),
                ),
            )
            .child(
                row()
                    .child(button(theme, this, "work-step", "Step", |g| {
                        g.work_failed = false;
                        g.work_at = (g.work_at + 1).min(WORK.len());
                    }))
                    .child(button(theme, this, "work-fail", "Fail", |g| {
                        g.work_failed = true
                    }))
                    .child(button(theme, this, "work-reset", "Reset", |g| {
                        g.work_failed = false;
                        g.work_at = 0;
                    })),
            )
            .into_any_element()
    }
}
