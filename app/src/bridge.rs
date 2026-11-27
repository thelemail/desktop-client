use std::collections::HashMap;
use std::future::Future;

use gpui_kit::{App, AppContext, Context, Entity, EventEmitter, Global, Task, Window};
use thelemail_core::CoreEvent;
use thelemail_keystore::Broadcast;
use thelemail_ui::state::Latest;
use tokio::sync::broadcast::error::RecvError;

use crate::services::Services;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    account_id: String,
    token: u64,
}

#[derive(Debug, Default)]
pub struct Generations {
    accounts: HashMap<String, Latest>,
}

impl Generations {
    pub fn ticket(&mut self, account_id: &str) -> Ticket {
        let token = self
            .accounts
            .entry(account_id.to_string())
            .or_default()
            .begin();
        Ticket {
            account_id: account_id.to_string(),
            token,
        }
    }

    pub fn is_current(&self, ticket: &Ticket) -> bool {
        self.accounts
            .get(&ticket.account_id)
            .is_some_and(|latest| latest.is_current(ticket.token))
    }

    pub fn invalidate(&mut self, account_id: &str) {
        if let Some(latest) = self.accounts.get_mut(account_id) {
            latest.invalidate();
        }
    }

    pub fn invalidate_all(&mut self) {
        self.accounts.values_mut().for_each(Latest::invalidate);
    }

    fn observe(&mut self, event: &CoreEvent) {
        match event {
            CoreEvent::TokenExpired { account_id }
            | CoreEvent::Keystore(
                Broadcast::Cleared { account_id } | Broadcast::Locked { account_id },
            ) => self.invalidate(account_id),
            CoreEvent::Keystore(Broadcast::ClearedAll) => self.invalidate_all(),
            _ => {}
        }
    }
}

#[derive(Debug, Clone)]
pub enum HubEvent {
    Core(CoreEvent),
    Missed(u64),
}

pub struct CoreHub {
    generations: Generations,
}

impl EventEmitter<HubEvent> for CoreHub {}

impl CoreHub {
    fn dispatch(&mut self, event: HubEvent, cx: &mut Context<Self>) {
        if let HubEvent::Core(event) = &event {
            self.generations.observe(event);
        }
        cx.emit(event);
    }
}

#[derive(Clone)]
pub struct Hub(pub Entity<CoreHub>);

impl Global for Hub {}

pub fn start(cx: &mut App) -> Entity<CoreHub> {
    let hub = cx.new(|_| CoreHub {
        generations: Generations::default(),
    });
    cx.set_global(Hub(hub.clone()));

    let (tx, rx) = async_channel::unbounded::<HubEvent>();
    let services = cx.global::<Services>();
    let mut events = services.core().subscribe();
    services.runtime().spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => HubEvent::Core(event),
                Err(RecvError::Lagged(missed)) => HubEvent::Missed(missed),
                Err(RecvError::Closed) => break,
            };
            if tx.send(event).await.is_err() {
                break;
            }
        }
    });

    let weak = hub.downgrade();
    cx.spawn(async move |cx| {
        while let Ok(event) = rx.recv().await {
            let delivered =
                cx.update(|cx| weak.update(cx, |hub, cx| hub.dispatch(event, cx)).is_ok());
            if !delivered {
                break;
            }
        }
    })
    .detach();
    hub
}

pub fn ticket(cx: &mut App, account_id: &str) -> Ticket {
    let hub = cx.global::<Hub>().0.clone();
    hub.update(cx, |hub, _| hub.generations.ticket(account_id))
}

pub fn invalidate(cx: &mut App, account_id: &str) {
    let hub = cx.global::<Hub>().0.clone();
    hub.update(cx, |hub, _| hub.generations.invalidate(account_id));
}

pub fn request_in<V, T, F>(
    window: &Window,
    cx: &mut Context<V>,
    ticket: Ticket,
    work: F,
    apply: impl FnOnce(&mut V, T, &mut Window, &mut Context<V>) + 'static,
) -> Task<()>
where
    V: 'static,
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let (tx, rx) = async_channel::bounded(1);
    cx.global::<Services>().runtime().spawn(async move {
        let _ = tx.send(work.await).await;
    });
    let hub = cx.global::<Hub>().0.downgrade();
    cx.spawn_in(window, async move |this, cx| {
        let Ok(value) = rx.recv().await else {
            return;
        };
        let _ = this.update_in(cx, |view, window, cx| {
            let current = hub
                .upgrade()
                .is_some_and(|hub| hub.read(cx).generations.is_current(&ticket));
            if current {
                apply(view, value, window, cx);
            }
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0c0c0c0c-0c0c-4c0c-8c0c-0c0c0c0c0c0c";
    const B: &str = "1d1d1d1d-1d1d-4d1d-8d1d-1d1d1d1d1d1d";

    #[test]
    fn a_newer_ticket_retires_the_older_one_for_that_account_only() {
        let mut g = Generations::default();
        let a1 = g.ticket(A);
        let b1 = g.ticket(B);
        let a2 = g.ticket(A);
        assert!(!g.is_current(&a1));
        assert!(g.is_current(&a2));
        assert!(g.is_current(&b1));
    }

    #[test]
    fn invalidating_an_account_drops_its_running_work() {
        let mut g = Generations::default();
        let a = g.ticket(A);
        let b = g.ticket(B);
        g.invalidate(A);
        assert!(!g.is_current(&a));
        assert!(g.is_current(&b));
    }

    #[test]
    fn a_ticket_for_an_unknown_account_is_never_current() {
        let g = Generations::default();
        let ticket = Ticket {
            account_id: A.into(),
            token: 1,
        };
        assert!(!g.is_current(&ticket));
    }

    #[test]
    fn sign_out_lock_and_expiry_invalidate_the_account() {
        for event in [
            CoreEvent::TokenExpired {
                account_id: A.into(),
            },
            CoreEvent::Keystore(Broadcast::Cleared {
                account_id: A.into(),
            }),
            CoreEvent::Keystore(Broadcast::Locked {
                account_id: A.into(),
            }),
        ] {
            let mut g = Generations::default();
            let a = g.ticket(A);
            let b = g.ticket(B);
            g.observe(&event);
            assert!(!g.is_current(&a));
            assert!(g.is_current(&b));
        }
    }

    #[test]
    fn clearing_every_account_invalidates_all_of_them() {
        let mut g = Generations::default();
        let a = g.ticket(A);
        let b = g.ticket(B);
        g.observe(&CoreEvent::Keystore(Broadcast::ClearedAll));
        assert!(!g.is_current(&a));
        assert!(!g.is_current(&b));
    }

    #[test]
    fn mirror_traffic_leaves_tickets_alone() {
        let mut g = Generations::default();
        let a = g.ticket(A);
        g.observe(&CoreEvent::MirrorChanged {
            account_id: A.into(),
            arrived: 3,
        });
        assert!(g.is_current(&a));
    }
}
