use serde_json::{Map, Value, json};

use crate::api::account_settings::Section;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkRead {
    Immediately,
    #[default]
    After2s,
    Never,
}

impl MarkRead {
    pub const ALL: [Self; 3] = [Self::Immediately, Self::After2s, Self::Never];

    pub fn delay_ms(self) -> Option<u64> {
        match self {
            Self::Immediately => Some(0),
            Self::After2s => Some(2000),
            Self::Never => None,
        }
    }

    fn wire(self) -> Value {
        self.delay_ms().map_or(Value::Null, Value::from)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpenMessage {
    pub mark_read: MarkRead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Privacy {
    pub strip_tracking_params: bool,
    pub share_spam_headers: Option<bool>,
}

impl Default for Privacy {
    fn default() -> Self {
        Self {
            strip_tracking_params: true,
            share_spam_headers: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplyDefault {
    #[default]
    Reply,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Composing {
    pub confirm_external: bool,
    pub confirm_subject: bool,
    pub confirm_unencrypted: bool,
    pub reply_default: ReplyDefault,
}

impl Default for Composing {
    fn default() -> Self {
        Self {
            confirm_external: true,
            confirm_subject: true,
            confirm_unencrypted: true,
            reply_default: ReplyDefault::Reply,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePref {
    #[default]
    Light,
    Dark,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Appearance {
    pub theme: ThemePref,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalendarPrivacy {
    Private,
    #[default]
    Busy,
    Shared,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendar {
    pub hidden: Vec<String>,
    pub default_calendar_id: Option<String>,
    pub default_privacy: CalendarPrivacy,
    pub default_reminder_minutes: Option<u32>,
    pub week_starts_on: u8,
    pub start_hour: u8,
}

impl Default for Calendar {
    fn default() -> Self {
        Self {
            hidden: Vec::new(),
            default_calendar_id: None,
            default_privacy: CalendarPrivacy::Busy,
            default_reminder_minutes: Some(10),
            week_starts_on: 1,
            start_hour: 6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSection {
    Favorites,
    Folders,
    Labels,
}

impl SidebarSection {
    const ALL: [Self; 3] = [Self::Favorites, Self::Folders, Self::Labels];

    fn as_str(self) -> &'static str {
        match self {
            Self::Favorites => "favorites",
            Self::Folders => "folders",
            Self::Labels => "labels",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MailSidebar {
    pub expanded: Vec<String>,
    pub collapsed: Vec<SidebarSection>,
    pub show_more: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecentKind {
    Move,
    Labels,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MailRecents {
    pub moves: Vec<String>,
    pub labels: Vec<String>,
}

impl MailRecents {
    pub fn of(&self, kind: RecentKind) -> &[String] {
        match kind {
            RecentKind::Move => &self.moves,
            RecentKind::Labels => &self.labels,
        }
    }

    fn of_mut(&mut self, kind: RecentKind) -> &mut Vec<String> {
        match kind {
            RecentKind::Move => &mut self.moves,
            RecentKind::Labels => &mut self.labels,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DateFormat {
    #[default]
    Dmy,
    Mdy,
    Iso,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeFormat {
    #[default]
    H24,
    H12,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Localization {
    pub date_format: DateFormat,
    pub time_format: TimeFormat,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AccountSettings {
    pub open_message: OpenMessage,
    pub privacy: Privacy,
    pub composing: Composing,
    pub appearance: Appearance,
    pub calendar: Calendar,
    pub mail_sidebar: MailSidebar,
    pub mail_recents: MailRecents,
    pub localization: Localization,
    pub(crate) raw: Map<String, Value>,
}

pub const MAX_REMINDER_MINUTES: f64 = 10080.0;
pub const MAX_SIDEBAR_EXPANDED: usize = 1500;
pub const MAX_RECENTS: usize = 6;
const MAX_ID_LEN: usize = 64;

pub(crate) const READING_OPEN_MESSAGE: &str = "reading_open_message";
pub(crate) const PRIVACY: &str = "privacy";
pub(crate) const COMPOSING: &str = "composing";
pub(crate) const APPEARANCE: &str = "appearance";
pub(crate) const CALENDAR: &str = "calendar";
pub(crate) const MAIL_SIDEBAR: &str = "mail_sidebar";
pub(crate) const MAIL_RECENTS: &str = "mail_recents";
pub(crate) const LOCALIZATION: &str = "localization";

fn record(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

fn boolean(section: &Map<String, Value>, key: &str) -> Option<bool> {
    section.get(key).and_then(Value::as_bool)
}

fn text<'a>(section: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    section.get(key).and_then(Value::as_str)
}

fn round_half_up(n: f64) -> f64 {
    (n + 0.5).floor()
}

pub fn parse_open_message(raw: Option<&Value>) -> OpenMessage {
    let Some(section) = record(raw) else {
        return OpenMessage::default();
    };
    let mark_read = match section.get("markRead") {
        Some(Value::Null) => Some(MarkRead::Never),
        Some(Value::Number(n)) => n.as_f64().and_then(|ms| {
            MarkRead::ALL
                .into_iter()
                .find(|m| m.delay_ms().is_some_and(|d| d as f64 == ms))
        }),
        Some(Value::String(s)) => match s.as_str() {
            "Immediately on open" => Some(MarkRead::Immediately),
            "After 2 seconds" => Some(MarkRead::After2s),
            "Never automatically" => Some(MarkRead::Never),
            _ => None,
        },
        _ => None,
    };
    OpenMessage {
        mark_read: mark_read.unwrap_or_default(),
    }
}

pub fn parse_privacy(raw: Option<&Value>) -> Privacy {
    let mut next = Privacy::default();
    if let Some(p) = record(raw) {
        if let Some(v) = boolean(p, "stripTrackingParams") {
            next.strip_tracking_params = v;
        }
        if let Some(v) = boolean(p, "shareSpamHeaders") {
            next.share_spam_headers = Some(v);
        }
    }
    next
}

pub fn parse_composing(raw: Option<&Value>) -> Composing {
    let mut next = Composing::default();
    if let Some(c) = record(raw) {
        if let Some(v) = boolean(c, "confirmExternal") {
            next.confirm_external = v;
        }
        if let Some(v) = boolean(c, "confirmSubject") {
            next.confirm_subject = v;
        }
        if let Some(v) = boolean(c, "confirmUnencrypted") {
            next.confirm_unencrypted = v;
        }
        match text(c, "replyDefault") {
            Some("reply") => next.reply_default = ReplyDefault::Reply,
            Some("all") => next.reply_default = ReplyDefault::All,
            _ => {}
        }
    }
    next
}

pub fn parse_appearance(raw: Option<&Value>) -> Appearance {
    let theme = match record(raw).and_then(|a| text(a, "theme")) {
        Some("dark") => ThemePref::Dark,
        Some("auto") => ThemePref::Auto,
        _ => ThemePref::Light,
    };
    Appearance { theme }
}

pub fn parse_calendar(raw: Option<&Value>) -> Calendar {
    let mut next = Calendar::default();
    let Some(c) = record(raw) else {
        return next;
    };
    if let Some(hidden) = c.get("hidden").and_then(Value::as_array) {
        next.hidden = hidden
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
    }
    if let Some(id) = text(c, "defaultCalendarId") {
        next.default_calendar_id = Some(id.to_owned());
    }
    match text(c, "defaultPrivacy") {
        Some("private") => next.default_privacy = CalendarPrivacy::Private,
        Some("busy") => next.default_privacy = CalendarPrivacy::Busy,
        Some("shared") => next.default_privacy = CalendarPrivacy::Shared,
        _ => {}
    }
    match c.get("defaultReminderMinutes") {
        Some(Value::Null) => next.default_reminder_minutes = None,
        Some(Value::Number(n)) => {
            if let Some(m) = n
                .as_f64()
                .filter(|m| (0.0..=MAX_REMINDER_MINUTES).contains(m))
            {
                next.default_reminder_minutes = Some(round_half_up(m) as u32);
            }
        }
        _ => {}
    }
    if let Some(day) = c
        .get("weekStartsOn")
        .and_then(Value::as_f64)
        .filter(|d| *d == 0.0 || *d == 1.0)
    {
        next.week_starts_on = day as u8;
    }
    if let Some(h) = c
        .get("startHour")
        .and_then(Value::as_f64)
        .filter(|h| (0.0..=12.0).contains(h))
    {
        next.start_hour = round_half_up(h) as u8;
    }
    next
}

pub fn parse_mail_sidebar(raw: Option<&Value>) -> MailSidebar {
    let mut next = MailSidebar::default();
    let Some(v) = record(raw) else {
        return next;
    };
    if let Some(expanded) = v.get("expanded").and_then(Value::as_array) {
        next.expanded = dedup(
            expanded
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| utf16_len(s) <= MAX_ID_LEN),
            MAX_SIDEBAR_EXPANDED,
        );
    }
    if let Some(collapsed) = v.get("collapsed").and_then(Value::as_array) {
        next.collapsed = SidebarSection::ALL
            .into_iter()
            .filter(|s| collapsed.iter().any(|c| c.as_str() == Some(s.as_str())))
            .collect();
    }
    if let Some(show) = boolean(v, "showMore") {
        next.show_more = show;
    }
    next
}

fn recent_ids<'a>(raw: impl Iterator<Item = &'a str>) -> Vec<String> {
    dedup(
        raw.filter(|s| !s.is_empty() && utf16_len(s) <= MAX_ID_LEN),
        MAX_RECENTS,
    )
}

pub fn parse_mail_recents(raw: Option<&Value>) -> MailRecents {
    let Some(v) = record(raw) else {
        return MailRecents::default();
    };
    let ids = |key: &str| {
        v.get(key)
            .and_then(Value::as_array)
            .map(|a| recent_ids(a.iter().filter_map(Value::as_str)))
            .unwrap_or_default()
    };
    MailRecents {
        moves: ids("move"),
        labels: ids("labels"),
    }
}

pub fn push_recents(list: &[String], used: &[String]) -> Vec<String> {
    recent_ids(used.iter().rev().chain(list.iter()).map(String::as_str))
}

pub fn parse_localization(raw: Option<&Value>) -> Localization {
    let mut next = Localization::default();
    if let Some(l) = record(raw) {
        match text(l, "dateFormat") {
            Some("dmy") => next.date_format = DateFormat::Dmy,
            Some("mdy") => next.date_format = DateFormat::Mdy,
            Some("iso") => next.date_format = DateFormat::Iso,
            _ => {}
        }
        match text(l, "timeFormat") {
            Some("24") => next.time_format = TimeFormat::H24,
            Some("12") => next.time_format = TimeFormat::H12,
            _ => {}
        }
    }
    next
}

fn dedup<'a>(items: impl Iterator<Item = &'a str>, cap: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if !out.iter().any(|o| o == item) {
            out.push(item.to_owned());
        }
    }
    out.truncate(cap);
    out
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

impl AccountSettings {
    pub fn parse(sections: Map<String, Value>) -> Self {
        let raw: Map<String, Value> = sections
            .into_iter()
            .map(|(k, v)| {
                let v = match v {
                    Value::Object(o) => Value::Object(o),
                    _ => Value::Object(Map::new()),
                };
                (k, v)
            })
            .collect();
        Self {
            open_message: parse_open_message(raw.get(READING_OPEN_MESSAGE)),
            privacy: parse_privacy(raw.get(PRIVACY)),
            composing: parse_composing(raw.get(COMPOSING)),
            appearance: parse_appearance(raw.get(APPEARANCE)),
            calendar: parse_calendar(raw.get(CALENDAR)),
            mail_sidebar: parse_mail_sidebar(raw.get(MAIL_SIDEBAR)),
            mail_recents: parse_mail_recents(raw.get(MAIL_RECENTS)),
            localization: parse_localization(raw.get(LOCALIZATION)),
            raw,
        }
    }

    pub fn mark_read_delay_ms(&self) -> Option<u64> {
        self.open_message.mark_read.delay_ms()
    }

    pub(crate) fn section(&self, name: &str) -> Section {
        self.raw
            .get(name)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn store_section(&mut self, name: &str, body: Section) {
        self.raw.insert(name.to_owned(), Value::Object(body));
        let reparsed = Self::parse(self.raw.clone());
        let sidebar = std::mem::take(&mut self.mail_sidebar);
        let recents = std::mem::take(&mut self.mail_recents);
        *self = Self {
            mail_sidebar: if name == MAIL_SIDEBAR {
                reparsed.mail_sidebar.clone()
            } else {
                sidebar
            },
            mail_recents: if name == MAIL_RECENTS {
                reparsed.mail_recents.clone()
            } else {
                recents
            },
            ..reparsed
        };
    }

    pub(crate) fn record_recents(&mut self, kind: RecentKind, used: &[String]) {
        let next = push_recents(self.mail_recents.of(kind), used);
        *self.mail_recents.of_mut(kind) = next;
    }
}

pub fn open_message_body(value: OpenMessage) -> Section {
    object(json!({ "markRead": value.mark_read.wire() }))
}

pub fn privacy_body(value: Privacy) -> Section {
    object(json!({
        "stripTrackingParams": value.strip_tracking_params,
        "shareSpamHeaders": value.share_spam_headers,
    }))
}

pub fn composing_body(value: Composing) -> Section {
    object(json!({
        "confirmExternal": value.confirm_external,
        "confirmSubject": value.confirm_subject,
        "confirmUnencrypted": value.confirm_unencrypted,
        "replyDefault": match value.reply_default {
            ReplyDefault::Reply => "reply",
            ReplyDefault::All => "all",
        },
    }))
}

pub fn appearance_body(value: Appearance) -> Section {
    object(json!({
        "theme": match value.theme {
            ThemePref::Light => "light",
            ThemePref::Dark => "dark",
            ThemePref::Auto => "auto",
        },
    }))
}

pub fn calendar_body(value: &Calendar) -> Section {
    object(json!({
        "hidden": value.hidden,
        "defaultCalendarId": value.default_calendar_id,
        "defaultPrivacy": match value.default_privacy {
            CalendarPrivacy::Private => "private",
            CalendarPrivacy::Busy => "busy",
            CalendarPrivacy::Shared => "shared",
        },
        "defaultReminderMinutes": value.default_reminder_minutes,
        "weekStartsOn": value.week_starts_on,
        "startHour": value.start_hour,
    }))
}

pub fn mail_sidebar_body(value: &MailSidebar) -> Section {
    object(json!({
        "expanded": value.expanded,
        "collapsed": value.collapsed.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        "showMore": value.show_more,
    }))
}

pub fn mail_recents_body(value: &MailRecents) -> Section {
    object(json!({ "move": value.moves, "labels": value.labels }))
}

pub fn localization_body(value: Localization) -> Section {
    object(json!({
        "dateFormat": match value.date_format {
            DateFormat::Dmy => "dmy",
            DateFormat::Mdy => "mdy",
            DateFormat::Iso => "iso",
        },
        "timeFormat": match value.time_format {
            TimeFormat::H24 => "24",
            TimeFormat::H12 => "12",
        },
    }))
}

fn object(value: Value) -> Section {
    match value {
        Value::Object(o) => o,
        _ => Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(v: Value) -> AccountSettings {
        AccountSettings::parse(v.as_object().cloned().unwrap_or_default())
    }

    #[test]
    fn nothing_stored_means_every_default() {
        let s = parse(json!({}));
        assert_eq!(s.open_message.mark_read, MarkRead::After2s);
        assert_eq!(s.mark_read_delay_ms(), Some(2000));
        assert_eq!(s.privacy, Privacy::default());
        assert!(s.privacy.strip_tracking_params);
        assert_eq!(s.composing, Composing::default());
        assert_eq!(s.appearance.theme, ThemePref::Light);
        assert_eq!(s.calendar, Calendar::default());
        assert_eq!(s.calendar.default_reminder_minutes, Some(10));
        assert_eq!(s.mail_sidebar, MailSidebar::default());
        assert_eq!(s.localization, Localization::default());
    }

    #[test]
    fn mark_read_reads_delays_null_and_the_old_labels() {
        let read = |v: Value| parse_open_message(Some(&json!({ "markRead": v }))).mark_read;
        assert_eq!(read(json!(0)), MarkRead::Immediately);
        assert_eq!(read(json!(2000)), MarkRead::After2s);
        assert_eq!(read(Value::Null), MarkRead::Never);
        assert_eq!(read(json!("Immediately on open")), MarkRead::Immediately);
        assert_eq!(read(json!("Never automatically")), MarkRead::Never);
        assert_eq!(read(json!(1500)), MarkRead::After2s);
        assert_eq!(read(json!("soon")), MarkRead::After2s);
        assert_eq!(
            open_message_body(OpenMessage {
                mark_read: MarkRead::Never
            })["markRead"],
            Value::Null
        );
    }

    #[test]
    fn typed_fields_ignore_wrong_types() {
        let s = parse(json!({
            "privacy": { "stripTrackingParams": "no", "shareSpamHeaders": false },
            "composing": { "confirmExternal": false, "replyDefault": "everyone" },
            "appearance": { "theme": "auto" },
            "localization": { "dateFormat": "iso", "timeFormat": 12 },
        }));
        assert!(s.privacy.strip_tracking_params);
        assert_eq!(s.privacy.share_spam_headers, Some(false));
        assert!(!s.composing.confirm_external);
        assert_eq!(s.composing.reply_default, ReplyDefault::Reply);
        assert_eq!(s.appearance.theme, ThemePref::Auto);
        assert_eq!(s.localization.date_format, DateFormat::Iso);
        assert_eq!(s.localization.time_format, TimeFormat::H24);
    }

    #[test]
    fn calendar_values_are_bounded_and_rounded() {
        let c = parse_calendar(Some(&json!({
            "hidden": ["a", 3, "b"],
            "defaultCalendarId": "cal",
            "defaultPrivacy": "shared",
            "defaultReminderMinutes": 14.5,
            "weekStartsOn": 0,
            "startHour": 8.5,
        })));
        assert_eq!(c.hidden, ["a", "b"]);
        assert_eq!(c.default_calendar_id.as_deref(), Some("cal"));
        assert_eq!(c.default_privacy, CalendarPrivacy::Shared);
        assert_eq!(c.default_reminder_minutes, Some(15));
        assert_eq!(c.week_starts_on, 0);
        assert_eq!(c.start_hour, 9);
        let out = parse_calendar(Some(&json!({
            "defaultReminderMinutes": 20000, "weekStartsOn": 3, "startHour": 13
        })));
        assert_eq!(out, Calendar::default());
        let none = parse_calendar(Some(&json!({ "defaultReminderMinutes": null })));
        assert_eq!(none.default_reminder_minutes, None);
    }

    #[test]
    fn the_sidebar_keeps_unique_short_ids_and_known_sections_in_order() {
        let long = "x".repeat(65);
        let s = parse_mail_sidebar(Some(&json!({
            "expanded": ["a", "a", long, 7, "b"],
            "collapsed": ["labels", "nope", "favorites"],
            "showMore": true,
        })));
        assert_eq!(s.expanded, ["a", "b"]);
        assert_eq!(
            s.collapsed,
            [SidebarSection::Favorites, SidebarSection::Labels]
        );
        assert!(s.show_more);
        let many: Vec<String> = (0..2000).map(|i| i.to_string()).collect();
        let capped = parse_mail_sidebar(Some(&json!({ "expanded": many })));
        assert_eq!(capped.expanded.len(), MAX_SIDEBAR_EXPANDED);
    }

    #[test]
    fn recents_put_the_newest_first_and_stay_short() {
        let r = parse_mail_recents(Some(&json!({
            "move": ["a", "", "a", "b"],
            "labels": ["1", "2", "3", "4", "5", "6", "7"],
        })));
        assert_eq!(r.moves, ["a", "b"]);
        assert_eq!(r.labels.len(), MAX_RECENTS);
        let list: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        let used: Vec<String> = ["x", "b"].map(String::from).to_vec();
        assert_eq!(push_recents(&list, &used), ["b", "x", "a", "c"]);
    }

    #[test]
    fn a_stored_section_replaces_only_its_own_fields() {
        let mut s = parse(json!({ "privacy": { "stripTrackingParams": false, "extra": 1 } }));
        s.mail_sidebar.show_more = true;
        s.store_section(
            COMPOSING,
            composing_body(Composing {
                confirm_subject: false,
                ..Composing::default()
            }),
        );
        assert!(!s.composing.confirm_subject);
        assert!(!s.privacy.strip_tracking_params);
        assert!(s.mail_sidebar.show_more);
        assert_eq!(s.section(PRIVACY)["extra"], json!(1));
    }

    #[test]
    fn a_non_object_section_reads_as_empty() {
        let s = parse(json!({ "privacy": [1, 2], "composing": "x" }));
        assert_eq!(s.privacy, Privacy::default());
        assert!(s.section(PRIVACY).is_empty());
    }
}
