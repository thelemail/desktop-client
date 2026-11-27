use gpui_kit::{IntoElement, ParentElement, Styled, div};

use crate::router::Route;

pub fn placeholder(route: &Route) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .child(route.path())
}
