use std::collections::HashMap;

use thelemail_api::{ApiConfig, MAX_UPLOAD_BYTES, Net, TransportError, UploadBegin, UploadTarget};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use url::Url;

struct Received {
    head: String,
    body: Vec<u8>,
}

async fn server() -> (String, oneshot::Receiver<Option<Received>>) {
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
            if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                let _ = tx.send(None);
                return;
            }
            let end = line == "\r\n";
            head.push_str(&line);
            if end {
                break;
            }
        }
        let length = head
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
            })
            .unwrap_or(0);
        let mut body = vec![0u8; length];
        if reader.read_exact(&mut body).await.is_err() {
            let _ = tx.send(None);
            return;
        }
        let reply = b"{\"messageId\":\"m-1\"}";
        let _ = write
            .write_all(
                format!(
                    "HTTP/1.1 202 Accepted\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    reply.len()
                )
                .as_bytes(),
            )
            .await;
        let _ = write.write_all(reply).await;
        let _ = tx.send(Some(Received { head, body }));
    });
    (format!("http://{addr}"), rx)
}

async fn nothing_delivered(rx: oneshot::Receiver<Option<Received>>) -> bool {
    match tokio::time::timeout(std::time::Duration::from_secs(3), rx).await {
        Err(_) => true,
        Ok(Err(_)) => true,
        Ok(Ok(got)) => got.is_none(),
    }
}

fn net(submission: &str) -> Net {
    Net::new(ApiConfig {
        api_base: Url::parse("http://127.0.0.1:1").expect("url"),
        submission_base: Url::parse(submission).expect("url"),
        blob_origin: Url::parse("http://127.0.0.1:2").expect("url"),
        web_origin: "http://localhost:5175".to_owned(),
    })
    .expect("net")
}

fn begin(url: &str, size: u64) -> UploadBegin {
    UploadBegin {
        url: url.to_owned(),
        target: UploadTarget::Submission,
        size,
        headers: HashMap::from([
            ("Content-Type".to_owned(), "message/rfc822".to_owned()),
            ("Authorization".to_owned(), "Bearer t".to_owned()),
            ("Cookie".to_owned(), "stolen=1".to_owned()),
        ]),
    }
}

#[tokio::test]
async fn streams_chunks_in_order_with_the_declared_length() {
    let (base, rx) = server().await;
    let net = net(&base);
    let payload: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    let id = net
        .upload_begin(begin(
            &format!("{base}/v1/submission/intents/x/message"),
            payload.len() as u64,
        ))
        .expect("begin");
    for chunk in payload.chunks(1 << 20) {
        net.upload_chunk(&id, chunk.to_vec()).await.expect("chunk");
    }
    let resp = net.upload_finish(&id).await.expect("finish");
    assert_eq!(resp.status, 202);
    assert_eq!(resp.body.as_deref(), Some(&b"{\"messageId\":\"m-1\"}"[..]));

    let got = rx.await.expect("server").expect("request");
    assert_eq!(got.body, payload);
    let head = got.head.to_ascii_lowercase();
    assert!(head.starts_with("put /v1/submission/intents/x/message http/1.1"));
    assert!(head.contains(&format!("content-length: {}", payload.len())));
    assert!(head.contains("content-type: message/rfc822"));
    assert!(head.contains("authorization: bearer t"));
    assert!(head.contains("origin: http://localhost:5175"));
    assert!(!head.contains("cookie"), "a forbidden header was forwarded");
}

#[tokio::test]
async fn refuses_hosts_outside_the_submission_origin() {
    let net = net("http://127.0.0.1:9");
    let err = net
        .upload_begin(begin("http://evil.test/upload", 10))
        .expect_err("begin");
    assert!(matches!(err, TransportError::HostNotAllowed));
    let mut blob = begin("http://127.0.0.1:9/upload", 10);
    blob.target = UploadTarget::Blob;
    assert!(matches!(
        net.upload_begin(blob).expect_err("blob target"),
        TransportError::HostNotAllowed
    ));
}

#[tokio::test]
async fn refuses_a_declared_size_over_the_cap() {
    let net = net("http://127.0.0.1:9");
    let err = net
        .upload_begin(begin("http://127.0.0.1:9/upload", MAX_UPLOAD_BYTES + 1))
        .expect_err("begin");
    assert!(matches!(err, TransportError::ResponseTooLarge));
}

#[tokio::test]
async fn refuses_more_bytes_than_declared_and_drops_the_session() {
    let (base, rx) = server().await;
    let net = net(&base);
    let id = net
        .upload_begin(begin(&format!("{base}/u"), 4))
        .expect("begin");
    let err = net
        .upload_chunk(&id, vec![1, 2, 3, 4, 5])
        .await
        .expect_err("overflow");
    assert!(matches!(err, TransportError::ResponseTooLarge));
    assert!(net.upload_finish(&id).await.is_err());
    assert!(
        nothing_delivered(rx).await,
        "an overflowing body reached the server"
    );
}

#[tokio::test]
async fn refuses_to_finish_a_short_body() {
    let (base, rx) = server().await;
    let net = net(&base);
    let id = net
        .upload_begin(begin(&format!("{base}/u"), 10))
        .expect("begin");
    net.upload_chunk(&id, vec![1, 2, 3]).await.expect("chunk");
    assert!(matches!(
        net.upload_finish(&id).await,
        Err(TransportError::InvalidRequest)
    ));
    assert!(nothing_delivered(rx).await, "a short body was delivered");
}

#[tokio::test]
async fn abort_cancels_the_request() {
    let (base, rx) = server().await;
    let net = net(&base);
    let id = net
        .upload_begin(begin(&format!("{base}/u"), 1_000_000))
        .expect("begin");
    net.upload_chunk(&id, vec![7; 1000]).await.expect("chunk");
    net.upload_abort(&id);
    assert!(net.upload_chunk(&id, vec![7; 10]).await.is_err());
    assert!(nothing_delivered(rx).await, "an aborted upload completed");
}
