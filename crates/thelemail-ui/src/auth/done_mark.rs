use std::time::Duration;

use gpui_kit::base::{Keyframe, Keyframes, Timing, animate_keyframes};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ElementId, InteractiveElement, IntoElement, LineCap, LineJoin, ParentElement, PathBuilder,
    PathStyle, Pixels, Point, RenderOnce, Role, StatefulInteractiveElement, StrokeOptions, Styled,
    Window, canvas, div, point, px,
};

use crate::motion::EASE_OUT;
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, RADIUS_PILL};

const DISC: f32 = 64.0;
const CHECK: f32 = 30.0;
const VIEW: f32 = 24.0;
const STROKE: f32 = 2.4;
const CHECK_POINTS: [(f32, f32); 3] = [(5.0, 12.5), (9.5, 17.0), (19.0, 7.5)];
const DASH: f32 = 24.0;
const POP: Duration = Duration::from_millis(320);
const POP_FROM: f32 = 0.8;
const DRAW_DELAY: Duration = Duration::from_millis(200);
const DRAW: Duration = Duration::from_millis(420);

fn length(points: &[(f32, f32)]) -> f32 {
    points
        .windows(2)
        .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
        .sum()
}

pub(crate) fn drawn(points: &[(f32, f32)], progress: f32) -> Vec<(f32, f32)> {
    let total = length(points);
    let mut left = (progress.clamp(0.0, 1.0) * DASH).min(total);
    let mut out = vec![points[0]];
    for w in points.windows(2) {
        let seg = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
        if left <= 0.0 {
            break;
        }
        if left >= seg {
            out.push(w[1]);
            left -= seg;
        } else {
            let t = left / seg;
            out.push((
                w[0].0 + (w[1].0 - w[0].0) * t,
                w[0].1 + (w[1].1 - w[0].1) * t,
            ));
            break;
        }
    }
    out
}

#[derive(IntoElement)]
pub struct DoneMark {
    id: ElementId,
    label: Option<gpui_kit::SharedString>,
    theme: Option<Theme>,
}

impl DoneMark {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            theme: None,
        }
    }

    pub fn label(mut self, label: impl Into<gpui_kit::SharedString>) -> Self {
        self.label = Some(label.into());
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

fn play(id: ElementId, delay: Duration, run: Duration, window: &mut Window, cx: &mut App) -> f32 {
    let total = delay + run;
    let hold = delay.as_secs_f32() / total.as_secs_f32();
    let keyframes = Keyframes::try_new([
        Keyframe::new(0.0, 0.0_f32),
        Keyframe::new(hold, 0.0).ease(EASE_OUT.easing()),
        Keyframe::new(1.0, 1.0),
    ])
    .expect("ordered keyframes");
    animate_keyframes(id, &keyframes, Timing::new(total), window, cx).value
}

impl RenderOnce for DoneMark {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let pop = play(self.child_id("pop"), Duration::ZERO, POP, window, cx);
        let draw = play(self.child_id("draw"), DRAW_DELAY, DRAW, window, cx);
        let disc = DISC * (POP_FROM + (1.0 - POP_FROM) * pop);
        let ink = theme.color(ColorToken::Primary);
        let line = drawn(&CHECK_POINTS, draw);
        let check = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                if line.len() < 2 {
                    return;
                }
                let scale = f32::from(bounds.size.width) / VIEW;
                let at = |(x, y): (f32, f32)| -> Point<Pixels> {
                    point(
                        bounds.origin.x + px(x * scale),
                        bounds.origin.y + px(y * scale),
                    )
                };
                let mut path = PathBuilder::stroke(px(STROKE * scale)).with_style(
                    PathStyle::Stroke(StrokeOptions {
                        line_width: STROKE * scale,
                        line_cap: LineCap::Round,
                        line_join: LineJoin::Round,
                        ..Default::default()
                    }),
                );
                path.move_to(at(line[0]));
                for p in &line[1..] {
                    path.line_to(at(*p));
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, ink);
                }
            },
        )
        .size(px(CHECK));
        div()
            .id(self.id.clone())
            .when_some(self.label.clone(), |el, label| {
                el.role(Role::Image).aria_label(label)
            })
            .flex_none()
            .size(px(DISC))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex_none()
                    .size(px(disc))
                    .rounded(px(RADIUS_PILL))
                    .bg(theme.color(ColorToken::Pine100))
                    .opacity(pop)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(check),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_drawn_before_the_stroke_starts() {
        assert_eq!(drawn(&CHECK_POINTS, 0.0), vec![CHECK_POINTS[0]]);
    }

    #[test]
    fn the_check_completes_before_the_dash_runs_out() {
        let full = length(&CHECK_POINTS);
        assert!(full < DASH);
        assert_eq!(drawn(&CHECK_POINTS, full / DASH), CHECK_POINTS.to_vec());
        assert_eq!(drawn(&CHECK_POINTS, 1.0), CHECK_POINTS.to_vec());
    }

    #[test]
    fn partway_ends_on_the_first_stroke() {
        let line = drawn(&CHECK_POINTS, 0.1);
        assert_eq!(line.len(), 2);
        assert!(line[1].0 > 5.0 && line[1].0 < 9.5);
    }
}
