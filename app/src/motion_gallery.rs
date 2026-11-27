use std::collections::VecDeque;
use std::time::{Duration, Instant};

use gpui_kit::base::input::{Input, InputEvent, InputState};
use gpui_kit::base::{ReduceMotionPreference, set_reduce_motion_preference};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Div, ElementId, Entity, Hsla, InteractiveElement,
    IntoElement, ParentElement, Render, Rgba, SharedString, StatefulInteractiveElement, Styled,
    Subscription, WeakEntity, Window, div, linear_color_stop, linear_gradient, px, relative,
};
use thelemail_ui::motion::{
    Look, Moving, NAMED, ROW_ARRIVE, SHIMMER_PERIOD, height_auto, shimmer, unfold,
};
use thelemail_ui::text::TextTokens;
use thelemail_ui::theme::{ActiveTokens, Theme};
use thelemail_ui::tokens::{ColorToken, RADIUS_MD, RADIUS_SM, ShadowToken, T_LABEL, T_SMALL};

use crate::gallery::{caption, frame, section, segmented};
use crate::router::GalleryTab;

const MISSED: Duration = Duration::from_micros(12_500);
const IDLE_GAP: Duration = Duration::from_millis(50);
const METER_WINDOW: usize = 240;
const STAGE_W: f32 = 232.0;
const STAGE_H: f32 = 112.0;

#[derive(Clone, Copy)]
struct Play {
    open: bool,
    run: usize,
}

#[derive(Default)]
struct FrameMeter {
    last: Option<Instant>,
    intervals: VecDeque<Duration>,
    frames: u64,
}

impl FrameMeter {
    fn tick(&mut self, now: Instant) {
        if let Some(last) = self.last.replace(now) {
            let gap = now.saturating_duration_since(last);
            if gap < IDLE_GAP {
                self.intervals.push_back(gap);
                self.frames += 1;
                if self.intervals.len() > METER_WINDOW {
                    self.intervals.pop_front();
                }
                if self.frames.is_multiple_of(METER_WINDOW as u64) {
                    tracing::info!(target: "motion", "{}", self.summary());
                }
            }
        }
    }

    fn summary(&self) -> String {
        if self.intervals.is_empty() {
            return "no animation frames yet".into();
        }
        let total: Duration = self.intervals.iter().sum();
        let worst = self.intervals.iter().max().copied().unwrap_or_default();
        let over = self.intervals.iter().filter(|gap| **gap > MISSED).count();
        format!(
            "last {} frames: mean {:.2} ms, worst {:.2} ms, missed: {}",
            self.intervals.len(),
            total.as_secs_f64() * 1000.0 / self.intervals.len() as f64,
            worst.as_secs_f64() * 1000.0,
            over
        )
    }
}

pub struct MotionGallery {
    meter: FrameMeter,
    plays: Vec<Play>,
    unfold_open: bool,
    height_run: usize,
    rows: Vec<usize>,
    next_row: usize,
    reduce: ReduceMotionPreference,
    input: Entity<InputState>,
    typed: SharedString,
    _subscriptions: Vec<Subscription>,
}

impl MotionGallery {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Type here"));
        let subscription =
            cx.subscribe_in(&input, window, |this, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.typed = input.read(cx).value();
                    cx.notify();
                }
            });
        Self {
            meter: FrameMeter::default(),
            plays: NAMED.iter().map(|_| Play { open: true, run: 0 }).collect(),
            unfold_open: false,
            height_run: 0,
            rows: vec![2, 1, 0],
            next_row: 3,
            reduce: ReduceMotionPreference::System,
            input,
            typed: SharedString::default(),
            _subscriptions: vec![subscription],
        }
    }
}

fn act(
    this: &WeakEntity<MotionGallery>,
    f: impl Fn(&mut MotionGallery) + 'static,
) -> impl Fn(&mut App) + 'static {
    let this = this.clone();
    move |cx: &mut App| {
        this.update(cx, |this, cx| {
            f(this);
            cx.notify();
        })
        .ok();
    }
}

fn chip(
    id: impl Into<ElementId>,
    label: &'static str,
    theme: &Theme,
    on: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px(px(10.0))
        .py(px(3.0))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(theme.color(ColorToken::BorderControl))
        .bg(theme.color(ColorToken::Surface))
        .cursor_pointer()
        .text_token(&T_LABEL, theme)
        .child(label)
        .on_click(move |_, _, cx| on(cx))
}

fn stage(theme: &Theme) -> Div {
    div()
        .relative()
        .w(px(STAGE_W))
        .h(px(STAGE_H))
        .overflow_hidden()
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(theme.color(ColorToken::BorderSoft))
        .bg(theme.color(ColorToken::BgSunken))
        .flex()
        .items_center()
        .justify_center()
}

fn card(theme: &Theme) -> Div {
    div()
        .w(px(132.0))
        .h(px(64.0))
        .rounded(px(RADIUS_MD))
        .bg(theme.color(ColorToken::Surface))
        .shadow(theme.shadow(ShadowToken::Md))
}

impl Render for MotionGallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.meter.tick(cx.background_executor().now());
        let current = *cx.tokens();
        let this = cx.entity().downgrade();
        frame(GalleryTab::Motion, &current, |theme| {
            self.column(theme, &this, window, cx)
        })
    }
}

impl MotionGallery {
    fn column(
        &self,
        theme: &Theme,
        this: &WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let side: SharedString = format!("{:?}", theme.appearance).into();
        div()
            .flex()
            .flex_col()
            .gap(px(28.0))
            .child(self.controls(theme, this, cx))
            .child(section(
                "Named motions",
                self.named(theme, &side, this, window, cx),
                theme,
            ))
            .child(section(
                "Row arrive",
                self.rows(theme, &side, this, window, cx),
                theme,
            ))
            .child(section(
                "Unfold and height",
                self.reveals(theme, &side, this, window, cx),
                theme,
            ))
            .child(section(
                "Skeleton shimmer",
                self.shimmer(theme, &side, window, cx),
                theme,
            ))
            .child(section("Typing", self.typing(theme), theme))
            .into_any_element()
    }

    fn controls(&self, theme: &Theme, this: &WeakEntity<Self>, cx: &App) -> impl IntoElement {
        let choices = [
            (ReduceMotionPreference::System, "System"),
            (ReduceMotionPreference::Reduce, "Reduce"),
            (ReduceMotionPreference::Allow, "Allow"),
        ];
        let reduce = self.reduce;
        let this = this.clone();
        let row = div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(div().text_token(&T_SMALL, theme).child("Reduced motion"))
            .child(segmented(
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
                if cx.reduce_motion() {
                    "reducing"
                } else {
                    "moving"
                }
                .to_string(),
                theme,
            ));
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(row)
            .child(caption(self.meter.summary(), theme))
    }

    fn named(
        &self,
        theme: &Theme,
        side: &SharedString,
        this: &WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap(px(16.0))
            .children(NAMED.iter().enumerate().map(|(index, (name, toggle))| {
                let play = self.plays[index];
                let id = ElementId::Name(format!("{side}-{name}-{}", play.run).into());
                let look = toggle.sample(id, play.open, window, cx);
                let flip = act(this, move |g| g.plays[index].open = !g.plays[index].open);
                let replay = act(this, move |g| {
                    g.plays[index].run += 1;
                    g.plays[index].open = true;
                });
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(stage(theme).child(sample(name, &look, theme)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(caption(name.to_string(), theme))
                            .child(div().flex_1())
                            .child(chip(
                                ElementId::Name(format!("{side}-{name}-flip").into()),
                                if play.open { "Reverse" } else { "Play" },
                                theme,
                                flip,
                            ))
                            .child(chip(
                                ElementId::Name(format!("{side}-{name}-replay").into()),
                                "Replay",
                                theme,
                                replay,
                            )),
                    )
            }))
            .into_any_element()
    }

    fn rows(
        &self,
        theme: &Theme,
        side: &SharedString,
        this: &WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pine = theme.color(ColorToken::Pine100);
        let add = act(this, |g| {
            g.rows.insert(0, g.next_row);
            g.next_row += 1;
            g.rows.truncate(5);
        });
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .w(px(360.0))
            .child(div().flex().child(chip(
                ElementId::Name(format!("{side}-row-add").into()),
                "New message",
                theme,
                add,
            )))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(px(RADIUS_MD))
                    .border_1()
                    .border_color(theme.color(ColorToken::Border))
                    .overflow_hidden()
                    .children(self.rows.iter().map(|row| {
                        let id = ElementId::Name(format!("{side}-row-{row}").into());
                        let look = ROW_ARRIVE.sample(id, true, window, cx);
                        let mut flash = pine;
                        flash.a *= look.flash();
                        div()
                            .h(px(44.0))
                            .border_b_1()
                            .border_color(theme.color(ColorToken::Border))
                            .bg(theme.color(ColorToken::Surface))
                            .child(
                                div()
                                    .size_full()
                                    .px(px(14.0))
                                    .flex()
                                    .items_center()
                                    .bg(flash)
                                    .look(&look)
                                    .text_token(&T_SMALL, theme)
                                    .child(format!("Message {row}")),
                            )
                    })),
            )
            .into_any_element()
    }

    fn reveals(
        &self,
        theme: &Theme,
        side: &SharedString,
        this: &WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let note = |text: &'static str| {
            div()
                .p(px(12.0))
                .rounded(px(RADIUS_SM))
                .bg(theme.color(ColorToken::Pine100))
                .text_token(&T_SMALL, theme)
                .child(text)
        };
        let below = || {
            div()
                .h(px(28.0))
                .rounded(px(RADIUS_SM))
                .bg(theme.color(ColorToken::NeutralFill))
        };
        let folded = unfold(
            ElementId::Name(format!("{side}-unfold").into()),
            self.unfold_open,
            div().pb(px(8.0)).child(note(
                "This note unfolds in. The block below slides down with it instead of jumping.",
            )),
            window,
            cx,
        );
        let grown = height_auto(
            ElementId::Name(format!("{side}-height-{}", self.height_run).into()),
            div()
                .pb(px(8.0))
                .child(note("Content that appears once grows its height in.")),
            window,
            cx,
        );
        let flip = act(this, |g| g.unfold_open = !g.unfold_open);
        let again = act(this, |g| g.height_run += 1);
        div()
            .flex()
            .gap(px(24.0))
            .child(
                div()
                    .w(px(260.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(div().flex().child(chip(
                        ElementId::Name(format!("{side}-unfold-flip").into()),
                        if self.unfold_open { "Fold" } else { "Unfold" },
                        theme,
                        flip,
                    )))
                    .children(folded)
                    .child(below()),
            )
            .child(
                div()
                    .w(px(260.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(div().flex().child(chip(
                        ElementId::Name(format!("{side}-height-again").into()),
                        "Replay",
                        theme,
                        again,
                    )))
                    .child(grown)
                    .child(below()),
            )
            .into_any_element()
    }

    fn shimmer(
        &self,
        theme: &Theme,
        side: &SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let phase = shimmer(
            ElementId::Name(format!("{side}-shimmer").into()),
            window,
            cx,
        );
        let moving = !cx.reduce_motion();
        let base = theme.color(ColorToken::Skeleton);
        let light = mix(base, theme.color(ColorToken::Surface), 0.45);
        let mut clear = light;
        clear.a = 0.0;
        let bar = |width: f32, key: &str| {
            let sweep = div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(phase * 2.0 - 1.0))
                .w(relative(1.0))
                .flex()
                .child(div().flex_1().bg(linear_gradient(
                    90.0,
                    linear_color_stop(clear, 0.0),
                    linear_color_stop(light, 1.0),
                )))
                .child(div().flex_1().bg(linear_gradient(
                    90.0,
                    linear_color_stop(light, 0.0),
                    linear_color_stop(clear, 1.0),
                )));
            div()
                .id(ElementId::Name(format!("{side}-bar-{key}").into()))
                .relative()
                .overflow_hidden()
                .w(px(width))
                .h(px(10.0))
                .rounded(px(5.0))
                .bg(base)
                .when_moving(moving, sweep)
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(bar(280.0, "a"))
            .child(bar(220.0, "b"))
            .child(bar(250.0, "c"))
            .child(caption(
                format!("{} ms linear loop", SHIMMER_PERIOD.as_millis()),
                theme,
            ))
            .into_any_element()
    }

    fn typing(&self, theme: &Theme) -> AnyElement {
        let count = self.typed.chars().count();
        let status = match count {
            0 => String::new(),
            n if n < 8 => format!("{n} characters, keep going"),
            n => format!("{n} characters, that is enough"),
        };
        div()
            .w(px(320.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .h(px(36.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded(px(RADIUS_SM))
                    .border_1()
                    .border_color(theme.color(ColorToken::BorderControl))
                    .bg(theme.color(ColorToken::Surface))
                    .text_color(theme.color(ColorToken::Fg))
                    .child(Input::new(&self.input)),
            )
            .child(div().h(px(36.0)).text_token(&T_SMALL, theme).child(status))
            .child(
                div()
                    .h(px(28.0))
                    .rounded(px(RADIUS_SM))
                    .bg(theme.color(ColorToken::NeutralFill)),
            )
            .into_any_element()
    }
}

trait WhenMoving: ParentElement + Sized {
    fn when_moving(self, moving: bool, child: impl IntoElement) -> Self {
        if moving { self.child(child) } else { self }
    }
}

impl<E: ParentElement> WhenMoving for E {}

fn sample(name: &str, look: &Look, theme: &Theme) -> AnyElement {
    if !look.present {
        return div().into_any_element();
    }
    match name {
        "overlay" => div()
            .absolute()
            .inset_0()
            .bg(theme.color(ColorToken::Scrim))
            .opacity(look.opacity)
            .into_any_element(),
        "sheet" => div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .justify_end()
            .child(
                div()
                    .w_full()
                    .h(px(72.0))
                    .rounded_t(px(RADIUS_MD))
                    .bg(theme.color(ColorToken::Surface))
                    .shadow(theme.shadow(ShadowToken::Lg))
                    .look_within(look, px(72.0)),
            )
            .into_any_element(),
        "tooltip" => div()
            .px(px(10.0))
            .py(px(7.0))
            .rounded(px(RADIUS_MD))
            .bg(theme.color(ColorToken::BgInverse))
            .text_color(theme.color(ColorToken::FgOnPine))
            .text_size(px(12.5))
            .child("Archive")
            .look(look)
            .into_any_element(),
        "row_arrive" => {
            let mut flash = theme.color(ColorToken::Pine100);
            flash.a *= look.flash();
            div()
                .w(px(200.0))
                .h(px(40.0))
                .rounded(px(RADIUS_SM))
                .bg(theme.color(ColorToken::Surface))
                .child(
                    div()
                        .size_full()
                        .rounded(px(RADIUS_SM))
                        .bg(flash)
                        .look(look),
                )
                .into_any_element()
        }
        _ => card(theme).look(look).into_any_element(),
    }
}

fn mix(from: Hsla, to: Hsla, amount: f32) -> Hsla {
    let (a, b) = (from.to_rgb(), to.to_rgb());
    let lerp = |x: f32, y: f32| x + (y - x) * amount;
    Rgba {
        r: lerp(a.r, b.r),
        g: lerp(a.g, b.g),
        b: lerp(a.b, b.b),
        a: lerp(a.a, b.a),
    }
    .into()
}
