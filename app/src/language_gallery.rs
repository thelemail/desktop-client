use gpui_kit::{AnyElement, App, Div, IntoElement, ParentElement, SharedString, Styled, div, px};
use thelemail_i18n::{Locale, Rich, m};
use thelemail_ui::rich::Spans;
use thelemail_ui::text::TextTokens;
use thelemail_ui::theme::Theme;
use thelemail_ui::tokens::{
    CONTROL_MD, ColorToken, RADIUS_CONTROL, RADIUS_MD, T_BODY, T_LABEL, T_SMALL,
};

use crate::gallery::{caption, section};

type Counted = fn(u64) -> String;

const COUNTS: [u64; 11] = [0, 1, 2, 3, 5, 11, 21, 22, 23, 101, 1_000_000];
const SIDEBAR: f32 = 220.0;
const MENU: f32 = 240.0;

pub fn render(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(28.0))
        .child(caption(
            format!(
                "{} · {} keys from web-client {}",
                thelemail_i18n::locale().name(),
                m::KEY_COUNT,
                m::SOURCE_TAG
            ),
            theme,
        ))
        .child(section("Buttons", buttons(theme), theme))
        .child(section("Sidebar", sidebar(theme), theme))
        .child(section(
            "Menu bar, tray and notifications",
            chrome(theme),
            theme,
        ))
        .child(section("Rich text", rich(theme), theme))
        .child(section("Selects", selects(theme), theme))
        .child(section("Plurals", plurals(theme), theme))
        .into_any_element()
}

fn plurals(theme: &Theme) -> AnyElement {
    let rows: [(&str, Counted); 5] = [
        ("mail_thread_count", m::mail_thread_count),
        ("mail_list_selected", m::mail_list_selected),
        ("mail_send_err_wait_minutes", m::mail_send_err_wait_minutes),
        ("lc_bytes_bytes", m::lc_bytes_bytes),
        ("cal_task_rollover", m::cal_task_rollover),
    ];
    div()
        .flex()
        .flex_col()
        .gap(px(16.0))
        .children(rows.into_iter().map(|(key, message)| {
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(caption(key.to_string(), theme))
                .children(COUNTS.into_iter().map(|n| {
                    div()
                        .flex()
                        .gap(px(12.0))
                        .child(div().w(px(72.0)).child(caption(n.to_string(), theme)))
                        .child(div().text_token(&T_BODY, theme).child(message(n)))
                }))
        }))
        .into_any_element()
}

fn selects(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .children(
            [
                "accepted",
                "tentative",
                "declined",
                "needs-action",
                "delegated",
            ]
            .into_iter()
            .map(|partstat| {
                div()
                    .flex()
                    .gap(px(12.0))
                    .child(
                        div()
                            .w(px(96.0))
                            .child(caption(partstat.to_string(), theme)),
                    )
                    .child(
                        div()
                            .text_token(&T_BODY, theme)
                            .child(m::cal_op_replied(partstat)),
                    )
            }),
        )
        .into_any_element()
}

fn rich(theme: &Theme) -> AnyElement {
    let samples: [(&str, Rich); 3] = [
        (
            "mail_alerts_dns_detail_more",
            m::mail_alerts_dns_detail_more("<b>mail</b>.example", 2),
        ),
        ("settings_import_resumable", m::settings_import_resumable(3)),
        (
            "lc_deletion_body_days",
            m::lc_deletion_body_days(4, "3 March 2027", "1 April 2027"),
        ),
    ];
    div()
        .flex()
        .flex_col()
        .gap(px(16.0))
        .children(samples.into_iter().map(|(key, rich)| {
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(caption(key.to_string(), theme))
                .child(div().max_w(px(520.0)).text_token(&T_BODY, theme).child(
                    Spans::new(&rich).interactive(key, theme, |tag, _, _| {
                        tracing::info!("gallery: {tag} clicked");
                    }),
                ))
        }))
        .into_any_element()
}

fn button(label: &'static str, primary: bool, theme: &Theme) -> Div {
    let (fill, fg) = if primary {
        (ColorToken::Primary, ColorToken::PrimaryFg)
    } else {
        (ColorToken::NeutralFill, ColorToken::Fg)
    };
    div()
        .flex()
        .items_center()
        .h(px(CONTROL_MD.height))
        .px(px(14.0))
        .rounded(px(RADIUS_CONTROL))
        .bg(theme.color(fill))
        .text_token(&T_LABEL, theme)
        .text_color(theme.color(fg))
        .whitespace_nowrap()
        .child(label)
}

fn buttons(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .gap(px(8.0))
        .child(button(m::mail_update_install(), true, theme))
        .child(button(m::settings_about_check(), false, theme))
        .child(button(m::common_continue(), true, theme))
        .child(button(m::common_cancel(), false, theme))
        .child(button(m::mail_compose_send_archive(), true, theme))
        .child(button(m::common_show_password(), false, theme))
        .into_any_element()
}

fn row(label: impl Into<SharedString>, width: f32, theme: &Theme) -> Div {
    div()
        .w(px(width))
        .h(px(CONTROL_MD.height))
        .flex()
        .items_center()
        .px(px(10.0))
        .rounded(px(RADIUS_MD))
        .overflow_hidden()
        .text_token(&T_SMALL, theme)
        .whitespace_nowrap()
        .text_ellipsis()
        .child(label.into())
}

fn sidebar(theme: &Theme) -> AnyElement {
    let labels = [
        m::mail_folder_inbox(),
        m::mail_folder_starred(),
        m::mailbox_folder_drafts(),
        m::mail_folder_archive(),
        m::mail_folder_spam(),
        m::mail_folder_trash(),
        m::settings_nav_about(),
    ];
    div()
        .flex()
        .flex_col()
        .p(px(6.0))
        .w(px(SIDEBAR + 12.0))
        .rounded(px(RADIUS_MD))
        .bg(theme.color(ColorToken::BgSunken))
        .children(labels.into_iter().map(|label| row(label, SIDEBAR, theme)))
        .into_any_element()
}

fn chrome(theme: &Theme) -> AnyElement {
    let menu = [
        m::desktop_menu_about(),
        m::desktop_menu_settings(),
        m::desktop_menu_services(),
        m::desktop_menu_hide(),
        m::desktop_menu_hide_others(),
        m::desktop_menu_show_all(),
        m::desktop_menu_quit(),
        m::desktop_menu_undo(),
        m::desktop_menu_select_all(),
        m::desktop_menu_full_screen(),
        m::desktop_menu_minimize(),
        m::desktop_menu_zoom(),
        m::desktop_menu_help_app(),
    ];
    let tray = [m::desktop_tray_show(), m::desktop_tray_quit()];
    div()
        .flex()
        .flex_wrap()
        .gap(px(24.0))
        .child(
            div()
                .flex()
                .flex_col()
                .p(px(6.0))
                .rounded(px(RADIUS_MD))
                .bg(theme.color(ColorToken::Surface))
                .children(menu.into_iter().map(|label| row(label, MENU, theme))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(16.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .p(px(6.0))
                        .rounded(px(RADIUS_MD))
                        .bg(theme.color(ColorToken::Surface))
                        .children(tray.into_iter().map(|label| row(label, 180.0, theme))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(300.0))
                        .p(px(12.0))
                        .rounded(px(RADIUS_MD))
                        .bg(theme.color(ColorToken::Surface))
                        .child(
                            div()
                                .text_token(&T_LABEL, theme)
                                .child(m::desktop_notify_new_message()),
                        )
                        .child(
                            div()
                                .text_token(&T_SMALL, theme)
                                .child(m::mail_compose_no_subject()),
                        ),
                ),
        )
        .into_any_element()
}

pub fn choices() -> [(Option<Locale>, &'static str); 5] {
    [
        (None, m::settings_region_language_auto()),
        (Some(Locale::En), Locale::En.name()),
        (Some(Locale::De), Locale::De.name()),
        (Some(Locale::Fr), Locale::Fr.name()),
        (Some(Locale::Pt), Locale::Pt.name()),
    ]
}

pub fn pick(choice: Option<Locale>, cx: &mut App) {
    crate::language::set(choice, cx);
}
