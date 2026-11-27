mod normalize;
mod policy;

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use serde::Deserialize;
use thelemail_api::{ApiRequest, ApiResponse};
use thelemail_store::avatars::{CachedAvatar, cached_avatar, prune_avatars, store_avatar};
use tokio::sync::Notify;

use policy::ImageFormat;
pub use thelemail_store::avatars::AvatarKind;

use crate::{Core, CoreEvent};

const BATCH_WINDOW: Duration = Duration::from_millis(20);
const RESOLVE_BATCH: usize = 100;
const MEMORY_MAX: usize = 400;
const MEMORY_KEEP: usize = 300;
const DISK_MAX: usize = 2000;

pub use normalize::EDGE;

#[derive(Debug, Clone)]
pub struct AvatarImage {
    pub png: Arc<[u8]>,
    pub id: u64,
}

impl AvatarImage {
    fn new(png: Vec<u8>) -> Self {
        let mut hasher = DefaultHasher::new();
        png.hash(&mut hasher);
        Self {
            png: png.into(),
            id: hasher.finish(),
        }
    }

    fn from_cache(cached: &CachedAvatar) -> Option<Self> {
        if ImageFormat::parse(&cached.format) != Some(ImageFormat::Png) {
            return None;
        }
        let bytes = cached.bytes.clone()?;
        (policy::photo(&bytes) == Some(ImageFormat::Png)).then(|| Self::new(bytes))
    }
}

fn normalized(source: &str, png: Option<Vec<u8>>) -> CachedAvatar {
    match png {
        Some(png) => CachedAvatar {
            source: source.to_owned(),
            format: ImageFormat::Png.as_str().to_owned(),
            bytes: Some(png),
            checked_at: now_unix(),
        },
        None => negative(source),
    }
}

async fn off_thread(work: impl FnOnce() -> Option<Vec<u8>> + Send + 'static) -> Option<Vec<u8>> {
    tokio::task::spawn_blocking(work).await.ok().flatten()
}

#[derive(Debug, Clone)]
pub enum Lookup {
    Ready(Option<AvatarImage>),
    Pending,
}

impl Lookup {
    pub fn image(&self) -> Option<&AvatarImage> {
        match self {
            Self::Ready(image) => image.as_ref(),
            Self::Pending => None,
        }
    }
}

type Key = (AvatarKind, String);

#[derive(Debug, Clone)]
struct Entry {
    image: Option<AvatarImage>,
    checked_at: i64,
    used: u64,
}

#[derive(Debug, Default)]
struct Account {
    generation: u64,
    entries: HashMap<Key, Entry>,
    queued: HashSet<Key>,
    failed: HashMap<Key, i64>,
}

#[derive(Debug)]
struct Job {
    account: String,
    generation: u64,
    kind: AvatarKind,
    key: String,
}

#[derive(Debug, Default)]
struct State {
    accounts: HashMap<String, Account>,
    queue: VecDeque<Job>,
    tick: u64,
    generations: u64,
}

impl State {
    fn account(&mut self, id: &str) -> &mut Account {
        if !self.accounts.contains_key(id) {
            self.generations += 1;
            let account = Account {
                generation: self.generations,
                ..Account::default()
            };
            self.accounts.insert(id.to_owned(), account);
        }
        self.accounts
            .get_mut(id)
            .expect("account was just inserted")
    }
}

#[derive(Debug, Default)]
pub struct Avatars {
    state: Mutex<State>,
    wake: Notify,
}

fn refresh_every(kind: AvatarKind, entry: &Entry) -> Option<i64> {
    match kind {
        AvatarKind::Person => Some(policy::PERSON_REFRESH),
        AvatarKind::Bimi => Some(policy::BIMI_REFRESH),
        AvatarKind::Account if entry.image.is_none() => Some(policy::PERSON_REFRESH),
        AvatarKind::Account => None,
    }
}

fn person_key(address: &str) -> String {
    address.trim().to_lowercase()
}

impl Avatars {
    pub fn person(&self, account_id: &str, address: &str) -> Lookup {
        let key = person_key(address);
        if key.is_empty() {
            return Lookup::Ready(None);
        }
        self.lookup(account_id, AvatarKind::Person, key)
    }

    pub fn bimi(&self, account_id: &str, domain: &str) -> Lookup {
        match crate::sender::bimi_domain(Some(domain)) {
            Some(key) => self.lookup(account_id, AvatarKind::Bimi, key),
            None => Lookup::Ready(None),
        }
    }

    pub fn account_avatar(&self, account_id: &str, avatar_path: &str) -> Lookup {
        if !avatar_path.starts_with("/v1/avatars/") {
            return Lookup::Ready(None);
        }
        self.lookup(account_id, AvatarKind::Account, avatar_path.to_owned())
    }

    pub fn forget(&self, account_id: &str) {
        let mut state = self.state.lock().expect("avatar state");
        state.accounts.remove(account_id);
        state.queue.retain(|job| job.account != account_id);
    }

    pub fn forget_all(&self) {
        let mut state = self.state.lock().expect("avatar state");
        state.accounts.clear();
        state.queue.clear();
    }

    fn lookup(&self, account_id: &str, kind: AvatarKind, key: String) -> Lookup {
        let now = now_unix();
        let mut state = self.state.lock().expect("avatar state");
        state.tick += 1;
        let tick = state.tick;
        let account = state.account(account_id);
        let generation = account.generation;
        let id = (kind, key);
        let (found, stale) = match account.entries.get_mut(&id) {
            Some(entry) => {
                entry.used = tick;
                let stale = refresh_every(kind, entry).is_some_and(|every| {
                    policy::freshness(entry.checked_at, now, every) == policy::Next::Refresh
                });
                (Some(Lookup::Ready(entry.image.clone())), stale)
            }
            None => (None, true),
        };
        let waiting = account.queued.contains(&id)
            || account
                .failed
                .get(&id)
                .is_some_and(|at| now.saturating_sub(*at) < policy::RETRY_AFTER);
        let enqueue = stale && !waiting;
        if enqueue {
            account.queued.insert(id.clone());
        }
        let (kind, key) = id;
        if enqueue {
            state.queue.push_back(Job {
                account: account_id.to_owned(),
                generation,
                kind,
                key,
            });
            self.wake.notify_one();
        }
        found.unwrap_or(Lookup::Pending)
    }

    fn current(&self, job_account: &str, generation: u64) -> bool {
        let state = self.state.lock().expect("avatar state");
        state
            .accounts
            .get(job_account)
            .is_some_and(|a| a.generation == generation)
    }

    fn finish(
        &self,
        account_id: &str,
        generation: u64,
        kind: AvatarKind,
        key: &str,
        outcome: Outcome,
    ) {
        let now = now_unix();
        let mut state = self.state.lock().expect("avatar state");
        state.tick += 1;
        let tick = state.tick;
        let Some(account) = state.accounts.get_mut(account_id) else {
            return;
        };
        if account.generation != generation {
            return;
        }
        let id = (kind, key.to_owned());
        account.queued.remove(&id);
        match outcome {
            Outcome::Known(entry) => {
                account.failed.remove(&id);
                account.entries.insert(
                    id,
                    Entry {
                        used: tick,
                        ..entry
                    },
                );
            }
            Outcome::Failed => {
                account.failed.insert(id, now);
            }
        }
        if account.entries.len() > MEMORY_MAX {
            let mut by_use: Vec<(u64, Key)> = account
                .entries
                .iter()
                .map(|(k, e)| (e.used, k.clone()))
                .collect();
            by_use.sort_unstable_by_key(|(used, _)| *used);
            let drop = account.entries.len() - MEMORY_KEEP;
            for (_, k) in by_use.into_iter().take(drop) {
                account.entries.remove(&k);
            }
        }
    }

    fn take_jobs(&self) -> Vec<Job> {
        self.state
            .lock()
            .expect("avatar state")
            .queue
            .drain(..)
            .collect()
    }
}

enum Outcome {
    Known(Entry),
    Failed,
}

pub fn start(core: &Arc<Core>) {
    let core = core.clone();
    tokio::spawn(async move {
        loop {
            core.avatars().wake.notified().await;
            tokio::time::sleep(BATCH_WINDOW).await;
            let mut by_account: HashMap<(String, u64), Vec<Job>> = HashMap::new();
            for job in core.avatars().take_jobs() {
                by_account
                    .entry((job.account.clone(), job.generation))
                    .or_default()
                    .push(job);
            }
            for ((account, generation), jobs) in by_account {
                tokio::spawn(process(core.clone(), account, generation, jobs));
            }
        }
    });
}

struct Pending {
    kind: AvatarKind,
    key: String,
    cached: Option<CachedAvatar>,
}

async fn process(core: Arc<Core>, account: String, generation: u64, jobs: Vec<Job>) {
    let lookups: Vec<(AvatarKind, String)> = jobs.into_iter().map(|j| (j.kind, j.key)).collect();
    let keys = lookups.clone();
    let reader = core.clone();
    let id = account.clone();
    let read = tokio::task::spawn_blocking(move || {
        let now = now_unix();
        reader.mirror().with_conn(&id, |conn| {
            Ok(lookups
                .into_iter()
                .map(|(kind, key)| {
                    let cached = cached_avatar(conn, kind, &key, now).ok().flatten();
                    Pending { kind, key, cached }
                })
                .collect::<Vec<_>>())
        })
    })
    .await;
    let pending = match read {
        Ok(Ok(pending)) => pending,
        _ => {
            for (kind, key) in keys {
                core.avatars()
                    .finish(&account, generation, kind, &key, Outcome::Failed);
            }
            return;
        }
    };
    let now = now_unix();
    let avatars = core.avatars();
    let mut network = Vec::new();
    let mut changed = false;
    for item in pending {
        if let Some(cached) = &item.cached {
            let entry = Entry {
                image: AvatarImage::from_cache(cached),
                checked_at: cached.checked_at,
                used: 0,
            };
            let fresh = refresh_every(item.kind, &entry).is_none_or(|every| {
                policy::freshness(entry.checked_at, now, every) == policy::Next::Fresh
            });
            avatars.finish(
                &account,
                generation,
                item.kind,
                &item.key,
                Outcome::Known(entry),
            );
            changed = true;
            if fresh {
                continue;
            }
            requeue(avatars, &account, generation, item.kind, &item.key);
        }
        network.push(item);
    }
    if changed {
        core.emit(CoreEvent::AvatarsChanged {
            account_id: account.clone(),
        });
    }
    if network.is_empty() || !avatars.current(&account, generation) {
        return;
    }
    let Some(token) = core.mirror().token(&account) else {
        for item in &network {
            avatars.finish(&account, generation, item.kind, &item.key, Outcome::Failed);
        }
        return;
    };
    let client = Client {
        core: &core,
        account: &account,
        token: &token,
    };
    let mut people = Vec::new();
    let mut results = Vec::new();
    for item in network {
        match item.kind {
            AvatarKind::Person => people.push(item),
            AvatarKind::Bimi => {
                let outcome = client.bimi(&item.key).await;
                results.push((item.kind, item.key, outcome));
            }
            AvatarKind::Account => {
                let outcome = client.image(&item.key, item.cached.as_ref()).await;
                results.push((item.kind, item.key, outcome));
            }
        }
    }
    for chunk in people.chunks(RESOLVE_BATCH) {
        for (key, outcome) in client.people(chunk).await {
            results.push((AvatarKind::Person, key, outcome));
        }
    }
    persist(&core, &account, generation, results).await;
}

fn requeue(avatars: &Avatars, account: &str, generation: u64, kind: AvatarKind, key: &str) {
    let mut state = avatars.state.lock().expect("avatar state");
    if let Some(a) = state.accounts.get_mut(account)
        && a.generation == generation
    {
        a.queued.insert((kind, key.to_owned()));
    }
}

enum Fetched {
    Found(CachedAvatar),
    Unauthorized,
    Failed,
}

async fn persist(
    core: &Arc<Core>,
    account: &str,
    generation: u64,
    results: Vec<(AvatarKind, String, Fetched)>,
) {
    let avatars = core.avatars();
    if !avatars.current(account, generation) {
        return;
    }
    let mut stored = Vec::new();
    let mut expired = false;
    for (kind, key, fetched) in results {
        match fetched {
            Fetched::Found(cached) => {
                let entry = Entry {
                    image: AvatarImage::from_cache(&cached),
                    checked_at: cached.checked_at,
                    used: 0,
                };
                avatars.finish(account, generation, kind, &key, Outcome::Known(entry));
                stored.push((kind, key, cached));
            }
            Fetched::Unauthorized => {
                expired = true;
                avatars.finish(account, generation, kind, &key, Outcome::Failed);
            }
            Fetched::Failed => avatars.finish(account, generation, kind, &key, Outcome::Failed),
        }
    }
    if expired {
        core.emit(CoreEvent::TokenExpired {
            account_id: account.to_owned(),
        });
    }
    if stored.is_empty() {
        return;
    }
    let writer = core.clone();
    let id = account.to_owned();
    let written = tokio::task::spawn_blocking(move || {
        let now = now_unix();
        writer.mirror().with_conn(&id, |conn| {
            for (kind, key, cached) in &stored {
                store_avatar(conn, *kind, key, cached, now).map_err(|e| e.to_string())?;
            }
            prune_avatars(conn, DISK_MAX).map_err(|e| e.to_string())?;
            Ok(())
        })
    })
    .await;
    if let Ok(Err(err)) | Err(err) = written.map_err(|e| e.to_string()) {
        tracing::warn!("avatars: caching failed: {err}");
    }
    core.emit(CoreEvent::AvatarsChanged {
        account_id: account.to_owned(),
    });
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Resolved {
    address: String,
    avatar_path: Option<String>,
}

#[derive(Deserialize)]
struct ResolveResponse {
    #[serde(default)]
    avatars: Vec<Resolved>,
}

#[derive(Deserialize)]
struct LogoResponse {
    #[serde(default)]
    svg: String,
}

struct Client<'a> {
    core: &'a Core,
    account: &'a str,
    token: &'a str,
}

fn negative(source: &str) -> CachedAvatar {
    CachedAvatar {
        source: source.to_owned(),
        format: String::new(),
        bytes: None,
        checked_at: now_unix(),
    }
}

impl Client<'_> {
    fn headers(&self, json: bool) -> HashMap<String, String> {
        let mut headers = crate::mirror::auth_headers(self.account, self.token);
        if json {
            headers.insert("Content-Type".to_owned(), "application/json".to_owned());
        }
        headers
    }

    fn url(&self, path: &str) -> Option<String> {
        let mut url = self.core.net().config().api_base.clone();
        url.set_path(path);
        Some(url.to_string())
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> Option<ApiResponse> {
        let url = self.url(path)?;
        crate::net::api_request(
            self.core,
            ApiRequest {
                url,
                method: "POST".to_owned(),
                headers: self.headers(true),
                body: Some(body.to_string()),
            },
        )
        .await
        .ok()
    }

    async fn people(&self, items: &[Pending]) -> Vec<(String, Fetched)> {
        let addresses: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        let resp = self
            .post(
                "/v1/accounts/avatars",
                serde_json::json!({ "addresses": addresses }),
            )
            .await;
        let found: HashMap<String, Option<String>> = match resp {
            Some(r) if r.status == 401 => {
                return items
                    .iter()
                    .map(|i| (i.key.clone(), Fetched::Unauthorized))
                    .collect();
            }
            Some(r) if r.status == 200 => match r
                .body
                .and_then(|b| serde_json::from_slice::<ResolveResponse>(&b).ok())
            {
                Some(parsed) => parsed
                    .avatars
                    .into_iter()
                    .map(|a| (person_key(&a.address), a.avatar_path))
                    .collect(),
                None => {
                    return items
                        .iter()
                        .map(|i| (i.key.clone(), Fetched::Failed))
                        .collect();
                }
            },
            _ => {
                return items
                    .iter()
                    .map(|i| (i.key.clone(), Fetched::Failed))
                    .collect();
            }
        };
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let fetched = match found.get(&item.key).cloned().flatten() {
                Some(path) => self.image(&path, item.cached.as_ref()).await,
                None => Fetched::Found(negative("")),
            };
            out.push((item.key.clone(), fetched));
        }
        out
    }

    async fn image(&self, path: &str, cached: Option<&CachedAvatar>) -> Fetched {
        if !path.starts_with("/v1/avatars/") {
            return Fetched::Found(negative(""));
        }
        let has_image = cached.is_some_and(|c| c.bytes.is_some());
        if !policy::needs_download(cached.map(|c| c.source.as_str()), has_image, path) {
            let mut kept = cached.cloned().expect("a cached image is present");
            kept.checked_at = now_unix();
            return Fetched::Found(kept);
        }
        let Some(url) = self.url(path) else {
            return Fetched::Failed;
        };
        let resp = crate::net::blob_request(
            self.core,
            ApiRequest {
                url,
                method: "GET".to_owned(),
                headers: self.headers(false),
                body: None,
            },
        )
        .await;
        match resp {
            Ok(r) if r.status == 401 => Fetched::Unauthorized,
            Ok(r) if r.status == 404 => Fetched::Found(negative(path)),
            Ok(r) if r.status == 200 => {
                let bytes = r.body.unwrap_or_default();
                Fetched::Found(normalized(
                    path,
                    off_thread(move || normalize::photo(&bytes)).await,
                ))
            }
            _ => Fetched::Failed,
        }
    }

    async fn bimi(&self, domain: &str) -> Fetched {
        let resp = self
            .post("/v1/bimi/logo", serde_json::json!({ "domain": domain }))
            .await;
        match resp {
            Some(r) if r.status == 401 => Fetched::Unauthorized,
            Some(r) if r.status == 404 => Fetched::Found(negative("")),
            Some(r) if r.status == 200 => {
                let svg = r
                    .body
                    .and_then(|b| serde_json::from_slice::<LogoResponse>(&b).ok())
                    .and_then(|l| base64::engine::general_purpose::STANDARD.decode(l.svg).ok());
                let png = match svg {
                    Some(svg) => off_thread(move || normalize::logo(&svg)).await,
                    None => None,
                };
                Fetched::Found(normalized("", png))
            }
            _ => Fetched::Failed,
        }
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: &str = "0c0c0c0c-0c0c-4c0c-8c0c-0c0c0c0c0c0c";

    fn photo_entry() -> Entry {
        Entry {
            image: Some(AvatarImage::new(b"\x89PNG\r\n\x1a\n".to_vec())),
            checked_at: now_unix(),
            used: 0,
        }
    }

    fn generation(avatars: &Avatars) -> u64 {
        avatars.state.lock().unwrap().accounts[ACCOUNT].generation
    }

    #[test]
    fn a_miss_is_pending_and_queued_once() {
        let avatars = Avatars::default();
        assert!(matches!(
            avatars.person(ACCOUNT, "Ada@Acme.test"),
            Lookup::Pending
        ));
        assert!(matches!(
            avatars.person(ACCOUNT, "ada@acme.test "),
            Lookup::Pending
        ));
        let jobs = avatars.take_jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].key, "ada@acme.test");
    }

    #[test]
    fn a_known_image_is_ready_without_new_work() {
        let avatars = Avatars::default();
        avatars.person(ACCOUNT, "ada@acme.test");
        let g = generation(&avatars);
        avatars.take_jobs();
        avatars.finish(
            ACCOUNT,
            g,
            AvatarKind::Person,
            "ada@acme.test",
            Outcome::Known(photo_entry()),
        );
        assert!(avatars.person(ACCOUNT, "ada@acme.test").image().is_some());
        assert!(avatars.take_jobs().is_empty());
    }

    #[test]
    fn a_known_absence_is_ready_as_none() {
        let avatars = Avatars::default();
        avatars.bimi(ACCOUNT, "acme.test");
        let g = generation(&avatars);
        avatars.take_jobs();
        let none = Entry {
            image: None,
            checked_at: now_unix(),
            used: 0,
        };
        avatars.finish(
            ACCOUNT,
            g,
            AvatarKind::Bimi,
            "acme.test",
            Outcome::Known(none),
        );
        assert!(matches!(
            avatars.bimi(ACCOUNT, "ACME.test"),
            Lookup::Ready(None)
        ));
    }

    #[test]
    fn a_failure_waits_before_trying_again() {
        let avatars = Avatars::default();
        avatars.person(ACCOUNT, "ada@acme.test");
        let g = generation(&avatars);
        avatars.take_jobs();
        avatars.finish(
            ACCOUNT,
            g,
            AvatarKind::Person,
            "ada@acme.test",
            Outcome::Failed,
        );
        assert!(matches!(
            avatars.person(ACCOUNT, "ada@acme.test"),
            Lookup::Pending
        ));
        assert!(avatars.take_jobs().is_empty());
    }

    #[test]
    fn results_for_a_forgotten_account_are_dropped() {
        let avatars = Avatars::default();
        avatars.person(ACCOUNT, "ada@acme.test");
        let old = generation(&avatars);
        avatars.forget(ACCOUNT);
        avatars.person(ACCOUNT, "ada@acme.test");
        avatars.finish(
            ACCOUNT,
            old,
            AvatarKind::Person,
            "ada@acme.test",
            Outcome::Known(photo_entry()),
        );
        assert!(matches!(
            avatars.person(ACCOUNT, "ada@acme.test"),
            Lookup::Pending
        ));
    }

    #[test]
    fn a_stale_entry_is_shown_while_it_refreshes() {
        let avatars = Avatars::default();
        avatars.person(ACCOUNT, "ada@acme.test");
        let g = generation(&avatars);
        avatars.take_jobs();
        let mut entry = photo_entry();
        entry.checked_at -= policy::PERSON_REFRESH + 1;
        avatars.finish(
            ACCOUNT,
            g,
            AvatarKind::Person,
            "ada@acme.test",
            Outcome::Known(entry),
        );
        assert!(avatars.person(ACCOUNT, "ada@acme.test").image().is_some());
        assert_eq!(avatars.take_jobs().len(), 1);
    }

    #[test]
    fn memory_stays_bounded() {
        let avatars = Avatars::default();
        avatars.person(ACCOUNT, "seed@acme.test");
        let g = generation(&avatars);
        for i in 0..(MEMORY_MAX + 10) {
            let key = format!("p{i}@acme.test");
            avatars.finish(
                ACCOUNT,
                g,
                AvatarKind::Person,
                &key,
                Outcome::Known(photo_entry()),
            );
        }
        let size = avatars.state.lock().unwrap().accounts[ACCOUNT]
            .entries
            .len();
        assert!(size <= MEMORY_MAX);
    }

    #[test]
    fn only_avatar_paths_are_ever_fetched_for_an_account_image() {
        let avatars = Avatars::default();
        assert!(matches!(
            avatars.account_avatar(ACCOUNT, "/v1/messages/x"),
            Lookup::Ready(None)
        ));
        assert!(matches!(
            avatars.account_avatar(ACCOUNT, "https://evil.test/a.png"),
            Lookup::Ready(None)
        ));
        assert!(avatars.take_jobs().is_empty());
    }

    #[test]
    fn a_cached_row_that_fails_validation_is_not_shown() {
        let bad = CachedAvatar {
            source: "/v1/avatars/a/1".into(),
            format: "png".into(),
            bytes: Some(b"<svg/>".to_vec()),
            checked_at: 1,
        };
        assert!(AvatarImage::from_cache(&bad).is_none());
        let svg = CachedAvatar {
            source: String::new(),
            format: "svg".into(),
            bytes: Some(br#"<svg viewBox="0 0 1 1"/>"#.to_vec()),
            checked_at: 1,
        };
        assert!(AvatarImage::from_cache(&svg).is_none());
    }
}
