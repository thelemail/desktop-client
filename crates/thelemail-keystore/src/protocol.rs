use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthScheme {
    SrpV1,
    OpaqueV1,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub account_id: String,
    pub email: String,
    pub unlocked: bool,
    pub has_persistent: bool,
    pub auth_scheme: AuthScheme,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub accounts: Vec<AccountStatus>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueStartAuthArgs {
    pub password: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub recovery: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueStartAuthResponse {
    pub operation_id: String,
    pub ke1: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueFinishAuthArgs {
    pub operation_id: String,
    pub account_id: String,
    pub ke2: String,
    #[serde(default)]
    pub recovery: bool,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum OpaqueFinishAuthResponse {
    Ok { ok: bool, ke3: String },
    Err { ok: bool, code: &'static str },
}

impl OpaqueFinishAuthResponse {
    pub fn ok(ke3: String) -> Self {
        Self::Ok { ok: true, ke3 }
    }
    pub fn err(code: &'static str) -> Self {
        Self::Err { ok: false, code }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueCompleteLoginUnlockArgs {
    pub operation_id: String,
    pub account_id: String,
    pub encrypted_private_key: String,
    pub wrapped_master_key: String,
    pub master_key_id: String,
    pub opaque_params_version: i64,
    pub server_auth_scheme: AuthScheme,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum OpaqueCompleteLoginUnlockResponse {
    Ok {
        ok: bool,
        #[serde(rename = "accountId")]
        account_id: String,
        email: String,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

impl OpaqueCompleteLoginUnlockResponse {
    pub fn ok(account_id: String, email: String) -> Self {
        Self::Ok {
            ok: true,
            account_id,
            email,
        }
    }
    pub fn err(code: &'static str) -> Self {
        Self::Err { ok: false, code }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountScopedArgs {
    pub account_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecryptArgs {
    pub account_id: String,
    #[serde(default)]
    pub ciphertext_armored: Option<String>,
    #[serde(default)]
    pub ciphertext_binary: Option<Vec<u8>>,
    #[serde(default)]
    pub binary: bool,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum DecryptResponse {
    Text {
        ok: bool,
        plaintext: String,
    },
    Binary {
        ok: bool,
        #[serde(rename = "plaintextBinary")]
        plaintext_binary: Vec<u8>,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

impl DecryptResponse {
    pub fn err(code: &'static str) -> Self {
        Self::Err { ok: false, code }
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum GetPublicKeyResponse {
    Ok {
        ok: bool,
        #[serde(rename = "publicKeyArmored")]
        public_key_armored: String,
        fingerprint: Vec<u8>,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Broadcast {
    VaultChanged {
        #[serde(rename = "accountId")]
        account_id: String,
        email: String,
    },
    Locked {
        #[serde(rename = "accountId")]
        account_id: String,
    },
    Cleared {
        #[serde(rename = "accountId")]
        account_id: String,
    },
    ClearedAll,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbandonArgs {
    pub operation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TryRestoreArgs {
    pub account_id: String,
    #[serde(default)]
    pub server_half: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueStartRegistrationArgs {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueStartRegistrationResponse {
    pub operation_id: String,
    pub registration_request: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueFinishRegistrationArgs {
    pub operation_id: String,
    pub account_id: String,
    pub registration_response: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum OpaqueFinishRegistrationResponse {
    Ok {
        ok: bool,
        #[serde(rename = "opaqueRecord")]
        opaque_record: String,
        #[serde(rename = "wrappedMasterKey")]
        wrapped_master_key: String,
        #[serde(rename = "masterKeyId")]
        master_key_id: String,
        #[serde(rename = "opaqueParamsVersion")]
        opaque_params_version: i64,
        #[serde(rename = "publicKey")]
        public_key: String,
        #[serde(rename = "encryptedPrivateKey")]
        encrypted_private_key: String,
        #[serde(rename = "keyAlgorithm")]
        key_algorithm: &'static str,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueFinalizeRegisterArgs {
    pub operation_id: String,
    pub account_id: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum OpaqueFinalizeRegisterResponse {
    Ok {
        ok: bool,
        #[serde(rename = "accountId")]
        account_id: String,
        email: String,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollPersistentArgs {
    pub account_id: String,
    #[serde(default)]
    pub server_half: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptArgs {
    pub account_id: String,
    pub recipient_public_key_armored: String,
    pub plaintext: Vec<u8>,
    #[serde(default)]
    pub sign_with_vault_key: bool,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum EncryptResponse {
    Ok { ok: bool, ciphertext: Vec<u8> },
    Err { ok: bool, code: &'static str },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptToKeysArgs {
    pub account_id: String,
    pub recipient_public_keys_armored: Vec<String>,
    pub plaintext: Vec<u8>,
    #[serde(default)]
    pub sign_with_vault_key: bool,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum EncryptToKeysResponse {
    Ok { ok: bool, armored: String },
    Err { ok: bool, code: &'static str },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasKeyGrantInput {
    pub alias_id: String,
    pub key_version: i64,
    pub alias_key_fingerprint_hex: String,
    pub wrapped_private_key_armored: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadAliasKeysArgs {
    pub account_id: String,
    pub grants: Vec<AliasKeyGrantInput>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedGrant {
    pub alias_id: String,
    pub key_version: i64,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum LoadAliasKeysResponse {
    Ok {
        ok: bool,
        loaded: Vec<String>,
        failed: Vec<FailedGrant>,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueRecoverySetupStartArgs {
    pub account_id: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum OpaqueRecoverySetupStartResponse {
    Ok {
        ok: bool,
        #[serde(rename = "operationId")]
        operation_id: String,
        phrase: String,
        #[serde(rename = "registrationRequest")]
        registration_request: String,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueRecoverySetupFinishArgs {
    pub account_id: String,
    pub operation_id: String,
    pub registration_response: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum RewrapResponse {
    Ok {
        ok: bool,
        #[serde(rename = "opaqueRecord")]
        opaque_record: String,
        #[serde(rename = "wrappedMasterKey")]
        wrapped_master_key: String,
        #[serde(rename = "masterKeyId")]
        master_key_id: String,
        #[serde(rename = "opaqueParamsVersion")]
        opaque_params_version: u32,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

impl RewrapResponse {
    pub fn err(code: &'static str) -> Self {
        Self::Err { ok: false, code }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueCompleteRecoveryUnlockArgs {
    pub operation_id: String,
    pub encrypted_private_key: String,
    pub wrapped_master_key: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum PlainOkResponse {
    Ok {
        ok: bool,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

impl PlainOkResponse {
    pub fn ok() -> Self {
        Self::Ok { ok: true }
    }

    pub fn err(code: &'static str) -> Self {
        Self::Err { ok: false, code }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaquePrepareCredentialResetArgs {
    pub operation_id: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum RegistrationRequestResponse {
    Ok {
        ok: bool,
        #[serde(rename = "registrationRequest")]
        registration_request: String,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueFinishCredentialResetArgs {
    pub operation_id: String,
    pub registration_response: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaquePasswordChangeStartArgs {
    pub account_id: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum OperationStartResponse {
    Ok {
        ok: bool,
        #[serde(rename = "operationId")]
        operation_id: String,
        #[serde(rename = "registrationRequest")]
        registration_request: String,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaquePasswordChangeFinishArgs {
    pub account_id: String,
    pub operation_id: String,
    pub registration_response: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaquePasswordChangeCommitArgs {
    pub account_id: String,
    pub operation_id: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum OpaquePasswordChangeCommitResponse {
    Ok { ok: bool, persisted: bool },
    Err { ok: bool, code: &'static str },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAliasKeyRecipient {
    pub account_id: String,
    pub public_key_armored: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAliasKeyArgs {
    pub account_id: String,
    pub email: String,
    pub display_name: String,
    pub recipients: Vec<CreateAliasKeyRecipient>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAliasKeyGrant {
    pub account_id: String,
    pub wrapped_private_key_armored: String,
    pub member_key_fingerprint_hex: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum CreateAliasKeyResponse {
    Ok {
        ok: bool,
        #[serde(rename = "publicKeyArmored")]
        public_key_armored: String,
        #[serde(rename = "keyFingerprintHex")]
        key_fingerprint_hex: String,
        grants: Vec<CreatedAliasKeyGrant>,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

impl CreateAliasKeyResponse {
    pub fn err(code: &'static str) -> Self {
        Self::Err { ok: false, code }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitReformattedKeyArgs {
    pub account_id: String,
    pub encrypted_private_key: String,
}
