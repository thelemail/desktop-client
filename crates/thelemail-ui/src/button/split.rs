use gpui_kit::{
    App, Corners, ElementId, IntoElement, ParentElement, RenderOnce, Styled, Window, div, px,
};

use super::{Button, look::{Flags, Phase, look}};
use crate::theme::{ActiveTokens, Theme};

const DIVIDER: f32 = 0.22;

#[derive(IntoElement)]
pub struct SplitButton {
    id: ElementId,
    main: Button,
    menu: Button,
    theme: Option<Theme>,
}

impl SplitButton {
    pub fn new(id: impl Into<ElementId>, main: Button, menu: Button) -> Self {
        Self {
            id: id.into(),
            main,
            menu,
            theme: None,
        }
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl RenderOnce for SplitButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme.unwrap_or(*cx.tokens());
        let flags = Flags {
            busy: self.main.busy,
            disabled: self.main.disabled,
            inverse: self.main.inverse,
            ..Flags::default()
        };
        let divider = look(self.main.variant, flags, Phase::Rest)
            .fg
            .resolve(&theme);
        let height = px(self.main.size.metrics().control.height);
        let main = self.main.theme(theme).corners(Corners {
            top_left: true,
            bottom_left: true,
            top_right: false,
            bottom_right: false,
        });
        let menu = self.menu.theme(theme).corners(Corners {
            top_left: false,
            bottom_left: false,
            top_right: true,
            bottom_right: true,
        });
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .child(main)
            .child(
                div()
                    .w(px(1.0))
                    .h(height)
                    .bg(divider.alpha(divider.a * DIVIDER)),
            )
            .child(menu)
    }
}
