pub mod ids;
pub mod keychain;
pub mod keystore;
pub mod locale;
pub mod net;
pub mod paths;
pub mod session;

use std::sync::Arc;

use thelemail_api::{ApiConfig, Net, TransportError};
use thelemail_keystore::{Broadcast, Keystore};
use tokio::sync::broadcast;

use crate::keystore::PersistentHalves;
use crate::locale::UiLocale;

#[derive(Debug, Clone)]
pub enum CoreEvent {
    Keystore(Broadcast),
    AccountKeysChanged { account_id: String },
}

pub struct Core {
    net: Net,
    keystore: Keystore,
    halves: PersistentHalves,
    locale: UiLocale,
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

    pub(crate) fn halves(&self) -> &PersistentHalves {
        &self.halves
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.events.subscribe()
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
                let _ = self.events.send(CoreEvent::AccountKeysChanged {
                    account_id: account_id.clone(),
                });
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
