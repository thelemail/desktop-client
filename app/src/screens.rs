use gpui_kit::{AnyElement, App, IntoElement, ParentElement, Styled, div};

use crate::router::Route;

pub fn render(route: &Route, cx: &App) -> AnyElement {
    #[cfg(debug_assertions)]
    if let Route::Debug(crate::router::DebugPage::Tokens(tab)) = route {
        return crate::gallery::render(*tab, cx).into_any_element();
    }
    let _ = cx;
    placeholder(route).into_any_element()
}

fn placeholder(route: &Route) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .child(route.path())
}
