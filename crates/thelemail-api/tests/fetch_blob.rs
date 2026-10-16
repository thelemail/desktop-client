use std::collections::HashMap;

use thelemail_api::{ApiConfig, ApiRequest, Net, TransportError};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use url::Url;

async fn server(
    status: &'static str,
    body: Vec<u8>,
    declare: bool,
) -> (String, oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("accept");
        let (read, mut write) = socket.into_split();
        let mut reader = BufReader::new(read);
        let mut head = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await.unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            head.push_str(&line);
        }
        let length = if declare {
            format!("Content-Length: {}\r\n", body.len())
        } else {
            String::new()
        };
        let _ = write
            .write_all(
                format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nRetry-After: 5\r\n{length}Connection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await;
        let _ = write.write_all(&body).await;
        let _ = write.shutdown().await;
        let _ = tx.send(head);
    });
    (format!("http://{addr}"), rx)
}

fn net(api: &str) -> Net {
    Net::new(ApiConfig {
        api_base: Url::parse(api).expect("url"),
        submission_base: Url::parse("http://127.0.0.1:9").expect("url"),
        web_origin: "http://localhost:5175".to_owned(),
    })
    .expect("net")
}

fn get(url: String) -> ApiRequest {
    ApiRequest {
        url,
        method: "GET".to_owned(),
        headers: HashMap::from([
            ("Authorization".to_owned(), "Bearer t".to_owned()),
            ("X-Account-Id".to_owned(), "acc-1".to_owned()),
            ("Cookie".to_owned(), "stolen=1".to_owned()),
        ]),
        body: None,
    }
}

#[tokio::test]
async fn reads_a_blob_with_auth_and_origin() {
    let payload: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let (base, rx) = server("200 OK", payload.clone(), true).await;
    let resp = net(&base)
        .fetch_blob(get(format!("{base}/v1/messages/m-1/body")))
        .await
        .expect("fetch");
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body.as_deref(), Some(&payload[..]));
    let head = rx.await.expect("server").to_ascii_lowercase();
    assert!(head.starts_with("get /v1/messages/m-1/body http/1.1"));
    assert!(head.contains("authorization: bearer t"));
    assert!(head.contains("x-account-id: acc-1"));
    assert!(head.contains("origin: http://localhost:5175"));
    assert!(!head.contains("cookie"), "a forbidden header was forwarded");
}

#[tokio::test]
async fn hands_error_statuses_back_with_their_headers() {
    for status in [
        "401 Unauthorized",
        "404 Not Found",
        "503 Service Unavailable",
    ] {
        let (base, _rx) = server(status, b"{}".to_vec(), true).await;
        let resp = net(&base)
            .fetch_blob(get(format!("{base}/v1/attachments/a-1")))
            .await
            .expect("fetch");
        assert_eq!(resp.status.to_string(), status[..3]);
        assert_eq!(
            resp.headers.get("retry-after").map(String::as_str),
            Some("5")
        );
    }
}

#[tokio::test]
async fn refuses_hosts_outside_the_api() {
    let net = net("http://127.0.0.1:9");
    for url in [
        "https://fsn1.your-objectstorage.com/thelemail/bodies/x",
        "http://evil.test/v1/messages/m/body",
    ] {
        assert!(matches!(
            net.fetch_blob(get(url.to_owned())).await.expect_err("host"),
            TransportError::HostNotAllowed
        ));
    }
}

async fn raw_server(head: &'static [u8], stream_bytes: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut buf = [0u8; 4096];
        let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
        let _ = socket.write_all(head).await;
        let chunk = vec![7u8; 1 << 20];
        let mut sent = 0;
        while sent < stream_bytes {
            if socket.write_all(&chunk).await.is_err() {
                return;
            }
            sent += chunk.len();
        }
        let _ = socket.shutdown().await;
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn refuses_a_declared_length_over_the_cap() {
    let api = raw_server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 999999999999\r\nConnection: close\r\n\r\n",
        0,
    )
    .await;
    assert!(matches!(
        net(&api)
            .fetch_blob(get(format!("{api}/v1/attachments/a-1")))
            .await
            .expect_err("cap"),
        TransportError::ResponseTooLarge
    ));
}

#[tokio::test]
async fn stops_reading_an_undeclared_body_at_the_cap() {
    let api = raw_server(
        b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
        130 * 1024 * 1024,
    )
    .await;
    assert!(matches!(
        net(&api)
            .fetch_blob(get(format!("{api}/v1/drafts/d-1/attachments-bundle")))
            .await
            .expect_err("cap"),
        TransportError::ResponseTooLarge
    ));
}
