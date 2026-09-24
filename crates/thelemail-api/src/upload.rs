use std::collections::HashMap;
use std::io;
use std::sync::Mutex;
use std::time::Duration;

use bytes::Bytes;
use futures_util::stream;
use reqwest::header::{CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue, ORIGIN};
use reqwest::{Body, Client};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use url::Url;

use crate::config::ApiConfig;
use crate::transport::{
    ApiResponse, TransportError, is_forbidden_request_header, is_hidden_response_header,
};

pub const MAX_UPLOAD_BYTES: u64 = 96 * 1024 * 1024;
const MAX_CHUNK_BYTES: usize = 4 * 1024 * 1024;
const CHANNEL_DEPTH: usize = 4;
const MAX_OPEN_UPLOADS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UploadTarget {
    Submission,
    Blob,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadBegin {
    pub url: String,
    pub target: UploadTarget,
    pub size: u64,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

struct Session {
    tx: Option<mpsc::Sender<Result<Bytes, io::Error>>>,
    task: JoinHandle<Result<ApiResponse, TransportError>>,
    expected: u64,
    sent: u64,
}

pub struct Uploads {
    client: Client,
    sessions: Mutex<HashMap<String, Session>>,
}

impl Uploads {
    pub fn new(user_agent: &str) -> Result<Self, TransportError> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(120))
            .user_agent(user_agent)
            .build()
            .map_err(|_| TransportError::Network)?;
        Ok(Self {
            client,
            sessions: Mutex::new(HashMap::new()),
        })
    }

    pub fn begin(&self, config: &ApiConfig, req: UploadBegin) -> Result<String, TransportError> {
        let url = Url::parse(&req.url).map_err(|_| TransportError::InvalidRequest)?;
        let permitted = match req.target {
            UploadTarget::Submission => config.allows_submission_host(&url),
            UploadTarget::Blob => config.allows_blob_host(&url),
        };
        if !permitted {
            return Err(TransportError::HostNotAllowed);
        }
        if req.size > MAX_UPLOAD_BYTES {
            return Err(TransportError::ResponseTooLarge);
        }

        let mut headers = HeaderMap::new();
        for (name, value) in &req.headers {
            if is_forbidden_request_header(name) || name.eq_ignore_ascii_case("content-length") {
                continue;
            }
            let n = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| TransportError::InvalidRequest)?;
            let v = HeaderValue::from_str(value).map_err(|_| TransportError::InvalidRequest)?;
            headers.insert(n, v);
        }
        if req.target == UploadTarget::Submission {
            headers.insert(
                ORIGIN,
                HeaderValue::from_str(&config.web_origin)
                    .map_err(|_| TransportError::InvalidRequest)?,
            );
        }
        headers.insert(CONTENT_LENGTH, HeaderValue::from(req.size));

        let mut sessions = self.sessions.lock().expect("upload sessions");
        if sessions.len() >= MAX_OPEN_UPLOADS {
            return Err(TransportError::InvalidRequest);
        }

        let (tx, rx) = mpsc::channel::<Result<Bytes, io::Error>>(CHANNEL_DEPTH);
        let body = Body::wrap_stream(stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        }));
        let request = self.client.put(url).headers(headers).body(body);
        let task = tokio::spawn(async move {
            let resp = request.send().await.map_err(|_| TransportError::Network)?;
            let status = resp.status().as_u16();
            let mut out_headers = HashMap::new();
            for (name, value) in resp.headers() {
                if is_hidden_response_header(name.as_str()) {
                    continue;
                }
                if let Ok(v) = value.to_str() {
                    out_headers.insert(name.as_str().to_owned(), v.to_owned());
                }
            }
            let bytes = resp.bytes().await.map_err(|_| TransportError::Network)?;
            Ok(ApiResponse {
                status,
                headers: out_headers,
                body: if bytes.is_empty() {
                    None
                } else {
                    Some(bytes.to_vec())
                },
            })
        });

        let id = uuid::Uuid::new_v4().to_string();
        sessions.insert(
            id.clone(),
            Session {
                tx: Some(tx),
                task,
                expected: req.size,
                sent: 0,
            },
        );
        Ok(id)
    }

    pub async fn push(&self, id: &str, chunk: Vec<u8>) -> Result<(), TransportError> {
        if chunk.len() > MAX_CHUNK_BYTES {
            self.abort(id);
            return Err(TransportError::InvalidRequest);
        }
        let tx = {
            let mut sessions = self.sessions.lock().expect("upload sessions");
            let session = sessions.get_mut(id).ok_or(TransportError::InvalidRequest)?;
            let next = session.sent + chunk.len() as u64;
            if next > session.expected {
                drop(sessions);
                self.abort(id);
                return Err(TransportError::ResponseTooLarge);
            }
            session.sent = next;
            session.tx.clone().ok_or(TransportError::InvalidRequest)?
        };
        if tx.send(Ok(Bytes::from(chunk))).await.is_err() {
            self.abort(id);
            return Err(TransportError::Network);
        }
        Ok(())
    }

    pub async fn finish(&self, id: &str) -> Result<ApiResponse, TransportError> {
        let session = {
            let mut sessions = self.sessions.lock().expect("upload sessions");
            sessions.remove(id).ok_or(TransportError::InvalidRequest)?
        };
        let Session {
            tx,
            task,
            expected,
            sent,
        } = session;
        if sent != expected {
            task.abort();
            return Err(TransportError::InvalidRequest);
        }
        drop(tx);
        task.await.map_err(|_| TransportError::Network)?
    }

    pub fn abort(&self, id: &str) {
        let session = self.sessions.lock().expect("upload sessions").remove(id);
        if let Some(session) = session {
            session.task.abort();
        }
    }
}
