use serde::Deserialize;
use thelemail_api::TransportError;

macro_rules! error_codes {
    ($($variant:ident => $wire:literal),+ $(,)?) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum ErrorCode {
            $($variant,)+
            Other(String),
        }

        impl ErrorCode {
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $wire,)+
                    Self::Other(code) => code,
                }
            }
        }

        impl From<String> for ErrorCode {
            fn from(code: String) -> Self {
                match code.as_str() {
                    $($wire => Self::$variant,)+
                    _ => Self::Other(code),
                }
            }
        }
    };
}

error_codes! {
    InvalidRequest => "invalid_request",
    InvalidMessage => "invalid_message",
    IntentNotFound => "intent_not_found",
    ScannerUnavailable => "scanner_unavailable",
    PayloadTooLarge => "payload_too_large",
    InvalidCredentials => "invalid_credentials",
    RateLimited => "rate_limited",
    ServerBusy => "server_busy",
    UnsupportedParameters => "unsupported_parameters",
    Forbidden => "forbidden",
    NotFound => "not_found",
    Conflict => "conflict",
    PaymentRequired => "payment_required",
    InternalError => "internal_error",
    ServiceUnavailable => "service_unavailable",
    AccountPendingDeletion => "account_pending_deletion",
    WorkspaceTransferRequired => "workspace_transfer_required",
    ReadOnly => "read_only",
    AccountSuspended => "account_suspended",
    UpgradeRequired => "upgrade_required",
    ContentRejected => "content_rejected",
    MigrationConflict => "migration_conflict",
    StaleRevision => "stale_revision",
    SharedAliasKeyStale => "shared_alias_key_stale",
    AccountKeyStale => "account_key_stale",
    KeyRotationUnavailable => "key_rotation_unavailable",
    KeyTransitionInvalid => "key_transition_invalid",
    InviteeNotFound => "invitee_not_found",
    InviteePlanConflict => "invitee_plan_conflict",
    InviteeStoreBilled => "invitee_store_billed",
    InviteeAlreadyInFamily => "invitee_already_in_family",
    InviteeHasCustomDomain => "invitee_has_custom_domain",
    InviteeDomainNotShared => "invitee_domain_not_shared",
    FamilyFull => "family_full",
    InviteNotAcceptable => "invite_not_acceptable",
    ConfirmationLinkExpired => "confirmation_link_expired",
    ConfirmationLinkInvalid => "confirmation_link_invalid",
    ForwardingSourceUnavailable => "forwarding_source_unavailable",
    SenderAddressSuspended => "sender_address_suspended",
    SendingNotVerified => "sending_not_verified",
    DomainPaused => "domain_paused",
    DomainCheckRunning => "domain_check_running",
    DomainStageVerified => "domain_stage_verified",
    DomainStageLocked => "domain_stage_locked",
    ProofRequired => "proof_required",
    ProofInvalid => "proof_invalid",
    ProofExpired => "proof_expired",
    ProofReused => "proof_reused",
    StepUpRequired => "step_up_required",
    ReauthenticationRequired => "reauthentication_required",
    UnsubscribeTagInvalid => "unsubscribe_tag_invalid",
    UnsubscribeRejected => "unsubscribe_rejected",
    UnsubscribeUnreachable => "unsubscribe_unreachable",
    AuthOriginUnavailable => "auth_origin_unavailable",
    HandoffAccountMismatch => "handoff_account_mismatch",
    StepUpGrantRequired => "step_up_grant_required",
    SourceBarrierPending => "source_barrier_pending",
    SourceSnapshotExpired => "source_snapshot_expired",
    SourceCursorExpired => "source_cursor_expired",
    SearchSyncDisabled => "search_sync_disabled",
    SearchGenerationChanged => "search_generation_changed",
    SearchProductEpochStale => "search_product_epoch_stale",
    SearchGenerationUsed => "search_generation_used",
    SearchGenerationExists => "search_generation_exists",
    SearchCapacityExceeded => "search_capacity_exceeded",
    SearchArtifactSize => "search_artifact_size",
    SearchArtifactMismatch => "search_artifact_mismatch",
    ClientNotAllowed => "client_not_allowed",
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(Self::from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    pub code: ErrorCode,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub retry_after_seconds: Option<u64>,
}

#[derive(Deserialize)]
struct Envelope {
    error: ErrorBody,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    #[error("network unavailable")]
    Network,
    #[error("request refused locally")]
    Request,
    #[error("session lost")]
    SessionLost,
    #[error("unauthorized")]
    Unauthorized(Option<ErrorBody>),
    #[error("forbidden")]
    Forbidden(Option<ErrorBody>),
    #[error("not found")]
    NotFound(Option<ErrorBody>),
    #[error("conflict")]
    Conflict(Option<ErrorBody>),
    #[error("invalid request ({status})")]
    Validation {
        status: u16,
        body: Option<ErrorBody>,
    },
    #[error("rate limited")]
    RateLimited {
        retry_after: Option<u64>,
        body: Option<ErrorBody>,
    },
    #[error("server error ({status})")]
    Server {
        status: u16,
        body: Option<ErrorBody>,
    },
    #[error("unexpected status {status}")]
    Status {
        status: u16,
        body: Option<ErrorBody>,
    },
    #[error("unreadable response: {0}")]
    Decode(String),
}

impl ApiError {
    pub fn from_status(status: u16, retry_after_header: Option<&str>, body: Option<&[u8]>) -> Self {
        let body = body
            .and_then(|bytes| serde_json::from_slice::<Envelope>(bytes).ok())
            .map(|e| e.error);
        match status {
            401 => Self::Unauthorized(body),
            403 => Self::Forbidden(body),
            404 => Self::NotFound(body),
            409 => Self::Conflict(body),
            400 | 422 => Self::Validation { status, body },
            429 => Self::RateLimited {
                retry_after: retry_after_header
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .filter(|s| *s > 0)
                    .or_else(|| {
                        body.as_ref()
                            .and_then(|b| b.retry_after_seconds)
                            .filter(|s| *s > 0)
                    }),
                body,
            },
            500..=599 => Self::Server { status, body },
            _ => Self::Status { status, body },
        }
    }

    pub fn status(&self) -> Option<u16> {
        Some(match self {
            Self::Network | Self::Request | Self::SessionLost | Self::Decode(_) => return None,
            Self::Unauthorized(_) => 401,
            Self::Forbidden(_) => 403,
            Self::NotFound(_) => 404,
            Self::Conflict(_) => 409,
            Self::RateLimited { .. } => 429,
            Self::Validation { status, .. }
            | Self::Server { status, .. }
            | Self::Status { status, .. } => *status,
        })
    }

    pub fn body(&self) -> Option<&ErrorBody> {
        match self {
            Self::Unauthorized(body)
            | Self::Forbidden(body)
            | Self::NotFound(body)
            | Self::Conflict(body)
            | Self::Validation { body, .. }
            | Self::RateLimited { body, .. }
            | Self::Server { body, .. }
            | Self::Status { body, .. } => body.as_ref(),
            Self::Network | Self::Request | Self::SessionLost | Self::Decode(_) => None,
        }
    }

    pub fn code(&self) -> Option<&ErrorCode> {
        self.body().map(|b| &b.code)
    }

    pub fn is_refusal(&self) -> bool {
        !matches!(
            self,
            Self::Network | Self::Server { .. } | Self::RateLimited { .. } | Self::Decode(_)
        )
    }
}

impl From<TransportError> for ApiError {
    fn from(err: TransportError) -> Self {
        match err {
            TransportError::Network => Self::Network,
            TransportError::HostNotAllowed
            | TransportError::InvalidRequest
            | TransportError::ResponseTooLarge => Self::Request,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(code: &str) -> Vec<u8> {
        format!(r#"{{"error":{{"code":"{code}","message":"m","retryAfterSeconds":7}}}}"#)
            .into_bytes()
    }

    type Check = fn(&ApiError) -> bool;

    #[test]
    fn each_status_lands_in_its_category() {
        let cases: [(u16, Check); 11] = [
            (400, |e| {
                matches!(e, ApiError::Validation { status: 400, .. })
            }),
            (401, |e| matches!(e, ApiError::Unauthorized(_))),
            (402, |e| matches!(e, ApiError::Status { status: 402, .. })),
            (403, |e| matches!(e, ApiError::Forbidden(_))),
            (404, |e| matches!(e, ApiError::NotFound(_))),
            (409, |e| matches!(e, ApiError::Conflict(_))),
            (410, |e| matches!(e, ApiError::Status { status: 410, .. })),
            (422, |e| {
                matches!(e, ApiError::Validation { status: 422, .. })
            }),
            (429, |e| matches!(e, ApiError::RateLimited { .. })),
            (500, |e| matches!(e, ApiError::Server { status: 500, .. })),
            (503, |e| matches!(e, ApiError::Server { status: 503, .. })),
        ];
        for (status, check) in cases {
            let err = ApiError::from_status(status, None, None);
            assert!(check(&err), "{status} mapped to {err:?}");
            assert_eq!(err.status(), Some(status));
        }
    }

    #[test]
    fn a_conflict_keeps_its_code() {
        let err = ApiError::from_status(409, None, Some(&envelope("stale_revision")));
        assert_eq!(err.code(), Some(&ErrorCode::StaleRevision));
        assert_eq!(err.body().map(|b| b.message.as_str()), Some("m"));
    }

    #[test]
    fn an_unknown_code_is_kept_verbatim() {
        let err = ApiError::from_status(403, None, Some(&envelope("brand_new_rule")));
        assert_eq!(err.code(), Some(&ErrorCode::Other("brand_new_rule".into())));
        assert_eq!(err.code().map(ErrorCode::as_str), Some("brand_new_rule"));
    }

    #[test]
    fn a_body_that_is_not_an_envelope_leaves_no_code() {
        for body in [&b"<html>bad gateway</html>"[..], b"{}", b"{\"error\":1}"] {
            let err = ApiError::from_status(502, None, Some(body));
            assert_eq!(err.body(), None);
            assert!(matches!(err, ApiError::Server { status: 502, .. }));
        }
    }

    #[test]
    fn retry_after_prefers_the_header_then_the_body() {
        let body = envelope("rate_limited");
        let header = ApiError::from_status(429, Some("12"), Some(&body));
        assert!(matches!(
            header,
            ApiError::RateLimited {
                retry_after: Some(12),
                ..
            }
        ));
        let fallback = ApiError::from_status(429, Some("soon"), Some(&body));
        assert!(matches!(
            fallback,
            ApiError::RateLimited {
                retry_after: Some(7),
                ..
            }
        ));
        let none = ApiError::from_status(429, Some("0"), None);
        assert!(matches!(
            none,
            ApiError::RateLimited {
                retry_after: None,
                ..
            }
        ));
    }

    #[test]
    fn transport_failures_are_not_refusals() {
        assert!(!ApiError::from(TransportError::Network).is_refusal());
        assert!(!ApiError::from_status(503, None, None).is_refusal());
        assert!(ApiError::from_status(403, None, None).is_refusal());
        assert!(ApiError::SessionLost.is_refusal());
    }

    #[test]
    fn every_known_code_round_trips() {
        for code in [
            "read_only",
            "account_suspended",
            "step_up_grant_required",
            "client_not_allowed",
        ] {
            assert_eq!(ErrorCode::from(code.to_owned()).as_str(), code);
            assert!(!matches!(
                ErrorCode::from(code.to_owned()),
                ErrorCode::Other(_)
            ));
        }
    }
}
