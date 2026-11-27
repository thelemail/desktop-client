pub mod ids;
pub mod keychain;
pub mod keystore;
pub mod locale;
pub mod mirror;
pub mod net;
pub mod notify;
pub mod paths;
pub mod productkeys;
pub mod session;
pub mod shell;
pub mod updates;

use std::sync::Arc;

use thelemail_api::{ApiConfig, Net, TransportError};
use thelemail_keystore::{Broadcast, Keystore};
use tokio::sync::broadcast;

use crate::keystore::PersistentHalves;
use crate::locale::UiLocale;
use crate::mirror::Mirror;

#[derive(Debug, Clone)]
pub enum CoreEvent {
    Keystore(Broadcast),
    MirrorChanged {
        account_id: String,
        arrived: usize,
    },
    MirrorProgress(mirror::SyncProgress),
    MirrorError {
        account_id: String,
        scope: Option<String>,
        error: String,
    },
    TokenExpired {
        account_id: String,
    },
    NotificationOpened(notify::NotificationTarget),
    NotifyStatus(notify::NotifyStatus),
}

pub struct Core {
    net: Net,
    keystore: Keystore,
    halves: PersistentHalves,
    locale: UiLocale,
    mirror: Mirror,
    events: broadcast::Sender<CoreEvent>,
}

impl Core {
    pub fn new(config: ApiConfig) -> Result<Arc<Self>, TransportError> {
        Ok(Arc::new(Self::with_net(Net::new(config)?)))
    }

    fn with_net(net: Net) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            net,
            keystore: Keystore::new(),
            halves: PersistentHalves::default(),
            locale: UiLocale::default(),
            mirror: Mirror::default(),
            events,
        }
    }

    pub fn net(&self) -> &Net {
        &self.net
    }

    pub fn keystore(&self) -> &Keystore {
        &self.keystore
    }

    pub fn locale(&self) -> &UiLocale {
        &self.locale
    }

    pub fn mirror(&self) -> &Mirror {
        &self.mirror
    }

    pub fn shutdown(&self) {
        self.mirror.shutdown();
    }

    pub(crate) fn halves(&self) -> &PersistentHalves {
        &self.halves
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.events.subscribe()
    }

    pub(crate) fn emit(&self, event: CoreEvent) {
        let _ = self.events.send(event);
    }

    pub fn start(self: &Arc<Self>) {
        let core = self.clone();
        let mut events = core.keystore.subscribe();
        tokio::spawn(async move {
            loop {
                let event = match events.recv().await {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                core.relay(event);
            }
        });
    }

    fn relay(self: &Arc<Self>, event: Broadcast) {
        match &event {
            Broadcast::AccountKeysChanged {
                account_id,
                changed: true,
                ..
            } => {
                let core = self.clone();
                let id = account_id.clone();
                tokio::task::spawn_blocking(move || {
                    keystore::save_key_history(&core, &id);
                    keystore::rewrap_persisted(&core, &id);
                });
                tokio::spawn(mirror::retry_undecryptable(
                    self.clone(),
                    account_id.clone(),
                ));
            }
            Broadcast::VaultChanged { account_id, .. } => {
                let core = self.clone();
                let id = account_id.clone();
                tokio::task::spawn_blocking(move || keystore::save_key_history(&core, &id));
            }
            _ => {}
        }
        let _ = self.events.send(CoreEvent::Keystore(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: &str = "0c0c0c0c-0c0c-4c0c-8c0c-0c0c0c0c0c0c";

    fn core() -> Arc<Core> {
        Core::new(ApiConfig {
            api_base: "https://api.thelemail.test/".parse().unwrap(),
            submission_base: "https://submit.thelemail.test/".parse().unwrap(),
            web_origin: "https://app.thelemail.test".to_owned(),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn keystore_events_reach_core_subscribers() {
        let core = core();
        let mut events = core.subscribe();
        core.start();
        tokio::task::yield_now().await;
        core.keystore().clear(ACCOUNT);
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
            .await
            .expect("an event within two seconds")
            .expect("an open channel");
        assert!(matches!(
            event,
            CoreEvent::Keystore(Broadcast::Cleared { ref account_id }) if account_id == ACCOUNT
        ));
    }
}
