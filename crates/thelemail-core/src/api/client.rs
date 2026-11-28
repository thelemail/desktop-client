use std::collections::HashMap;

use base64::Engine;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use thelemail_api::{ApiRequest, ApiResponse, Net};

use super::auth::Auth;
use super::error::{ApiError, ErrorCode};

pub const CLIENT_HEADER: &str = "X-Client";
pub const CLIENT: &str = "desktop";
const STEP_UP_GRANT: &str = "X-Step-Up-Grant";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Api,
    Submission,
    Blob,
}

#[derive(Debug, Clone)]
pub struct Call {
    method: Method,
    path: String,
    body: Option<String>,
    target: Target,
    auth: bool,
    retry_unauthorized: bool,
    csrf: bool,
    headers: Vec<(String, String)>,
}

impl Call {
    fn new(method: Method, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            body: None,
            target: Target::Api,
            auth: true,
            retry_unauthorized: true,
            csrf: false,
            headers: Vec::new(),
        }
    }

    pub fn get(path: impl Into<String>) -> Self {
        Self::new(Method::Get, path)
    }

    pub fn post(path: impl Into<String>) -> Self {
        Self::new(Method::Post, path)
    }

    pub fn put(path: impl Into<String>) -> Self {
        Self::new(Method::Put, path)
    }

    pub fn patch(path: impl Into<String>) -> Self {
        Self::new(Method::Patch, path)
    }

    pub fn delete(path: impl Into<String>) -> Self {
        Self::new(Method::Delete, path)
    }

    pub fn blob(url: impl Into<String>) -> Self {
        let mut call = Self::new(Method::Get, url);
        call.target = Target::Blob;
        call
    }

    pub fn json<T: Serialize + ?Sized>(mut self, body: &T) -> Self {
        self.body = serde_json::to_string(body).ok();
        self
    }

    pub fn submission(mut self) -> Self {
        self.target = Target::Submission;
        self
    }

    pub fn anonymous(mut self) -> Self {
        self.auth = false;
        self.retry_unauthorized = false;
        self
    }

    pub fn credential_check(mut self) -> Self {
        self.retry_unauthorized = false;
        self
    }

    pub fn csrf(mut self) -> Self {
        self.csrf = true;
        self
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    fn has_header(&self, name: &str) -> bool {
        self.headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Copy)]
pub struct Api<'a> {
    net: &'a Net,
    auth: &'a Auth,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GrantRequest<'a> {
    method: &'static str,
    path: &'a str,
    body_digest: String,
}

#[derive(serde::Deserialize)]
struct GrantResponse {
    grant: String,
}

impl<'a> Api<'a> {
    pub fn new(net: &'a Net, auth: &'a Auth) -> Self {
        Self { net, auth }
    }

    pub fn net(&self) -> &'a Net {
        self.net
    }

    pub fn auth(&self) -> &'a Auth {
        self.auth
    }

    pub async fn json<T: DeserializeOwned>(
        &self,
        account_id: &str,
        call: Call,
    ) -> Result<T, ApiError> {
        let resp = self.send(account_id, call).await?;
        serde_json::from_slice(&resp.body).map_err(|e| ApiError::Decode(e.to_string()))
    }

    pub async fn empty(&self, account_id: &str, call: Call) -> Result<(), ApiError> {
        self.send(account_id, call).await.map(drop)
    }

    pub async fn public<T: DeserializeOwned>(&self, call: Call) -> Result<T, ApiError> {
        let call = call.anonymous();
        let (resp, _) = self.attempt(None, &call).await?;
        if !(200..300).contains(&resp.status) {
            return Err(ApiError::from_status(
                resp.status,
                header(&resp, "retry-after"),
                resp.body.as_deref(),
            ));
        }
        serde_json::from_slice(&resp.body.unwrap_or_default())
            .map_err(|e| ApiError::Decode(e.to_string()))
    }

    pub async fn send(&self, account_id: &str, mut call: Call) -> Result<Response, ApiError> {
        crate::ids::account_id(account_id).map_err(|_| ApiError::Request)?;
        let mut renewed = false;
        loop {
            let (resp, generation) = self.attempt(Some(account_id), &call).await?;
            if (200..300).contains(&resp.status) {
                return Ok(Response {
                    status: resp.status,
                    headers: resp.headers,
                    body: resp.body.unwrap_or_default(),
                });
            }
            let err = ApiError::from_status(
                resp.status,
                header(&resp, "retry-after"),
                resp.body.as_deref(),
            );
            if resp.status == 401 && call.auth && call.retry_unauthorized && !renewed {
                renewed = true;
                if let Some(generation) = generation {
                    match self
                        .auth
                        .renew_after_unauthorized(self.net, account_id, generation)
                        .await
                    {
                        Ok(()) => continue,
                        Err(ApiError::SessionLost) => return Err(ApiError::SessionLost),
                        Err(_) => return Err(err),
                    }
                }
            }
            match err.code() {
                Some(ErrorCode::StepUpGrantRequired) if !call.has_header(STEP_UP_GRANT) => {
                    let grant = self.step_up_grant(account_id, &call).await?;
                    call = call.header(STEP_UP_GRANT, grant);
                    continue;
                }
                Some(ErrorCode::ReadOnly | ErrorCode::AccountSuspended) => {
                    self.auth.lifecycle_error(account_id);
                }
                _ => {}
            }
            return Err(err);
        }
    }

    async fn step_up_grant(&self, account_id: &str, call: &Call) -> Result<String, ApiError> {
        let digest = Sha256::digest(call.body.as_deref().unwrap_or_default().as_bytes());
        let request = GrantRequest {
            method: call.method.as_str(),
            path: call.path.split('?').next().unwrap_or_default(),
            body_digest: base64::engine::general_purpose::STANDARD.encode(digest),
        };
        let grant: GrantResponse = Box::pin(self.json(
            account_id,
            Call::post("/v1/auth/step-up/grants").json(&request),
        ))
        .await?;
        Ok(grant.grant)
    }

    fn url(&self, call: &Call) -> Result<String, ApiError> {
        let base = match call.target {
            Target::Api => &self.net.config().api_base,
            Target::Submission => &self.net.config().submission_base,
            Target::Blob => return Ok(call.path.clone()),
        };
        let path = call.path.as_str();
        if !path.starts_with('/') || path.starts_with("//") {
            return Err(ApiError::Request);
        }
        Ok(format!("{base}{}", &path[1..]))
    }

    async fn attempt(
        &self,
        account_id: Option<&str>,
        call: &Call,
    ) -> Result<(ApiResponse, Option<u64>), ApiError> {
        let token = match account_id {
            Some(id) if call.auth => Some(self.auth.fresh(self.net, id).await?),
            _ => None,
        };
        let mut headers: HashMap<String, String> = call.headers.iter().cloned().collect();
        headers.insert(CLIENT_HEADER.to_owned(), CLIENT.to_owned());
        headers.insert(
            "X-Device-Id".to_owned(),
            self.auth.device(account_id.unwrap_or_default()),
        );
        if let Some(id) = account_id {
            headers.insert("X-Account-Id".to_owned(), id.to_owned());
        }
        if let Some(token) = &token {
            headers.insert(
                "Authorization".to_owned(),
                format!("Bearer {}", token.access.as_str()),
            );
        }
        if call.body.is_some() {
            headers.insert("Content-Type".to_owned(), "application/json".to_owned());
        }
        if call.csrf
            && let Some(csrf) = account_id.and_then(|id| self.auth.csrf(id))
        {
            headers.insert("X-CSRF-Token".to_owned(), csrf.as_str().to_owned());
        }
        let req = ApiRequest {
            url: self.url(call)?,
            method: call.method.as_str().to_owned(),
            headers,
            body: call.body.clone(),
        };
        let resp = match call.target {
            Target::Api => self.net.request(req).await?,
            Target::Submission => self.net.submit(req).await?,
            Target::Blob => self.net.fetch_blob(req).await?,
        };
        if let Some(id) = account_id
            && call.target == Target::Api
            && call.path.starts_with("/v1/auth/")
        {
            crate::session::sync_persisted(self.net, id);
        }
        Ok((resp, token.map(|t| t.generation)))
    }
}

fn header<'r>(resp: &'r ApiResponse, name: &str) -> Option<&'r str> {
    resp.headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}
