use std::sync::{Arc, Mutex};

use thelemail_api::ApiConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub const ACCOUNT: &str = "0c0c0c0c-0c0c-4c0c-8c0c-0c0c0c0c0c0c";

#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

pub type Route = dyn Fn(&Seen, usize) -> String + Send + Sync;

pub struct Stub {
    pub base: String,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

impl Stub {
    pub async fn start(route: impl Fn(&Seen, usize) -> String + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let base = format!("http://{}/", listener.local_addr().expect("addr"));
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let route: Arc<Route> = Arc::new(route);
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let route = route.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let Some(request) = read_request(&mut socket).await else {
                        return;
                    };
                    let hits = {
                        let mut log = log.lock().expect("stub log");
                        log.push(request.clone());
                        log.iter().filter(|s| s.path == request.path).count()
                    };
                    let reply = route(&request, hits);
                    let _ = socket.write_all(reply.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        Self { base, seen }
    }

    pub fn config(&self) -> ApiConfig {
        ApiConfig {
            api_base: self.base.parse().expect("base"),
            submission_base: self.base.parse().expect("base"),
            web_origin: "http://127.0.0.1".to_owned(),
        }
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().expect("stub log").clone()
    }

    pub fn hits(&self, path: &str) -> usize {
        self.requests().iter().filter(|s| s.path == path).count()
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<Seen> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let length = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some(Seen {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

pub fn json(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

pub fn with_headers(status: &str, extra: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\n{extra}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

pub fn no_content() -> String {
    "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
}

pub fn refreshed(token: &str) -> String {
    json(
        "200 OK",
        &format!(
            r#"{{"accessToken":"{token}","tokenType":"Bearer","expiresInSeconds":900,"accountId":"{ACCOUNT}","csrfToken":"csrf-{token}"}}"#
        ),
    )
}

pub fn error(status: &str, code: &str) -> String {
    json(
        status,
        &format!(r#"{{"error":{{"code":"{code}","message":"{code}"}}}}"#),
    )
}
