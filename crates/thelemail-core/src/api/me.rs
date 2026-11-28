use serde::Deserialize;

use super::wire::wire_enum;
use super::{Api, ApiError, Call};

wire_enum!(AccountStatus {
    Active => "active",
    Disabled => "disabled",
    Suspended => "suspended",
});

wire_enum!(LifecycleStage {
    Active => "active",
    Grace => "grace",
    Suspended => "suspended",
    PendingDeletion => "pending_deletion",
});

wire_enum!(KeyRotationBlocker {
    SignInUpgradeRequired => "sign_in_upgrade_required",
    RecoveryUpgradeRequired => "recovery_upgrade_required",
});

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionStatus {
    pub requested_at: String,
    pub purge_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WelcomeBack {
    #[serde(default)]
    pub messages_during_grace: Option<u64>,
    #[serde(default)]
    pub bounce_from: Option<String>,
    #[serde(default)]
    pub bounce_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleInfo {
    pub stage: LifecycleStage,
    pub day0: String,
    pub suspend_at: String,
    pub deletion_date: String,
    pub expiry_screen_shown: bool,
    #[serde(default)]
    pub welcome_back: Option<WelcomeBack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub account_id: String,
    #[serde(default)]
    pub auth_origin_enrolled: bool,
    #[serde(default)]
    pub product_keys_active: bool,
    pub email: String,
    #[serde(default)]
    pub full_name: String,
    pub status: AccountStatus,
    pub created_at: String,
    #[serde(default)]
    pub recovery_enabled: bool,
    #[serde(default)]
    pub avatar_path: Option<String>,
    #[serde(default)]
    pub default_reply_address_id: Option<String>,
    #[serde(default)]
    pub deletion: Option<DeletionStatus>,
    #[serde(default)]
    pub lifecycle: Option<LifecycleInfo>,
    pub key_fingerprint: String,
    pub key_version: i64,
    #[serde(default)]
    pub key_rotation_blocker: Option<KeyRotationBlocker>,
}

pub async fn get(api: Api<'_>, account_id: &str) -> Result<Me, ApiError> {
    api.json(account_id, Call::get("/v1/me")).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minimal_profile_reads_with_defaults() {
        let me: Me = serde_json::from_value(serde_json::json!({
            "accountId": "a", "email": "ada@thelemail.test", "fullName": "Ada",
            "status": "active", "createdAt": "2026-01-01T00:00:00Z",
            "keyFingerprint": "ab", "keyVersion": 2
        }))
        .unwrap();
        assert_eq!(me.status, AccountStatus::Active);
        assert!(!me.recovery_enabled);
        assert_eq!(me.lifecycle, None);
        assert_eq!(me.key_rotation_blocker, None);
    }

    #[test]
    fn new_server_values_do_not_break_the_profile() {
        let me: Me = serde_json::from_value(serde_json::json!({
            "accountId": "a", "email": "e", "fullName": "", "status": "frozen",
            "createdAt": "t", "keyFingerprint": "ab", "keyVersion": 1,
            "keyRotationBlocker": "something_new",
            "lifecycle": {"stage": "grace", "day0": "d", "suspendAt": "s",
                "deletionDate": "x", "expiryScreenShown": false,
                "welcomeBack": {"messagesDuringGrace": 3, "bounceFrom": null}}
        }))
        .unwrap();
        assert_eq!(me.status, AccountStatus::Other("frozen".into()));
        assert_eq!(
            me.key_rotation_blocker,
            Some(KeyRotationBlocker::Other("something_new".into()))
        );
        let lifecycle = me.lifecycle.unwrap();
        assert_eq!(lifecycle.stage, LifecycleStage::Grace);
        assert_eq!(
            lifecycle.welcome_back.unwrap().messages_during_grace,
            Some(3)
        );
    }
}
