use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{0} is not a valid absolute URL")]
    InvalidUrl(&'static str),
}

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub api_base: Url,
    pub submission_base: Url,
    pub blob_origin: Url,
    pub web_origin: String,
}

impl ApiConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let api_base = parse("THELEMAIL_DESKTOP_API_BASE_URL", default_api_base())?;
        let submission_base = parse(
            "THELEMAIL_DESKTOP_SUBMISSION_BASE_URL",
            default_submission_base(),
        )?;
        let blob_origin = parse("THELEMAIL_DESKTOP_BLOB_ORIGIN", default_blob_origin())?;
        let web_origin = std::env::var("THELEMAIL_DESKTOP_WEB_ORIGIN")
            .unwrap_or_else(|_| default_web_origin().to_owned());

        Ok(Self {
            api_base,
            submission_base,
            blob_origin,
            web_origin,
        })
    }

    pub fn allows_api_host(&self, url: &Url) -> bool {
        same_origin(url, &self.api_base)
    }

    pub fn allows_submission_host(&self, url: &Url) -> bool {
        same_origin(url, &self.submission_base)
    }

    pub fn allows_blob_host(&self, url: &Url) -> bool {
        same_origin(url, &self.blob_origin)
    }
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

fn parse(var: &'static str, fallback: &str) -> Result<Url, ConfigError> {
    let raw = std::env::var(var).unwrap_or_else(|_| fallback.to_owned());
    Url::parse(&raw).map_err(|_| ConfigError::InvalidUrl(var))
}

const fn default_api_base() -> &'static str {
    match option_env!("THELEMAIL_API_BASE_URL") {
        Some(v) => v,
        None => "https://api.thelemail.com",
    }
}

const fn default_submission_base() -> &'static str {
    match option_env!("THELEMAIL_SUBMISSION_BASE_URL") {
        Some(v) => v,
        None => "https://submission.thelemail.com",
    }
}

const fn default_blob_origin() -> &'static str {
    match option_env!("THELEMAIL_BLOB_ORIGIN") {
        Some(v) => v,
        None => "https://fsn1.your-objectstorage.com",
    }
}

const fn default_web_origin() -> &'static str {
    match option_env!("THELEMAIL_WEB_ORIGIN") {
        Some(v) => v,
        None => "https://app.thelemail.com",
    }
}
