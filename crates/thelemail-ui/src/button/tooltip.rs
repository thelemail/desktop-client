use std::time::Duration;

use gpui_kit::{
    AnyView, App, AppContext as _, Context, FontWeight, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, div, px, relative,
};

use crate::motion::{Moving as _, TOOLTIP, ToggleHandle};
use crate::theme::{ActiveTokens, Theme};
use crate::tokens::{ColorToken, FONT_MONO, FONT_SANS, RADIUS_MENU, RADIUS_XS};

pub const TOOLTIP_DELAY: Duration = Duration::from_millis(350);
const KBD_BORDER: f32 = 0.35;

pub struct Tooltip {
    text: SharedString,
    shortcut: Option<SharedString>,
    theme: Option<Theme>,
    motion: ToggleHandle,
}

impl Tooltip {
    pub fn build(
        text: SharedString,
        shortcut: Option<SharedString>,
        theme: Option<Theme>,
        _: &mut Window,
        cx: &mut App,
    ) -> AnyView {
        cx.new(|_| Self {
            text,
            shortcut,
            theme,
            motion: ToggleHandle::new(&TOOLTIP),
        })
        .into()
    }
}

impl Render for Tooltip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let look = TOOLTIP.sample_handle(&self.motion, true, window, cx);
        let fg = theme.color(ColorToken::FgOnPine);
        div().pt(px(6.0)).child(
            div()
                .look(&look)
                .flex()
                .items_center()
                .gap(px(8.0))
                .max_w(px(280.0))
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(RADIUS_MENU))
                .bg(theme.color(ColorToken::BgInverse))
                .text_color(fg)
                .font_family(FONT_SANS.name)
                .text_size(px(12.5 * FONT_SANS.size_adjust))
                .font_weight(FontWeight::MEDIUM)
                .line_height(relative(1.3))
                .child(self.text.clone())
                .children(self.shortcut.clone().map(|keys| {
                    div()
                        .px(px(4.0))
                        .py(px(2.0))
                        .border_1()
                        .border_color(fg.alpha(KBD_BORDER))
                        .rounded(px(RADIUS_XS))
                        .font_family(FONT_MONO.name)
                        .text_size(px(11.0 * FONT_MONO.size_adjust))
                        .line_height(relative(1.0))
                        .child(keys)
                })),
        )
    }
}
