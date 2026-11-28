use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;

use thelemail_state::{Latest, Resource};

use crate::api::ApiError;

struct Cell<T> {
    value: Resource<T, ApiError>,
    latest: Latest,
    running: Option<u64>,
    again: bool,
}

impl<T> Default for Cell<T> {
    fn default() -> Self {
        Self {
            value: Resource::NotLoaded,
            latest: Latest::new(),
            running: None,
            again: false,
        }
    }
}

pub struct Store<T> {
    cells: Mutex<HashMap<String, Cell<T>>>,
}

impl<T> Default for Store<T> {
    fn default() -> Self {
        Self {
            cells: Mutex::default(),
        }
    }
}

pub(crate) enum Loaded {
    Changed,
    Unchanged,
}

impl<T: Clone> Store<T> {
    pub fn get(&self, key: &str) -> Resource<T, ApiError> {
        self.cells
            .lock()
            .expect("resource cells")
            .get(key)
            .map(|c| c.value.clone())
            .unwrap_or_default()
    }

    pub fn data(&self, key: &str) -> Option<T> {
        self.get(key).data().cloned()
    }

    pub fn is_loaded(&self, key: &str) -> bool {
        self.cells
            .lock()
            .expect("resource cells")
            .get(key)
            .is_some_and(|c| c.value.data().is_some())
    }

    pub(crate) fn is_started(&self, key: &str) -> bool {
        self.cells
            .lock()
            .expect("resource cells")
            .get(key)
            .is_some_and(|c| !matches!(c.value, Resource::NotLoaded))
    }

    pub fn forget(&self, key: &str) {
        let mut cells = self.cells.lock().expect("resource cells");
        if let Some(mut cell) = cells.remove(key) {
            cell.latest.invalidate();
            let latest = cell.latest;
            cells.insert(
                key.to_owned(),
                Cell {
                    latest,
                    ..Cell::default()
                },
            );
        }
    }

    pub fn forget_all(&self) {
        let keys: Vec<String> = self
            .cells
            .lock()
            .expect("resource cells")
            .keys()
            .cloned()
            .collect();
        for key in keys {
            self.forget(&key);
        }
    }

    pub(crate) fn update(&self, key: &str, f: impl FnOnce(&mut T)) -> bool {
        let mut cells = self.cells.lock().expect("resource cells");
        let Some(cell) = cells.get_mut(key) else {
            return false;
        };
        match &mut cell.value {
            Resource::Loaded(data) | Resource::Refreshing(data) => {
                f(data);
                true
            }
            Resource::Failed {
                data: Some(data), ..
            } => {
                f(data);
                true
            }
            _ => false,
        }
    }

    fn begin(&self, key: &str) -> Option<u64> {
        let mut cells = self.cells.lock().expect("resource cells");
        let cell = cells.entry(key.to_owned()).or_default();
        if let Some(token) = cell.running
            && cell.latest.is_current(token)
        {
            cell.again = true;
            return None;
        }
        let token = cell.latest.begin();
        cell.running = Some(token);
        cell.again = false;
        cell.value.begin();
        Some(token)
    }

    fn finish(
        &self,
        key: &str,
        token: u64,
        result: Result<T, ApiError>,
        merge: &impl Fn(Option<&T>, T) -> T,
    ) -> Option<bool> {
        let mut cells = self.cells.lock().expect("resource cells");
        let cell = cells.get_mut(key)?;
        if !cell.latest.is_current(token) {
            return None;
        }
        cell.running = None;
        match result {
            Ok(data) => {
                let merged = merge(cell.value.data(), data);
                cell.value.succeed(merged);
            }
            Err(err) => cell.value.fail(err),
        }
        Some(std::mem::take(&mut cell.again))
    }

    pub(crate) async fn load<F, Fut>(&self, key: &str, fetch: F) -> Option<Loaded>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, ApiError>>,
    {
        self.load_merging(key, fetch, |_, fresh| fresh).await
    }

    pub(crate) async fn load_merging<F, Fut>(
        &self,
        key: &str,
        fetch: F,
        merge: impl Fn(Option<&T>, T) -> T,
    ) -> Option<Loaded>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, ApiError>>,
    {
        let mut token = self.begin(key)?;
        loop {
            let result = fetch().await;
            match self.finish(key, token, result, &merge) {
                None => return Some(Loaded::Unchanged),
                Some(false) => return Some(Loaded::Changed),
                Some(true) => match self.begin(key) {
                    Some(next) => token = next,
                    None => return Some(Loaded::Changed),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn a_first_load_goes_through_loading_to_loaded() {
        let store = Store::<u32>::default();
        assert_eq!(store.get("a"), Resource::NotLoaded);
        store.load("a", || async { Ok(7) }).await;
        assert_eq!(store.get("a"), Resource::Loaded(7));
    }

    #[tokio::test]
    async fn a_failed_reload_keeps_what_was_shown() {
        let store = Store::<u32>::default();
        store.load("a", || async { Ok(1) }).await;
        store.load("a", || async { Err(ApiError::Network) }).await;
        let value = store.get("a");
        assert_eq!(value.data(), Some(&1));
        assert_eq!(value.error(), Some(&ApiError::Network));
    }

    #[tokio::test]
    async fn a_reload_asked_for_mid_flight_runs_once_more_afterwards() {
        let store = Arc::new(Store::<usize>::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let (release, gate) = tokio::sync::watch::channel(false);
        let first = {
            let store = store.clone();
            let calls = calls.clone();
            tokio::spawn(async move {
                store
                    .load("a", || {
                        let calls = calls.clone();
                        let mut gate = gate.clone();
                        async move {
                            let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
                            if n == 1 {
                                let _ = gate.wait_for(|open| *open).await;
                            }
                            Ok(n)
                        }
                    })
                    .await
            })
        };
        tokio::task::yield_now().await;
        for _ in 0..3 {
            assert!(store.load("a", || async { Ok(99) }).await.is_none());
        }
        release.send(true).unwrap();
        first.await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(store.get("a"), Resource::Loaded(2));
    }

    #[tokio::test]
    async fn a_result_for_a_forgotten_account_is_dropped() {
        let store = Arc::new(Store::<u32>::default());
        let (release, gate) = tokio::sync::watch::channel(false);
        let loading = {
            let store = store.clone();
            tokio::spawn(async move {
                store
                    .load("a", || {
                        let mut gate = gate.clone();
                        async move {
                            let _ = gate.wait_for(|open| *open).await;
                            Ok(5)
                        }
                    })
                    .await
            })
        };
        tokio::task::yield_now().await;
        store.forget("a");
        release.send(true).unwrap();
        assert!(matches!(loading.await.unwrap(), Some(Loaded::Unchanged)));
        assert_eq!(store.get("a"), Resource::NotLoaded);
        store.load("a", || async { Ok(6) }).await;
        assert_eq!(store.get("a"), Resource::Loaded(6));
    }
}
