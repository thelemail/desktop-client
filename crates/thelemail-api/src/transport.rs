use std::collections::HashMap;
use std::sync::Arc;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ORIGIN};
use reqwest::{Client, Method};
use reqwest_cookie_store::CookieStoreMutex;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::ApiConfig;

const MAX_BLOB_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Audience {
    Api,
    Submission,
}

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("host not allowed")]
    HostNotAllowed,
    #[error("invalid request")]
    InvalidRequest,
    #[error("response too large")]
    ResponseTooLarge,
    #[error("network error")]
    Network,
}

impl Serialize for TransportError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiRequest {
    pub url: String,
    pub method: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

pub struct Net {
    api: Client,
    blob: Client,
    cookies: Arc<CookieStoreMutex>,
    config: ApiConfig,
}

impl Net {
    pub fn new(config: ApiConfig) -> Result<Self, TransportError> {
        let cookies = Arc::new(CookieStoreMutex::default());
        let api = Client::builder()
            .cookie_provider(Arc::clone(&cookies))
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(60))
            .user_agent(user_agent())
            .build()
            .map_err(|_| TransportError::Network)?;
        let blob = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(300))
            .user_agent(user_agent())
            .build()
            .map_err(|_| TransportError::Network)?;
        Ok(Self {
            api,
            blob,
            cookies,
            config,
        })
    }

    pub fn config(&self) -> &ApiConfig {
        &self.config
    }

    pub fn cookies(&self) -> &Arc<CookieStoreMutex> {
        &self.cookies
    }

    pub async fn request(&self, req: ApiRequest) -> Result<ApiResponse, TransportError> {
        self.send(req, Audience::Api).await
    }

    pub async fn submit(&self, req: ApiRequest) -> Result<ApiResponse, TransportError> {
        self.send(req, Audience::Submission).await
    }

    async fn send(
        &self,
        req: ApiRequest,
        audience: Audience,
    ) -> Result<ApiResponse, TransportError> {
        let url = Url::parse(&req.url).map_err(|_| TransportError::InvalidRequest)?;
        let permitted = match audience {
            Audience::Api => self.config.allows_api_host(&url),
            Audience::Submission => self.config.allows_submission_host(&url),
        };
        if !permitted {
            return Err(TransportError::HostNotAllowed);
        }
        let method = Method::from_bytes(req.method.as_bytes())
            .map_err(|_| TransportError::InvalidRequest)?;

        let mut headers = HeaderMap::new();
        for (name, value) in &req.headers {
            if is_forbidden_request_header(name) {
                continue;
            }
            let n = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| TransportError::InvalidRequest)?;
            let v = HeaderValue::from_str(value).map_err(|_| TransportError::InvalidRequest)?;
            headers.insert(n, v);
        }
        headers.insert(
            ORIGIN,
            HeaderValue::from_str(&self.config.web_origin)
                .map_err(|_| TransportError::InvalidRequest)?,
        );

        let mut builder = self.api.request(method, url).headers(headers);
        if let Some(body) = req.body {
            builder = builder.body(body);
        }

        let resp = builder.send().await.map_err(|_| TransportError::Network)?;
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
    }

    pub fn export_cookies(&self) -> Vec<(String, String)> {
        let store = self.cookies.lock().expect("cookie store");
        store
            .iter_any()
            .map(|c| (c.name().to_owned(), c.value().to_owned()))
            .collect()
    }

    pub fn import_cookie(&self, name: &str, value: &str) -> Result<(), TransportError> {
        let raw = format!("{name}={value}; Path=/v1/auth");
        let mut store = self.cookies.lock().expect("cookie store");
        store
            .parse(&raw, &self.config.api_base)
            .map_err(|_| TransportError::InvalidRequest)?;
        Ok(())
    }

    pub fn forget_cookie(&self, name: &str) {
        let mut store = self.cookies.lock().expect("cookie store");
        let hosts: Vec<(String, String, String)> = store
            .iter_any()
            .filter(|c| c.name() == name)
            .map(|c| {
                (
                    c.domain().unwrap_or_default().to_owned(),
                    c.path().unwrap_or("/").to_owned(),
                    c.name().to_owned(),
                )
            })
            .collect();
        for (domain, path, name) in hosts {
            store.remove(&domain, &path, &name);
        }
    }

    pub async fn stream_events(&self, raw_url: &str) -> Result<reqwest::Response, TransportError> {
        let url = Url::parse(raw_url).map_err(|_| TransportError::InvalidRequest)?;
        if !self.config.allows_api_host(&url) {
            return Err(TransportError::HostNotAllowed);
        }
        self.api
            .get(url)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .header(
                ORIGIN,
                HeaderValue::from_str(&self.config.web_origin)
                    .map_err(|_| TransportError::InvalidRequest)?,
            )
            .timeout(std::time::Duration::from_secs(60 * 60))
            .send()
            .await
            .map_err(|_| TransportError::Network)
    }

    pub async fn blob_put(
        &self,
        raw_url: &str,
        bytes: Vec<u8>,
        content_type: Option<String>,
    ) -> Result<u16, TransportError> {
        let url = Url::parse(raw_url).map_err(|_| TransportError::InvalidRequest)?;
        if !self.config.allows_blob_host(&url) {
            return Err(TransportError::HostNotAllowed);
        }
        if bytes.len() > MAX_BLOB_BYTES {
            return Err(TransportError::ResponseTooLarge);
        }

        let mut request = self.blob.put(url).body(bytes);
        if let Some(ct) = content_type {
            let value = HeaderValue::from_str(&ct).map_err(|_| TransportError::InvalidRequest)?;
            request = request.header(reqwest::header::CONTENT_TYPE, value);
        }
        let resp = request.send().await.map_err(|_| TransportError::Network)?;
        Ok(resp.status().as_u16())
    }

    pub async fn blob_get(&self, raw_url: &str) -> Result<Vec<u8>, TransportError> {
        let url = Url::parse(raw_url).map_err(|_| TransportError::InvalidRequest)?;
        if !self.config.allows_blob_host(&url) {
            return Err(TransportError::HostNotAllowed);
        }
        let resp = self
            .blob
            .get(url)
            .send()
            .await
            .map_err(|_| TransportError::Network)?;
        if !resp.status().is_success() {
            return Err(TransportError::Network);
        }
        if let Some(len) = resp.content_length()
            && len as usize > MAX_BLOB_BYTES
        {
            return Err(TransportError::ResponseTooLarge);
        }
        let bytes = resp.bytes().await.map_err(|_| TransportError::Network)?;
        if bytes.len() > MAX_BLOB_BYTES {
            return Err(TransportError::ResponseTooLarge);
        }
        Ok(bytes.to_vec())
    }
}

fn user_agent() -> String {
    format!("Thelemail/{} (macOS)", env!("CARGO_PKG_VERSION"))
}

fn is_forbidden_request_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(lower.as_str(), "cookie" | "origin" | "referer" | "host")
}

fn is_hidden_response_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "set-cookie" | "connection" | "keep-alive" | "transfer-encoding" | "upgrade"
    )
}
