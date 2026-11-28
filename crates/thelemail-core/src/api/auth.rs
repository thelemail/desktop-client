use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use thelemail_api::Net;
use tokio::sync::{Notify, broadcast, mpsc};
use zeroize::Zeroizing;

use super::client::{Api, Call};
use super::error::ApiError;
use crate::CoreEvent;

pub const REFRESH_SKEW: Duration = Duration::from_secs(60);

struct Slot {
    access: Zeroizing<String>,
    expires_at: Instant,
    csrf: Option<Zeroizing<String>>,
    generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub access: Zeroizing<String>,
    pub generation: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefreshResponse {
    access_token: String,
    expires_in_seconds: u64,
    account_id: String,
    #[serde(default)]
    csrf_token: Option<String>,
}

pub struct Auth {
    slots: Mutex<HashMap<String, Slot>>,
    lost: Mutex<HashSet<String>>,
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    revived: Mutex<HashMap<String, Arc<Notify>>>,
    devices: Mutex<HashMap<String, String>>,
    generation: AtomicU64,
    events: broadcast::Sender<CoreEvent>,
    lifecycle: mpsc::UnboundedSender<String>,
    lifecycle_rx: Mutex<Option<mpsc::UnboundedReceiver<String>>>,
}

impl Auth {
    pub fn new(events: broadcast::Sender<CoreEvent>) -> Self {
        let (lifecycle, rx) = mpsc::unbounded_channel();
        Self {
            slots: Mutex::default(),
            lost: Mutex::default(),
            locks: Mutex::default(),
            revived: Mutex::default(),
            devices: Mutex::default(),
            generation: AtomicU64::new(0),
            events,
            lifecycle,
            lifecycle_rx: Mutex::new(Some(rx)),
        }
    }

    pub fn adopt(&self, account_id: &str, access: &str, expires_in: Duration, csrf: Option<&str>) {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let mut slots = self.slots.lock().expect("auth slots");
        let csrf = csrf
            .map(|c| Zeroizing::new(c.to_owned()))
            .or_else(|| slots.get(account_id).and_then(|s| s.csrf.clone()));
        slots.insert(
            account_id.to_owned(),
            Slot {
                access: Zeroizing::new(access.to_owned()),
                expires_at: Instant::now() + expires_in,
                csrf,
                generation,
            },
        );
        drop(slots);
        self.lost.lock().expect("auth lost").remove(account_id);
        self.revived(account_id).notify_one();
    }

    pub fn forget(&self, account_id: &str) {
        self.slots.lock().expect("auth slots").remove(account_id);
        self.lost.lock().expect("auth lost").remove(account_id);
        self.locks.lock().expect("auth locks").remove(account_id);
        self.devices
            .lock()
            .expect("auth devices")
            .remove(account_id);
    }

    pub fn is_lost(&self, account_id: &str) -> bool {
        self.lost.lock().expect("auth lost").contains(account_id)
    }

    pub fn has_token(&self, account_id: &str) -> bool {
        self.slots
            .lock()
            .expect("auth slots")
            .contains_key(account_id)
    }

    pub fn revived(&self, account_id: &str) -> Arc<Notify> {
        self.revived
            .lock()
            .expect("auth revived")
            .entry(account_id.to_owned())
            .or_default()
            .clone()
    }

    pub fn device(&self, account_id: &str) -> String {
        self.devices
            .lock()
            .expect("auth devices")
            .entry(account_id.to_owned())
            .or_insert_with(|| uuid::Uuid::new_v4().to_string())
            .clone()
    }

    pub(crate) fn csrf(&self, account_id: &str) -> Option<Zeroizing<String>> {
        self.slots
            .lock()
            .expect("auth slots")
            .get(account_id)
            .and_then(|s| s.csrf.clone())
    }

    pub fn take_lifecycle(&self) -> Option<mpsc::UnboundedReceiver<String>> {
        self.lifecycle_rx.lock().expect("auth lifecycle").take()
    }

    pub(crate) fn lifecycle_error(&self, account_id: &str) {
        let _ = self.lifecycle.send(account_id.to_owned());
    }

    fn current(&self, account_id: &str, skew: Duration) -> Option<Token> {
        let slots = self.slots.lock().expect("auth slots");
        let slot = slots.get(account_id)?;
        (slot.expires_at.saturating_duration_since(Instant::now()) > skew).then(|| Token {
            access: slot.access.clone(),
            generation: slot.generation,
        })
    }

    fn generation_of(&self, account_id: &str) -> Option<u64> {
        self.slots
            .lock()
            .expect("auth slots")
            .get(account_id)
            .map(|s| s.generation)
    }

    fn lock(&self, account_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.locks
            .lock()
            .expect("auth locks")
            .entry(account_id.to_owned())
            .or_default()
            .clone()
    }

    pub async fn fresh(&self, net: &Net, account_id: &str) -> Result<Token, ApiError> {
        if self.is_lost(account_id) {
            return Err(ApiError::SessionLost);
        }
        if let Some(token) = self.current(account_id, REFRESH_SKEW) {
            return Ok(token);
        }
        let lock = self.lock(account_id);
        let _held = lock.lock().await;
        if let Some(token) = self.current(account_id, REFRESH_SKEW) {
            return Ok(token);
        }
        self.refresh_locked(net, account_id).await?;
        self.current(account_id, Duration::ZERO)
            .ok_or(ApiError::SessionLost)
    }

    pub async fn renew_after_unauthorized(
        &self,
        net: &Net,
        account_id: &str,
        rejected: u64,
    ) -> Result<(), ApiError> {
        if self.is_lost(account_id) {
            return Err(ApiError::SessionLost);
        }
        let lock = self.lock(account_id);
        let _held = lock.lock().await;
        if self.is_lost(account_id) {
            return Err(ApiError::SessionLost);
        }
        if self
            .generation_of(account_id)
            .is_some_and(|g| g != rejected)
        {
            return Ok(());
        }
        self.refresh_locked(net, account_id).await
    }

    async fn refresh_locked(&self, net: &Net, account_id: &str) -> Result<(), ApiError> {
        let call = Call::post("/v1/auth/refresh")
            .anonymous()
            .header("X-Refresh-Rotation", "1");
        let api = Api::new(net, self);
        let refreshed = Box::pin(api.json::<RefreshResponse>(account_id, call));
        match refreshed.await {
            Ok(res) if res.account_id == account_id => {
                self.adopt(
                    account_id,
                    &res.access_token,
                    Duration::from_secs(res.expires_in_seconds),
                    res.csrf_token.as_deref(),
                );
                Ok(())
            }
            Ok(_) => Err(ApiError::Decode(
                "refresh answered for another account".into(),
            )),
            Err(ApiError::Unauthorized(_)) => {
                self.mark_lost(account_id);
                Err(ApiError::SessionLost)
            }
            Err(err) => Err(err),
        }
    }

    fn mark_lost(&self, account_id: &str) {
        self.slots.lock().expect("auth slots").remove(account_id);
        let newly = self
            .lost
            .lock()
            .expect("auth lost")
            .insert(account_id.to_owned());
        if newly {
            let _ = self.events.send(CoreEvent::SessionLost {
                account_id: account_id.to_owned(),
            });
        }
    }
}
