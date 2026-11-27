use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use thelemail_api::ApiConfig;
use thelemail_core::avatars::{EDGE, Lookup};
use thelemail_core::{Core, CoreEvent};
use url::Url;

const ACCOUNT: &str = "0c0c0c0c-0c0c-4c0c-8c0c-0c0c0c0c0c0c";
const LOGO: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4"><rect width="4" height="4"/></svg>"#;
const HOSTILE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="/etc/hosts"/></svg>"#;

type Seen = Arc<Mutex<Vec<(String, String)>>>;

fn photo() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(300, 200, image::Rgba([0, 92, 59, 255]));
    let mut out = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("png");
    out
}

fn respond(path: &str, body: &str) -> (u16, &'static str, Vec<u8>) {
    let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
    match path {
        "/v1/accounts/avatars" => {
            let wanted: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
            let avatars: Vec<_> = wanted["addresses"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|a| a.as_str() == Some("ada@acme.test"))
                .map(|a| serde_json::json!({ "address": a, "avatarPath": "/v1/avatars/a/1" }))
                .collect();
            (
                200,
                "application/json",
                serde_json::json!({ "avatars": avatars })
                    .to_string()
                    .into_bytes(),
            )
        }
        "/v1/avatars/a/1" => (200, "application/octet-stream", photo()),
        "/v1/bimi/logo" if body.contains("acme.test") => (
            200,
            "application/json",
            serde_json::json!({ "svg": b64(LOGO) })
                .to_string()
                .into_bytes(),
        ),
        "/v1/bimi/logo" if body.contains("evil.test") => (
            200,
            "application/json",
            serde_json::json!({ "svg": b64(HOSTILE) })
                .to_string()
                .into_bytes(),
        ),
        _ => (404, "application/json", b"{}".to_vec()),
    }
}

fn serve(seen: Seen, hits: Arc<AtomicUsize>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            hits.fetch_add(1, Ordering::SeqCst);
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut request_line = String::new();
            let _ = reader.read_line(&mut request_line);
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; length];
            let _ = reader.read_exact(&mut body);
            let body = String::from_utf8_lossy(&body).into_owned();
            let path = request_line.split(' ').nth(1).unwrap_or("/").to_owned();
            seen.lock().unwrap().push((path.clone(), body.clone()));
            let (status, kind, payload) = respond(&path, &body);
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            let _ = stream.write_all(&payload);
        }
    });
    format!("http://{addr}/")
}

fn core(api: &str) -> Arc<Core> {
    Core::new(ApiConfig {
        api_base: Url::parse(api).expect("url"),
        submission_base: Url::parse("http://127.0.0.1:9").expect("url"),
        web_origin: "http://localhost:5175".to_owned(),
    })
    .expect("core")
}

fn mirror_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().expect("db");
    thelemail_store::migrations::migrate(&conn).expect("migrate");
    conn
}

async fn settle(core: &Core, ready: impl Fn(&Core) -> bool) {
    let mut events = core.subscribe();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !ready(core) {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(!left.is_zero(), "avatars did not settle in time");
        match tokio::time::timeout(left, events.recv()).await {
            Ok(Ok(CoreEvent::AvatarsChanged { .. })) | Err(_) => {}
            Ok(_) => {}
        }
    }
}

fn ready(lookup: Lookup) -> Option<Option<(u32, u32)>> {
    match lookup {
        Lookup::Ready(image) => Some(image.map(|i| {
            let decoded = image::load_from_memory_with_format(&i.png, image::ImageFormat::Png)
                .expect("a stored avatar is always a PNG");
            (decoded.width(), decoded.height())
        })),
        Lookup::Pending => None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn photos_and_logos_are_fetched_checked_cached_and_served_offline() {
    let seen: Seen = Arc::default();
    let hits = Arc::new(AtomicUsize::new(0));
    let api = serve(seen.clone(), hits.clone());
    let online = core(&api);
    online.start();
    online
        .mirror()
        .adopt_connection(ACCOUNT, mirror_db())
        .expect("adopt");
    online.mirror().set_token(ACCOUNT, "token-1");

    let avatars = online.avatars();
    assert!(matches!(
        avatars.person(ACCOUNT, "Ada@Acme.test"),
        Lookup::Pending
    ));
    avatars.person(ACCOUNT, "nobody@acme.test");
    avatars.bimi(ACCOUNT, "acme.test");
    avatars.bimi(ACCOUNT, "evil.test");

    settle(&online, |c| {
        let a = c.avatars();
        [
            ready(a.person(ACCOUNT, "ada@acme.test")),
            ready(a.person(ACCOUNT, "nobody@acme.test")),
            ready(a.bimi(ACCOUNT, "acme.test")),
            ready(a.bimi(ACCOUNT, "evil.test")),
        ]
        .iter()
        .all(Option::is_some)
    })
    .await;

    let a = online.avatars();
    assert_eq!(
        ready(a.person(ACCOUNT, "ada@acme.test")),
        Some(Some((EDGE, EDGE)))
    );
    assert_eq!(ready(a.person(ACCOUNT, "nobody@acme.test")), Some(None));
    assert_eq!(
        ready(a.bimi(ACCOUNT, "acme.test")),
        Some(Some((EDGE, EDGE)))
    );
    assert_eq!(ready(a.bimi(ACCOUNT, "evil.test")), Some(None));

    let requests = seen.lock().unwrap().clone();
    let resolves: Vec<_> = requests
        .iter()
        .filter(|(p, _)| p == "/v1/accounts/avatars")
        .collect();
    assert_eq!(
        resolves.len(),
        1,
        "lookups in one tick share one resolve call"
    );
    assert!(resolves[0].1.contains("ada@acme.test") && resolves[0].1.contains("nobody@acme.test"));
    assert_eq!(
        requests
            .iter()
            .filter(|(p, _)| p == "/v1/avatars/a/1")
            .count(),
        1
    );

    let conn = online.mirror().take_connection(ACCOUNT).expect("conn");
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM avatar_cache", [], |r| r.get(0))
        .expect("count");
    assert_eq!(
        rows, 4,
        "answers, including the negative ones, are kept in the mirror"
    );

    let before = hits.load(Ordering::SeqCst);
    let offline = core("http://127.0.0.1:9/");
    offline.start();
    offline
        .mirror()
        .adopt_connection(ACCOUNT, conn)
        .expect("adopt");
    let o = offline.avatars();
    assert!(matches!(
        o.person(ACCOUNT, "ada@acme.test"),
        Lookup::Pending
    ));
    o.bimi(ACCOUNT, "acme.test");
    settle(&offline, |c| {
        let a = c.avatars();
        ready(a.person(ACCOUNT, "ada@acme.test")).is_some()
            && ready(a.bimi(ACCOUNT, "acme.test")).is_some()
    })
    .await;
    assert_eq!(
        ready(o.person(ACCOUNT, "ada@acme.test")),
        Some(Some((EDGE, EDGE)))
    );
    assert_eq!(
        ready(o.bimi(ACCOUNT, "acme.test")),
        Some(Some((EDGE, EDGE)))
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        before,
        "offline lookups never touch the network"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn locking_an_account_drops_its_images_from_memory() {
    let seen: Seen = Arc::default();
    let api = serve(seen, Arc::new(AtomicUsize::new(0)));
    let core = core(&api);
    core.start();
    core.mirror()
        .adopt_connection(ACCOUNT, mirror_db())
        .expect("adopt");
    core.mirror().set_token(ACCOUNT, "token-1");
    core.avatars().person(ACCOUNT, "ada@acme.test");
    settle(&core, |c| {
        ready(c.avatars().person(ACCOUNT, "ada@acme.test")).is_some()
    })
    .await;
    core.mirror().take_connection(ACCOUNT);
    core.keystore().clear(ACCOUNT);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        if matches!(
            core.avatars().person(ACCOUNT, "ada@acme.test"),
            Lookup::Pending
        ) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the image was still in memory after the account was cleared");
}
