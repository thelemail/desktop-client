mod dispatch;
mod hint;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Deserialize;
use tokio::sync::oneshot;

pub use self::dispatch::{Effects, effects};
pub use self::hint::{Action, Entity, Hint, HintKind};

use crate::api::{ApiError, Call};
use crate::{Core, CoreEvent};

const BASE_DELAY: Duration = Duration::from_secs(1);
const MAX_DELAY: Duration = Duration::from_secs(30);
const JITTER: f64 = 0.3;
const MAX_FRAME_BYTES: usize = 256 * 1024;
pub const STALE: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct Realtime {
    open: Mutex<HashMap<String, oneshot::Sender<()>>>,
}

impl Realtime {
    pub fn close(&self, account_id: &str) {
        if let Some(stop) = self.open.lock().expect("realtime").remove(account_id) {
            let _ = stop.send(());
        }
    }

    pub fn close_all(&self) {
        for (_, stop) in self.open.lock().expect("realtime").drain() {
            let _ = stop.send(());
        }
    }

    fn claim(&self, account_id: &str) -> Option<oneshot::Receiver<()>> {
        let mut open = self.open.lock().expect("realtime");
        if open.get(account_id).is_some_and(|stop| !stop.is_closed()) {
            return None;
        }
        let (stop, stopped) = oneshot::channel();
        open.insert(account_id.to_owned(), stop);
        Some(stopped)
    }
}

pub fn open(core: &Arc<Core>, account_id: &str) -> Result<(), String> {
    crate::ids::account_id(account_id)?;
    let Some(stopped) = core.realtime().claim(account_id) else {
        return Ok(());
    };
    tokio::spawn(run(core.clone(), account_id.to_owned(), stopped));
    Ok(())
}

pub fn next_delay(attempt: u32, random: f64) -> Duration {
    let raw = BASE_DELAY
        .saturating_mul(2u32.saturating_pow(attempt.min(16)))
        .min(MAX_DELAY);
    let spread = raw.as_secs_f64() * JITTER;
    let delta = (random.clamp(0.0, 1.0) * 2.0 - 1.0) * spread;
    Duration::from_secs_f64((raw.as_secs_f64() + delta).max(0.0))
}

enum Ticket {
    Minted(String),
    Lost,
    Unavailable,
    Retry,
}

#[derive(Deserialize)]
struct TicketResponse {
    ticket: String,
}

async fn mint(core: &Core, account_id: &str) -> Ticket {
    match core
        .api()
        .json::<TicketResponse>(account_id, Call::post("/v1/realtime/ticket"))
        .await
    {
        Ok(t) => Ticket::Minted(t.ticket),
        Err(ApiError::SessionLost) => Ticket::Lost,
        Err(ApiError::NotFound(_)) => Ticket::Unavailable,
        Err(_) => Ticket::Retry,
    }
}

fn stream_url(core: &Core, ticket: &str, since: Option<&str>) -> String {
    let mut url = core.net().config().api_base.clone();
    url.set_path("v1/realtime/stream");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("ticket", ticket);
        if let Some(since) = since {
            query.append_pair("since", since);
        }
    }
    url.to_string()
}

fn state(core: &Core, account_id: &str, open: bool) {
    core.emit(CoreEvent::RealtimeState {
        account_id: account_id.to_owned(),
        open,
    });
}

pub fn apply(core: &Arc<Core>, hint: Hint) {
    let wanted = effects(&hint.kind);
    let account_id = hint.account_id.clone();
    core.emit(CoreEvent::Hint(hint));
    if wanted.poke_mirror {
        crate::mirror::poke(core, &account_id);
    }
    refresh(core, &account_id, wanted);
}

fn refresh(core: &Arc<Core>, account_id: &str, wanted: Effects) {
    let account = core.account();
    let profile = wanted.profile;
    let settings = wanted.settings && account.has_settings(account_id);
    let billing = wanted.billing && account.has_subscription(account_id);
    spawn_loads(core, account_id, profile, settings, billing);
}

fn spawn_loads(core: &Arc<Core>, account_id: &str, profile: bool, settings: bool, billing: bool) {
    if !(profile || settings || billing) {
        return;
    }
    let core = core.clone();
    let account_id = account_id.to_owned();
    tokio::spawn(async move {
        let profile = async {
            if profile {
                crate::account::load_profile(&core, &account_id).await;
            }
        };
        let settings = async {
            if settings {
                crate::account::load_settings(&core, &account_id).await;
            }
        };
        let billing = async {
            if billing {
                crate::account::load_billing(&core, &account_id).await;
            }
        };
        tokio::join!(profile, settings, billing);
    });
}

fn resync(core: &Arc<Core>, account_id: &str, down: Duration) {
    crate::mirror::poke(core, account_id);
    let account = core.account();
    spawn_loads(
        core,
        account_id,
        account.has_profile(account_id),
        account.has_settings(account_id),
        account.has_subscription(account_id),
    );
    core.emit(CoreEvent::RealtimeResync {
        account_id: account_id.to_owned(),
        full: down >= STALE,
    });
}

async fn run(core: Arc<Core>, account_id: String, mut stopped: oneshot::Receiver<()>) {
    let mut attempt: u32 = 0;
    let mut since: Option<String> = None;
    let mut closed_at: Option<Instant> = None;
    loop {
        let ticket = match mint(&core, &account_id).await {
            Ticket::Minted(ticket) => ticket,
            Ticket::Unavailable => break,
            Ticket::Lost => {
                let revived = core.auth().revived(&account_id);
                tokio::select! {
                    _ = &mut stopped => return,
                    () = revived.notified() => continue,
                }
            }
            Ticket::Retry => {
                closed_at.get_or_insert_with(Instant::now);
                if wait(&mut stopped, &mut attempt).await {
                    return;
                }
                continue;
            }
        };

        let url = stream_url(&core, &ticket, since.as_deref());
        match core.net().stream_events(&url).await {
            Ok(resp) if resp.status().is_success() => {
                state(&core, &account_id, true);
                attempt = 0;
                if let Some(at) = closed_at.take() {
                    resync(&core, &account_id, at.elapsed());
                }
                let ended = read(&core, &account_id, resp, &mut since, &mut stopped).await;
                state(&core, &account_id, false);
                if ended == Ended::Stopped {
                    return;
                }
            }
            Ok(resp) if resp.status().as_u16() == 404 => break,
            _ => {}
        }
        closed_at.get_or_insert_with(Instant::now);
        if wait(&mut stopped, &mut attempt).await {
            return;
        }
    }
    core.realtime()
        .open
        .lock()
        .expect("realtime")
        .remove(&account_id);
}

async fn wait(stopped: &mut oneshot::Receiver<()>, attempt: &mut u32) -> bool {
    let delay = next_delay(*attempt, rand::random::<f64>());
    *attempt = attempt.saturating_add(1);
    tokio::select! {
        _ = stopped => true,
        _ = tokio::time::sleep(delay) => false,
    }
}

#[derive(PartialEq, Eq)]
enum Ended {
    Dropped,
    Stopped,
}

async fn read(
    core: &Arc<Core>,
    account_id: &str,
    resp: reqwest::Response,
    since: &mut Option<String>,
    stopped: &mut oneshot::Receiver<()>,
) -> Ended {
    let mut body = resp.bytes_stream();
    let mut buffer = String::new();
    loop {
        tokio::select! {
            _ = &mut *stopped => return Ended::Stopped,
            chunk = body.next() => {
                let Some(Ok(bytes)) = chunk else { return Ended::Dropped };
                buffer.push_str(&String::from_utf8_lossy(&bytes));
                while let Some(split) = find_frame_end(&buffer) {
                    let raw = buffer[..split].to_owned();
                    buffer.drain(..split + frame_sep_len(&buffer, split));
                    if let Some((data, id)) = parse_frame(&raw) {
                        if id.is_some() {
                            since.clone_from(&id);
                        }
                        if let Some(hint) = Hint::parse(account_id, &data) {
                            apply(core, hint);
                        }
                    }
                }
                if buffer.len() > MAX_FRAME_BYTES {
                    tracing::warn!("realtime: a frame for {account_id} passed {MAX_FRAME_BYTES} bytes, reconnecting");
                    return Ended::Dropped;
                }
            }
        }
    }
}

fn find_frame_end(buffer: &str) -> Option<usize> {
    let lf = buffer.find("\n\n");
    let crlf = buffer.find("\r\n\r\n");
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn frame_sep_len(buffer: &str, at: usize) -> usize {
    if buffer[at..].starts_with("\r\n\r\n") {
        4
    } else {
        2
    }
}

fn parse_frame(raw: &str) -> Option<(String, Option<String>)> {
    let mut data = String::new();
    let mut id = None;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim_start());
        } else if let Some(rest) = line.strip_prefix("id:") {
            id = Some(rest.trim().to_owned());
        }
    }
    if data.is_empty() {
        return None;
    }
    Some((data, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use thelemail_api::ApiConfig;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const ACCOUNT: &str = "0c0c0c0c-0c0c-4c0c-8c0c-0c0c0c0c0c0c";

    #[test]
    fn the_delay_doubles_from_one_second_to_thirty_with_thirty_percent_jitter() {
        assert_eq!(next_delay(0, 0.5), Duration::from_secs(1));
        assert_eq!(next_delay(1, 0.5), Duration::from_secs(2));
        assert_eq!(next_delay(4, 0.5), Duration::from_secs(16));
        assert_eq!(next_delay(5, 0.5), Duration::from_secs(30));
        assert_eq!(next_delay(40, 0.5), Duration::from_secs(30));
        assert_eq!(next_delay(0, 0.0), Duration::from_millis(700));
        assert_eq!(next_delay(0, 1.0), Duration::from_millis(1300));
        assert_eq!(next_delay(9, 1.0), Duration::from_secs(39));
    }

    #[test]
    fn frames_join_data_lines_and_keep_the_last_id() {
        assert_eq!(
            parse_frame(": comment\nid: 4\ndata: {\"a\":\ndata: 1}"),
            Some(("{\"a\":\n1}".to_owned(), Some("4".to_owned())))
        );
        assert_eq!(parse_frame("id: 5"), None);
        assert_eq!(find_frame_end("data: x\r\n\r\nrest"), Some(7));
        assert_eq!(frame_sep_len("data: x\r\n\r\n", 7), 4);
    }

    struct Seen {
        lines: Vec<String>,
    }

    async fn serve(listener: TcpListener, script: Vec<(&'static str, String)>) -> Seen {
        let mut seen = Seen { lines: Vec::new() };
        for (expect, reply) in script {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 8192];
            let n = socket.read(&mut buf).await.expect("read");
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = request.lines().next().unwrap_or_default().to_owned();
            assert!(line.starts_with(expect), "expected {expect}, got {line}");
            seen.lines.push(line);
            socket.write_all(reply.as_bytes()).await.expect("write");
            let _ = socket.shutdown().await;
        }
        seen
    }

    fn json(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn events(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}"
        )
    }

    async fn core_for(listener: &TcpListener) -> Arc<Core> {
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let core = Core::new(ApiConfig {
            api_base: base.parse().unwrap(),
            submission_base: "http://127.0.0.1:9/".parse().unwrap(),
            web_origin: "http://127.0.0.1".to_owned(),
        })
        .unwrap();
        core.auth()
            .adopt(ACCOUNT, "token", Duration::from_secs(900), None);
        core
    }

    async fn next_frame(events: &mut tokio::sync::broadcast::Receiver<CoreEvent>) -> CoreEvent {
        tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("an event in time")
            .expect("open channel")
    }

    #[tokio::test]
    async fn a_dropped_stream_reconnects_with_a_fresh_ticket_from_the_last_event() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let core = core_for(&listener).await;
        let mut seen = core.subscribe();
        let server = tokio::spawn(serve(
            listener,
            vec![
                (
                    "POST /v1/realtime/ticket",
                    json("200 OK", r#"{"ticket":"t1","expiresAt":"x"}"#),
                ),
                (
                    "GET /v1/realtime/stream?ticket=t1 ",
                    events("id: 7\ndata: {\"kind\":\"message.created\",\"id\":\"m1\"}\n\n"),
                ),
                (
                    "POST /v1/realtime/ticket",
                    json("200 OK", r#"{"ticket":"t2","expiresAt":"x"}"#),
                ),
                ("GET /v1/realtime/stream?ticket=t2&since=7 ", events("")),
            ],
        ));
        open(&core, ACCOUNT).unwrap();

        let mut steps = Vec::new();
        while steps.len() < 6 {
            match next_frame(&mut seen).await {
                CoreEvent::RealtimeState { open, .. } => {
                    steps.push(if open { "open" } else { "closed" });
                }
                CoreEvent::Hint(hint) => {
                    assert_eq!(hint.account_id, ACCOUNT);
                    assert!(hint.kind.is(Entity::Message, Action::Created));
                    assert_eq!(hint.id.as_deref(), Some("m1"));
                    steps.push("hint");
                }
                CoreEvent::RealtimeResync { full, .. } => {
                    assert!(!full, "a short drop is not a full resync");
                    steps.push("resync");
                }
                _ => {}
            }
        }
        assert_eq!(
            steps,
            ["open", "hint", "closed", "open", "resync", "closed"]
        );
        core.realtime().close(ACCOUNT);
        let seen = server.await.unwrap();
        assert_eq!(seen.lines.len(), 4);
    }

    #[tokio::test]
    async fn a_settings_hint_reloads_settings_that_were_loaded() {
        let stub = crate::stub::Stub::start(|s, _| match s.path.as_str() {
            "/v1/account/settings" => crate::stub::json(
                "200 OK",
                r#"{"sections":{"composing":{"replyDefault":"all"}}}"#,
            ),
            _ => crate::stub::json("200 OK", "{}"),
        })
        .await;
        let core = Arc::new(Core::with_net(
            thelemail_api::Net::new(stub.config()).unwrap(),
        ));
        core.auth()
            .adopt(ACCOUNT, "t", Duration::from_secs(900), None);
        let mut seen = core.subscribe();
        let hint = |kind: &str| Hint::parse(ACCOUNT, &format!(r#"{{"kind":"{kind}"}}"#)).unwrap();
        apply(&core, hint("settings.updated"));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            stub.hits("/v1/account/settings"),
            0,
            "nothing asked for them yet"
        );
        crate::account::load_settings(&core, ACCOUNT).await;
        apply(&core, hint("settings.updated"));
        loop {
            if let CoreEvent::SettingsChanged { .. } = next_frame(&mut seen).await
                && stub.hits("/v1/account/settings") == 2
            {
                break;
            }
        }
        let signature = hint("signature.updated");
        apply(&core, signature.clone());
        loop {
            if let CoreEvent::Hint(h) = next_frame(&mut seen).await
                && h == signature
            {
                break;
            }
        }
    }

    #[tokio::test]
    async fn a_lost_session_stops_minting_until_the_account_signs_in_again() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let core = core_for(&listener).await;
        let mut seen = core.subscribe();
        let server = tokio::spawn(serve(
            listener,
            vec![
                ("POST /v1/realtime/ticket", json("401 Unauthorized", "{}")),
                ("POST /v1/auth/refresh", json("401 Unauthorized", "{}")),
            ],
        ));
        open(&core, ACCOUNT).unwrap();
        loop {
            if let CoreEvent::SessionLost { account_id } = next_frame(&mut seen).await {
                assert_eq!(account_id, ACCOUNT);
                break;
            }
        }
        core.realtime().close(ACCOUNT);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn an_endless_frame_is_cut_off_instead_of_filling_memory() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let core = core_for(&listener).await;
        let mut seen = core.subscribe();
        let flood = format!("data: {}", "x".repeat(MAX_FRAME_BYTES + 1024));
        let server = tokio::spawn(serve(
            listener,
            vec![
                (
                    "POST /v1/realtime/ticket",
                    json("200 OK", r#"{"ticket":"t1","expiresAt":"x"}"#),
                ),
                ("GET /v1/realtime/stream?ticket=t1 ", events(&flood)),
            ],
        ));
        open(&core, ACCOUNT).unwrap();
        let mut states = Vec::new();
        while states.len() < 2 {
            if let CoreEvent::RealtimeState { open, .. } = next_frame(&mut seen).await {
                states.push(open);
            }
        }
        assert_eq!(states, [true, false]);
        core.realtime().close(ACCOUNT);
        server.await.unwrap();
    }
}
