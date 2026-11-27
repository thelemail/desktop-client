pub mod plans;
pub mod settings;
mod store;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use thelemail_state::Resource;

pub use self::settings::AccountSettings;
use self::settings::{
    Appearance, Calendar, Composing, Localization, MAIL_RECENTS, MAIL_SIDEBAR, MailSidebar,
    OpenMessage, Privacy, RecentKind,
};
pub use self::store::Store;
use crate::api::ApiError;
use crate::api::account_settings::{self, Section};
use crate::api::billing::{self, Plan, PlanCatalog, PlanCode, Subscription};
use crate::api::me::{self, Me};
use crate::{Core, CoreEvent};

pub const DEBOUNCE: Duration = Duration::from_millis(400);
const CATALOG: &str = "catalog";

#[derive(Default)]
pub struct Account {
    profiles: Store<Me>,
    settings: Store<AccountSettings>,
    subscriptions: Store<Subscription>,
    catalog: Store<PlanCatalog>,
    pending: Mutex<HashMap<(String, &'static str), u64>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Billing {
    pub subscription: Subscription,
    pub plan_code: Option<PlanCode>,
    pub plan: Option<Plan>,
    pub needs_payment: bool,
}

impl Billing {
    fn of(subscription: Subscription, catalog: Option<&PlanCatalog>) -> Self {
        let plan_code = subscription.plan_code.clone();
        let plan = plan_code
            .as_ref()
            .and_then(|c| catalog.and_then(|cat| cat.plan(c)))
            .cloned();
        Self {
            needs_payment: !subscription.entitled,
            subscription,
            plan_code,
            plan,
        }
    }

    pub fn is_free(&self) -> bool {
        plans::is_free_plan(self.plan_code.as_ref())
    }

    pub fn is_free_family(&self) -> bool {
        plans::is_free_family(self.plan_code.as_ref())
    }

    pub fn can_add_domains(&self) -> bool {
        plans::allows_custom_domains(self.plan_code.as_ref())
    }

    pub fn can_add_shared_addresses(&self) -> bool {
        plans::allows_shared_addresses(self.plan_code.as_ref())
    }

    pub fn can_add_shared_domain_alias(&self) -> bool {
        plans::allows_shared_domain_alias(self.plan_code.as_ref())
    }

    pub fn can_add_members(&self) -> bool {
        plans::allows_members(self.plan_code.as_ref())
    }
}

impl Account {
    pub fn profile(&self, account_id: &str) -> Resource<Me, ApiError> {
        self.profiles.get(account_id)
    }

    pub fn settings(&self, account_id: &str) -> Resource<AccountSettings, ApiError> {
        self.settings.get(account_id)
    }

    pub fn subscription(&self, account_id: &str) -> Resource<Subscription, ApiError> {
        self.subscriptions.get(account_id)
    }

    pub fn catalog(&self) -> Resource<PlanCatalog, ApiError> {
        self.catalog.get(CATALOG)
    }

    pub fn billing(&self, account_id: &str) -> Resource<Billing, ApiError> {
        let catalog = self.catalog.data(CATALOG);
        match self.subscriptions.get(account_id) {
            Resource::NotLoaded => Resource::NotLoaded,
            Resource::Loading => Resource::Loading,
            Resource::Loaded(s) => Resource::Loaded(Billing::of(s, catalog.as_ref())),
            Resource::Refreshing(s) => Resource::Refreshing(Billing::of(s, catalog.as_ref())),
            Resource::Failed { error, data } => Resource::Failed {
                error,
                data: data.map(|s| Billing::of(s, catalog.as_ref())),
            },
        }
    }

    pub fn forget(&self, account_id: &str) {
        self.profiles.forget(account_id);
        self.settings.forget(account_id);
        self.subscriptions.forget(account_id);
        self.pending
            .lock()
            .expect("settings pending")
            .retain(|(id, _), _| id != account_id);
    }

    pub fn forget_all(&self) {
        self.profiles.forget_all();
        self.settings.forget_all();
        self.subscriptions.forget_all();
        self.pending.lock().expect("settings pending").clear();
    }

    pub(crate) fn has_profile(&self, account_id: &str) -> bool {
        self.profiles.is_started(account_id)
    }

    pub(crate) fn has_settings(&self, account_id: &str) -> bool {
        self.settings.is_started(account_id)
    }

    pub(crate) fn has_subscription(&self, account_id: &str) -> bool {
        self.subscriptions.is_started(account_id)
    }

    fn is_pending(&self, account_id: &str, section: &'static str) -> bool {
        self.pending
            .lock()
            .expect("settings pending")
            .contains_key(&(account_id.to_owned(), section))
    }

    fn bump(&self, account_id: &str, section: &'static str) -> u64 {
        let mut pending = self.pending.lock().expect("settings pending");
        let slot = pending.entry((account_id.to_owned(), section)).or_default();
        *slot += 1;
        *slot
    }

    fn settle(&self, account_id: &str, section: &'static str, generation: u64) -> bool {
        let mut pending = self.pending.lock().expect("settings pending");
        let key = (account_id.to_owned(), section);
        if pending.get(&key) == Some(&generation) {
            pending.remove(&key);
            true
        } else {
            false
        }
    }
}

fn changed(core: &Core, loaded: Option<store::Loaded>, event: CoreEvent) {
    if matches!(loaded, Some(store::Loaded::Changed)) {
        core.emit(event);
    }
}

pub async fn load_profile(core: &Arc<Core>, account_id: &str) {
    let loaded = core
        .account()
        .profiles
        .load(account_id, || me::get(core.api(), account_id))
        .await;
    changed(
        core,
        loaded,
        CoreEvent::ProfileChanged {
            account_id: account_id.to_owned(),
        },
    );
}

pub async fn load_settings(core: &Arc<Core>, account_id: &str) {
    let account = core.account();
    let loaded = account
        .settings
        .load_merging(
            account_id,
            || async {
                account_settings::get(core.api(), account_id)
                    .await
                    .map(|r| AccountSettings::parse(r.sections))
            },
            |previous, mut fresh| {
                if let Some(previous) = previous {
                    if account.is_pending(account_id, MAIL_SIDEBAR) {
                        fresh.mail_sidebar = previous.mail_sidebar.clone();
                    }
                    if account.is_pending(account_id, MAIL_RECENTS) {
                        fresh.mail_recents = previous.mail_recents.clone();
                    }
                }
                fresh
            },
        )
        .await;
    changed(
        core,
        loaded,
        CoreEvent::SettingsChanged {
            account_id: account_id.to_owned(),
        },
    );
}

pub async fn load_billing(core: &Arc<Core>, account_id: &str) {
    let account = core.account();
    let catalog = async {
        if !account.catalog.is_loaded(CATALOG) {
            account
                .catalog
                .load(CATALOG, || billing::plans(core.api()))
                .await
        } else {
            None
        }
    };
    let subscription = account
        .subscriptions
        .load(account_id, || billing::subscription(core.api(), account_id));
    let (catalog, subscription) = futures_util::join!(catalog, subscription);
    if matches!(catalog, Some(store::Loaded::Changed))
        || matches!(subscription, Some(store::Loaded::Changed))
    {
        core.emit(CoreEvent::BillingChanged {
            account_id: account_id.to_owned(),
        });
    }
}

pub async fn put_section(
    core: &Arc<Core>,
    account_id: &str,
    name: &'static str,
    patch: Section,
    merge: bool,
) -> Result<(), ApiError> {
    let account = core.account();
    let Some(current) = account.settings.data(account_id) else {
        return Err(ApiError::Request);
    };
    let body = if merge {
        let mut body = current.section(name);
        body.extend(patch);
        body
    } else {
        patch
    };
    account_settings::put(core.api(), account_id, name, &body).await?;
    let stored = account
        .settings
        .update(account_id, |s| s.store_section(name, body));
    if stored {
        core.emit(CoreEvent::SettingsChanged {
            account_id: account_id.to_owned(),
        });
    }
    Ok(())
}

pub async fn persist_open_message(
    core: &Arc<Core>,
    account_id: &str,
    value: OpenMessage,
) -> Result<(), ApiError> {
    let body = settings::open_message_body(value);
    put_section(core, account_id, settings::READING_OPEN_MESSAGE, body, true).await
}

pub async fn persist_privacy(
    core: &Arc<Core>,
    account_id: &str,
    value: Privacy,
) -> Result<(), ApiError> {
    let body = settings::privacy_body(value);
    put_section(core, account_id, settings::PRIVACY, body, true).await
}

pub async fn persist_composing(
    core: &Arc<Core>,
    account_id: &str,
    value: Composing,
) -> Result<(), ApiError> {
    let body = settings::composing_body(value);
    put_section(core, account_id, settings::COMPOSING, body, true).await
}

pub async fn persist_localization(
    core: &Arc<Core>,
    account_id: &str,
    value: Localization,
) -> Result<(), ApiError> {
    let body = settings::localization_body(value);
    put_section(core, account_id, settings::LOCALIZATION, body, true).await
}

pub async fn persist_appearance(
    core: &Arc<Core>,
    account_id: &str,
    value: Appearance,
) -> Result<(), ApiError> {
    let body = settings::appearance_body(value);
    put_section(core, account_id, settings::APPEARANCE, body, false).await
}

pub async fn persist_calendar(
    core: &Arc<Core>,
    account_id: &str,
    value: Calendar,
) -> Result<(), ApiError> {
    let body = settings::calendar_body(&value);
    let shown = core
        .account()
        .settings
        .update(account_id, |s| s.calendar = value);
    if shown {
        core.emit(CoreEvent::SettingsChanged {
            account_id: account_id.to_owned(),
        });
    }
    put_section(core, account_id, settings::CALENDAR, body, true).await
}

pub fn persist_mail_sidebar(core: &Arc<Core>, account_id: &str, value: MailSidebar) {
    let value =
        settings::parse_mail_sidebar(Some(&Value::Object(settings::mail_sidebar_body(&value))));
    let shown = core
        .account()
        .settings
        .update(account_id, |s| s.mail_sidebar = value);
    if shown {
        debounce(core, account_id, MAIL_SIDEBAR, |s| {
            settings::mail_sidebar_body(&s.mail_sidebar)
        });
    }
}

pub fn record_recents(core: &Arc<Core>, account_id: &str, kind: RecentKind, used: &[String]) {
    if used.is_empty() {
        return;
    }
    let shown = core
        .account()
        .settings
        .update(account_id, |s| s.record_recents(kind, used));
    if shown {
        debounce(core, account_id, MAIL_RECENTS, |s| {
            settings::mail_recents_body(&s.mail_recents)
        });
    }
}

fn debounce(
    core: &Arc<Core>,
    account_id: &str,
    section: &'static str,
    body: impl FnOnce(&AccountSettings) -> Section + Send + 'static,
) {
    core.emit(CoreEvent::SettingsChanged {
        account_id: account_id.to_owned(),
    });
    let generation = core.account().bump(account_id, section);
    let core = core.clone();
    let account_id = account_id.to_owned();
    tokio::spawn(async move {
        tokio::time::sleep(DEBOUNCE).await;
        if !core.account().settle(&account_id, section, generation) {
            return;
        }
        let Some(current) = core.account().settings.data(&account_id) else {
            return;
        };
        let body = body(&current);
        if account_settings::put(core.api(), &account_id, section, &body)
            .await
            .is_ok()
        {
            let account = core.account();
            let newer = account.is_pending(&account_id, section);
            account.settings.update(&account_id, |s| {
                if newer {
                    s.raw.insert(section.to_owned(), Value::Object(body));
                } else {
                    s.store_section(section, body);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests;
