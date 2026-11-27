use thelemail_core::notify::NotificationTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemFolder {
    Inbox,
    Starred,
    Sent,
    Drafts,
    Scheduled,
    Snoozed,
    Archive,
    Spam,
    Trash,
}

impl SystemFolder {
    const ALL: [Self; 9] = [
        Self::Inbox,
        Self::Starred,
        Self::Sent,
        Self::Drafts,
        Self::Scheduled,
        Self::Snoozed,
        Self::Archive,
        Self::Spam,
        Self::Trash,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Starred => "starred",
            Self::Sent => "sent",
            Self::Drafts => "drafts",
            Self::Scheduled => "scheduled",
            Self::Snoozed => "snoozed",
            Self::Archive => "archive",
            Self::Spam => "spam",
            Self::Trash => "trash",
        }
    }

    fn parse(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.slug() == slug)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailFolder {
    System(SystemFolder),
    Folder(String),
    Label(String),
}

impl MailFolder {
    fn segment(&self) -> String {
        match self {
            Self::System(f) => f.slug().to_string(),
            Self::Folder(id) => format!("f-{id}"),
            Self::Label(id) => format!("l-{id}"),
        }
    }

    fn parse(segment: &str) -> Option<Self> {
        if let Some(f) = SystemFolder::parse(segment) {
            return Some(Self::System(f));
        }
        if let Some(id) = segment.strip_prefix("f-").and_then(collection_id) {
            return Some(Self::Folder(id));
        }
        segment
            .strip_prefix("l-")
            .and_then(collection_id)
            .map(Self::Label)
    }
}

fn collection_id(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    thelemail_core::ids::account_id(&lower)
        .ok()
        .map(str::to_string)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthPage {
    Welcome,
    Login,
    Register,
    Recover,
    Invite(String),
    ForwardingConfirm,
    NotificationEmailConfirm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsSection {
    About,
    Account,
    Addresses,
    Address(String),
    Appearance,
    Blocked,
    Collections,
    Domains,
    Domain(String),
    NewDomain,
    Import,
    Notify,
    Profile,
    Reading,
    Region,
    Search,
    Security,
    Sending,
}

impl SettingsSection {
    const PLAIN: [(&'static str, Self); 15] = [
        ("about", Self::About),
        ("account", Self::Account),
        ("addresses", Self::Addresses),
        ("appearance", Self::Appearance),
        ("blocked", Self::Blocked),
        ("collections", Self::Collections),
        ("domains", Self::Domains),
        ("import", Self::Import),
        ("notify", Self::Notify),
        ("profile", Self::Profile),
        ("reading", Self::Reading),
        ("region", Self::Region),
        ("search", Self::Search),
        ("security", Self::Security),
        ("sending", Self::Sending),
    ];

    fn path(&self) -> String {
        match self {
            Self::Address(id) => format!("addresses/{id}"),
            Self::Domain(id) => format!("domains/{id}"),
            Self::NewDomain => "domains/new".into(),
            plain => Self::PLAIN
                .iter()
                .find(|(_, s)| s == plain)
                .map(|(slug, _)| (*slug).to_string())
                .unwrap_or_default(),
        }
    }

    fn parse(rest: &[&str]) -> Option<Self> {
        match rest {
            ["addresses", id] => Some(Self::Address((*id).to_string())),
            ["domains", "new"] => Some(Self::NewDomain),
            ["domains", id] => Some(Self::Domain((*id).to_string())),
            [slug] => Self::PLAIN
                .iter()
                .find(|(s, _)| s == slug)
                .map(|(_, section)| section.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecyclePage {
    Suspended,
    Expired,
    Restore,
    Export,
    Downgrade,
    DeletionPending,
    Recovery,
    BillingChoose,
    BillingStart,
    BillingReturn,
    BillingCancel,
}

impl LifecyclePage {
    const ALL: [(&'static str, Self); 11] = [
        ("lifecycle/suspended", Self::Suspended),
        ("lifecycle/expired", Self::Expired),
        ("lifecycle/restore", Self::Restore),
        ("lifecycle/export", Self::Export),
        ("lifecycle/downgrade", Self::Downgrade),
        ("deletion-pending", Self::DeletionPending),
        ("recovery", Self::Recovery),
        ("billing/choose", Self::BillingChoose),
        ("billing/start", Self::BillingStart),
        ("billing/return", Self::BillingReturn),
        ("billing/cancel", Self::BillingCancel),
    ];

    fn path(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, p)| *p == self)
            .map(|(path, _)| *path)
            .unwrap_or_default()
    }

    fn parse(rest: &[&str]) -> Option<Self> {
        let joined = rest.join("/");
        Self::ALL
            .iter()
            .find(|(path, _)| *path == joined)
            .map(|(_, p)| *p)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountPage {
    Mail {
        folder: MailFolder,
        id: Option<String>,
    },
    Settings(SettingsSection),
    Lifecycle(LifecyclePage),
}

#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryTab {
    Colours,
    Shadows,
    Shapes,
    Type,
    Motion,
    Icons,
    Buttons,
    Fields,
    Choices,
    Language,
}

#[cfg(debug_assertions)]
impl GalleryTab {
    pub const ALL: [(&'static str, Self); 10] = [
        ("colours", Self::Colours),
        ("shadows", Self::Shadows),
        ("shapes", Self::Shapes),
        ("type", Self::Type),
        ("motion", Self::Motion),
        ("icons", Self::Icons),
        ("buttons", Self::Buttons),
        ("fields", Self::Fields),
        ("choices", Self::Choices),
        ("language", Self::Language),
    ];

    pub fn slug(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, t)| *t == self)
            .map(|(slug, _)| *slug)
            .unwrap_or_default()
    }

    fn parse(slug: &str) -> Option<Self> {
        Self::ALL.iter().find(|(s, _)| *s == slug).map(|(_, t)| *t)
    }
}

#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugPage {
    Tokens(GalleryTab),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Auth(AuthPage),
    Account {
        slot: u8,
        page: AccountPage,
    },
    #[cfg(debug_assertions)]
    Debug(DebugPage),
}

impl Route {
    pub fn inbox(slot: u8) -> Self {
        Self::Account {
            slot,
            page: AccountPage::Mail {
                folder: MailFolder::System(SystemFolder::Inbox),
                id: None,
            },
        }
    }

    pub fn slot(&self) -> Option<u8> {
        match self {
            Self::Account { slot, .. } => Some(*slot),
            Self::Auth(_) => None,
            #[cfg(debug_assertions)]
            Self::Debug(_) => None,
        }
    }

    pub fn path(&self) -> String {
        match self {
            Self::Auth(page) => match page {
                AuthPage::Welcome => "/".into(),
                AuthPage::Login => "/login".into(),
                AuthPage::Register => "/register".into(),
                AuthPage::Recover => "/recover".into(),
                AuthPage::Invite(token) => format!("/invite/{token}"),
                AuthPage::ForwardingConfirm => "/forwarding/confirm".into(),
                AuthPage::NotificationEmailConfirm => "/notification-email/confirm".into(),
            },
            Self::Account { slot, page } => match page {
                AccountPage::Mail { folder, id } => match id {
                    Some(id) => format!("/u/{slot}/mail/{}/{id}", folder.segment()),
                    None => format!("/u/{slot}/mail/{}", folder.segment()),
                },
                AccountPage::Settings(section) => {
                    format!("/u/{slot}/settings/{}", section.path())
                }
                AccountPage::Lifecycle(page) => format!("/u/{slot}/{}", page.path()),
            },
            #[cfg(debug_assertions)]
            Self::Debug(DebugPage::Tokens(tab)) => format!("/debug/tokens/{}", tab.slug()),
        }
    }

    pub fn parse(path: &str) -> Option<Self> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        match segments.as_slice() {
            [] => Some(Self::Auth(AuthPage::Welcome)),
            ["login"] => Some(Self::Auth(AuthPage::Login)),
            ["register"] => Some(Self::Auth(AuthPage::Register)),
            ["recover"] => Some(Self::Auth(AuthPage::Recover)),
            ["invite", token] => Some(Self::Auth(AuthPage::Invite((*token).to_string()))),
            ["forwarding", "confirm"] => Some(Self::Auth(AuthPage::ForwardingConfirm)),
            ["notification-email", "confirm"] => {
                Some(Self::Auth(AuthPage::NotificationEmailConfirm))
            }
            #[cfg(debug_assertions)]
            ["debug", "tokens"] => Some(Self::Debug(DebugPage::Tokens(GalleryTab::Colours))),
            #[cfg(debug_assertions)]
            ["debug", "tokens", tab] => {
                Some(Self::Debug(DebugPage::Tokens(GalleryTab::parse(tab)?)))
            }
            ["u", slot, rest @ ..] => {
                if !slot.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                let slot: u8 = slot.parse().ok()?;
                let page = match rest {
                    ["mail"] => AccountPage::Mail {
                        folder: MailFolder::System(SystemFolder::Inbox),
                        id: None,
                    },
                    ["mail", folder] => AccountPage::Mail {
                        folder: MailFolder::parse(folder)?,
                        id: None,
                    },
                    ["mail", folder, id] => AccountPage::Mail {
                        folder: MailFolder::parse(folder)?,
                        id: Some((*id).to_string()),
                    },
                    ["settings", section @ ..] => {
                        AccountPage::Settings(SettingsSection::parse(section)?)
                    }
                    other => AccountPage::Lifecycle(LifecyclePage::parse(other)?),
                };
                Some(Self::Account { slot, page })
            }
            _ => None,
        }
    }

    pub fn for_notification(target: &NotificationTarget, accounts: &[String]) -> Option<Self> {
        let slot = accounts.iter().position(|id| *id == target.account_id)?;
        Some(Self::Account {
            slot: u8::try_from(slot).ok()?,
            page: AccountPage::Mail {
                folder: MailFolder::System(SystemFolder::Inbox),
                id: Some(target.message_id.clone()),
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct History {
    entries: Vec<Route>,
    index: usize,
}

impl History {
    pub fn new(start: Route) -> Self {
        Self {
            entries: vec![start],
            index: 0,
        }
    }

    pub fn current(&self) -> &Route {
        &self.entries[self.index]
    }

    pub fn push(&mut self, route: Route) -> bool {
        if *self.current() == route {
            return false;
        }
        self.entries.truncate(self.index + 1);
        self.entries.push(route);
        self.index += 1;
        true
    }

    pub fn replace(&mut self, route: Route) {
        self.entries[self.index] = route;
    }

    pub fn reset(&mut self, route: Route) {
        self.entries = vec![route];
        self.index = 0;
    }

    pub fn can_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_forward(&self) -> bool {
        self.index + 1 < self.entries.len()
    }

    pub fn back(&mut self) -> bool {
        if !self.can_back() {
            return false;
        }
        self.index -= 1;
        true
    }

    pub fn forward(&mut self) -> bool {
        if !self.can_forward() {
            return false;
        }
        self.index += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT_A: &str = "0c0c0c0c-0c0c-4c0c-8c0c-0c0c0c0c0c0c";
    const ACCOUNT_B: &str = "1d1d1d1d-1d1d-4d1d-8d1d-1d1d1d1d1d1d";
    const COLLECTION: &str = "2e2e2e2e-2e2e-4e2e-8e2e-2e2e2e2e2e2e";

    fn settings(section: SettingsSection) -> Route {
        Route::Account {
            slot: 0,
            page: AccountPage::Settings(section),
        }
    }

    #[test]
    fn every_web_route_round_trips() {
        let paths = [
            "/",
            "/login",
            "/register",
            "/recover",
            "/invite/abc123",
            "/forwarding/confirm",
            "/notification-email/confirm",
            "/u/0/mail/inbox",
            "/u/1/mail/starred",
            "/u/0/mail/sent",
            "/u/0/mail/drafts",
            "/u/0/mail/scheduled",
            "/u/0/mail/snoozed",
            "/u/0/mail/archive",
            "/u/0/mail/spam",
            "/u/0/mail/trash/m-1",
            "/u/0/mail/f-2e2e2e2e-2e2e-4e2e-8e2e-2e2e2e2e2e2e",
            "/u/0/mail/l-2e2e2e2e-2e2e-4e2e-8e2e-2e2e2e2e2e2e/m-2",
            "/u/0/settings/about",
            "/u/0/settings/account",
            "/u/0/settings/addresses",
            "/u/0/settings/addresses/a-1",
            "/u/0/settings/appearance",
            "/u/0/settings/blocked",
            "/u/0/settings/collections",
            "/u/0/settings/domains",
            "/u/0/settings/domains/d-1",
            "/u/0/settings/domains/new",
            "/u/0/settings/import",
            "/u/0/settings/notify",
            "/u/0/settings/profile",
            "/u/0/settings/reading",
            "/u/0/settings/region",
            "/u/0/settings/search",
            "/u/0/settings/security",
            "/u/0/settings/sending",
            "/u/0/lifecycle/suspended",
            "/u/0/lifecycle/expired",
            "/u/0/lifecycle/restore",
            "/u/0/lifecycle/export",
            "/u/0/lifecycle/downgrade",
            "/u/0/deletion-pending",
            "/u/0/recovery",
            "/u/0/billing/choose",
            "/u/0/billing/start",
            "/u/0/billing/return",
            "/u/0/billing/cancel",
        ];
        for path in paths {
            let route = Route::parse(path).unwrap_or_else(|| panic!("{path} did not parse"));
            assert_eq!(route.path(), path);
        }
        #[cfg(debug_assertions)]
        for (slug, _) in GalleryTab::ALL {
            let path = format!("/debug/tokens/{slug}");
            assert_eq!(Route::parse(&path).map(|r| r.path()), Some(path));
        }
    }

    #[test]
    fn bare_mail_is_the_inbox() {
        assert_eq!(Route::parse("/u/2/mail"), Some(Route::inbox(2)));
    }

    #[test]
    fn collection_ids_are_lowercased_like_the_web() {
        let upper = format!("/u/0/mail/f-{}", COLLECTION.to_uppercase());
        assert_eq!(
            Route::parse(&upper),
            Some(Route::Account {
                slot: 0,
                page: AccountPage::Mail {
                    folder: MailFolder::Folder(COLLECTION.into()),
                    id: None
                }
            })
        );
    }

    #[test]
    fn unknown_paths_are_rejected() {
        for path in [
            "/u/x/mail",
            "/u/-1/mail",
            "/u/+1/mail",
            "/u/999/mail",
            "/u/0/mail/outbox",
            "/u/0/mail/f-not-a-uuid",
            "/u/0/settings",
            "/u/0/settings/nope",
            "/u/0/calendar",
            "/u/0/lifecycle",
            "/auth/callback",
            "/invite",
        ] {
            assert_eq!(Route::parse(path), None, "{path}");
        }
    }

    #[test]
    fn a_notification_opens_its_message_in_the_right_slot() {
        let accounts = vec![ACCOUNT_A.to_string(), ACCOUNT_B.to_string()];
        let target = NotificationTarget {
            account_id: ACCOUNT_B.into(),
            message_id: "m-9".into(),
        };
        assert_eq!(
            Route::for_notification(&target, &accounts).map(|r| r.path()),
            Some("/u/1/mail/inbox/m-9".to_string())
        );
    }

    #[test]
    fn a_notification_for_an_account_not_signed_in_opens_nothing() {
        let target = NotificationTarget {
            account_id: ACCOUNT_B.into(),
            message_id: "m-9".into(),
        };
        assert_eq!(Route::for_notification(&target, &[ACCOUNT_A.into()]), None);
        assert_eq!(Route::for_notification(&target, &[]), None);
    }

    #[test]
    fn back_and_forward_walk_the_history() {
        let mut h = History::new(Route::inbox(0));
        assert!(!h.can_back());
        assert!(h.push(settings(SettingsSection::Profile)));
        assert!(h.push(settings(SettingsSection::Security)));
        assert!(h.back());
        assert_eq!(h.current(), &settings(SettingsSection::Profile));
        assert!(h.back());
        assert_eq!(h.current(), &Route::inbox(0));
        assert!(!h.back());
        assert!(h.forward());
        assert!(h.forward());
        assert_eq!(h.current(), &settings(SettingsSection::Security));
        assert!(!h.forward());
    }

    #[test]
    fn pushing_after_going_back_drops_the_forward_entries() {
        let mut h = History::new(Route::inbox(0));
        h.push(settings(SettingsSection::Profile));
        h.push(settings(SettingsSection::Security));
        h.back();
        h.back();
        h.push(settings(SettingsSection::Reading));
        assert!(!h.can_forward());
        assert!(h.back());
        assert_eq!(h.current(), &Route::inbox(0));
    }

    #[test]
    fn pushing_the_current_route_adds_no_entry() {
        let mut h = History::new(Route::inbox(0));
        assert!(!h.push(Route::inbox(0)));
        assert!(!h.can_back());
    }

    #[test]
    fn replace_and_reset() {
        let mut h = History::new(Route::inbox(0));
        h.push(settings(SettingsSection::Profile));
        h.replace(settings(SettingsSection::Region));
        assert_eq!(h.current(), &settings(SettingsSection::Region));
        assert!(h.back());
        assert!(h.forward());
        h.reset(Route::Auth(AuthPage::Welcome));
        assert!(!h.can_back());
        assert!(!h.can_forward());
        assert_eq!(h.current(), &Route::Auth(AuthPage::Welcome));
    }
}
