use serde::{Deserialize, Serialize};
use tauri::State;
use thelemail_api::Net;
use thelemail_crypto::attframe::{self, DecryptedAttachmentHeader};
use thelemail_keystore::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentArgs {
    pub account_id: String,
    pub url: String,
    #[serde(default)]
    pub attachment_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum AttachmentHeaderResponse {
    Ok {
        ok: bool,
        header: DecryptedAttachmentHeader,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum AttachmentBytesResponse {
    Ok {
        ok: bool,
        header: DecryptedAttachmentHeader,
        payload: Vec<u8>,
    },
    Err {
        ok: bool,
        code: &'static str,
    },
}

async fn fetch_and_decrypt(
    net: &Net,
    ks: &Keystore,
    account_id: &str,
    url: &str,
) -> Result<Vec<u8>, &'static str> {
    let ciphertext = net.blob_get(url).await.map_err(|_| "network")?;
    ks.decrypt_bytes(account_id, &ciphertext)
}

#[tauri::command]
pub async fn keystore_attachment_header(
    net: State<'_, Net>,
    ks: State<'_, Keystore>,
    args: AttachmentArgs,
) -> Result<AttachmentHeaderResponse, KeystoreError> {
    let plain = match fetch_and_decrypt(&net, &ks, &args.account_id, &args.url).await {
        Ok(p) => p,
        Err(code) => return Ok(AttachmentHeaderResponse::Err { ok: false, code }),
    };
    match attframe::parse_header(&plain) {
        Ok((header, _)) => Ok(AttachmentHeaderResponse::Ok { ok: true, header }),
        Err(_) => Ok(AttachmentHeaderResponse::Err {
            ok: false,
            code: "invalid_ciphertext",
        }),
    }
}

#[tauri::command]
pub async fn keystore_attachment_bytes(
    net: State<'_, Net>,
    ks: State<'_, Keystore>,
    mirror: State<'_, crate::mirror::Mirror>,
    args: AttachmentArgs,
) -> Result<AttachmentBytesResponse, KeystoreError> {
    let now = crate::mirror::now_iso();

    if let Some(id) = &args.attachment_id {
        let cached = mirror
            .with_conn(&args.account_id, |conn| {
                thelemail_store::list::cached_attachment(conn, id, &now).map_err(|e| e.to_string())
            })
            .ok()
            .flatten();
        if let Some(bytes) = cached
            && let Ok((header, payload)) = attframe::parse(&bytes) {
                return Ok(AttachmentBytesResponse::Ok {
                    ok: true,
                    header,
                    payload,
                });
            }
    }

    let plain = match fetch_and_decrypt(&net, &ks, &args.account_id, &args.url).await {
        Ok(p) => p,
        Err(code) => return Ok(AttachmentBytesResponse::Err { ok: false, code }),
    };

    match attframe::parse(&plain) {
        Ok((header, payload)) => {
            if let Some(id) = &args.attachment_id {
                let _ = mirror.with_conn(&args.account_id, |conn| {
                    thelemail_store::list::store_attachment(conn, id, &plain, &now)
                        .map_err(|e| e.to_string())
                });
            }
            Ok(AttachmentBytesResponse::Ok {
                ok: true,
                header,
                payload,
            })
        }
        Err(_) => Ok(AttachmentBytesResponse::Err {
            ok: false,
            code: "invalid_ciphertext",
        }),
    }
}

#[tauri::command]
pub fn keystore_status(ks: State<'_, Keystore>) -> StatusResponse {
    ks.status()
}

#[tauri::command]
pub async fn keystore_opaque_start_auth(
    ks: State<'_, Keystore>,
    args: OpaqueStartAuthArgs,
) -> Result<OpaqueStartAuthResponse, KeystoreError> {
    ks.opaque_start_auth(args).await
}

#[tauri::command]
pub async fn keystore_opaque_finish_auth(
    ks: State<'_, Keystore>,
    args: OpaqueFinishAuthArgs,
) -> Result<OpaqueFinishAuthResponse, KeystoreError> {
    Ok(ks.opaque_finish_auth(args).await)
}

#[tauri::command]
pub fn keystore_opaque_complete_login_unlock(
    ks: State<'_, Keystore>,
    args: OpaqueCompleteLoginUnlockArgs,
) -> OpaqueCompleteLoginUnlockResponse {
    ks.opaque_complete_login_unlock(args)
}

#[tauri::command]
pub fn keystore_opaque_abandon_operation(ks: State<'_, Keystore>, args: AbandonArgs) {
    ks.abandon(&args.operation_id);
}

#[tauri::command]
pub async fn keystore_opaque_start_registration(
    ks: State<'_, Keystore>,
    args: OpaqueStartRegistrationArgs,
) -> Result<OpaqueStartRegistrationResponse, KeystoreError> {
    ks.opaque_start_registration(args).await
}

#[tauri::command]
pub async fn keystore_opaque_finish_registration(
    ks: State<'_, Keystore>,
    args: OpaqueFinishRegistrationArgs,
) -> Result<OpaqueFinishRegistrationResponse, KeystoreError> {
    Ok(ks.opaque_finish_registration(args).await)
}

#[tauri::command]
pub fn keystore_opaque_finalize_register(
    ks: State<'_, Keystore>,
    args: OpaqueFinalizeRegisterArgs,
) -> OpaqueFinalizeRegisterResponse {
    ks.opaque_finalize_register(args)
}

#[tauri::command]
pub fn keystore_enroll_persistent(_ks: State<'_, Keystore>, _args: EnrollPersistentArgs) {}

#[tauri::command]
pub fn keystore_invalidate_persisted_vault(_ks: State<'_, Keystore>, _args: AccountScopedArgs) {}

#[tauri::command]
pub fn keystore_try_restore_from_persistent(
    ks: State<'_, Keystore>,
    args: TryRestoreArgs,
) -> RestoreResponse {
    ks.try_restore_from_persistent(&args.account_id)
}

#[tauri::command]
pub fn keystore_disable_persistent(_ks: State<'_, Keystore>, _args: AccountScopedArgs) {}

#[tauri::command]
pub fn keystore_clear(ks: State<'_, Keystore>, args: AccountScopedArgs) {
    ks.clear(&args.account_id);
}

#[tauri::command]
pub fn keystore_lock(ks: State<'_, Keystore>, args: AccountScopedArgs) {
    ks.lock(&args.account_id);
}

#[tauri::command]
pub fn keystore_clear_all(ks: State<'_, Keystore>) {
    ks.clear_all();
}

#[tauri::command]
pub fn keystore_decrypt(ks: State<'_, Keystore>, args: DecryptArgs) -> DecryptResponse {
    ks.decrypt(args)
}

#[tauri::command]
pub fn keystore_load_alias_keys(
    ks: State<'_, Keystore>,
    args: LoadAliasKeysArgs,
) -> LoadAliasKeysResponse {
    ks.load_alias_keys(args)
}

#[tauri::command]
pub fn keystore_unload_alias_keys(ks: State<'_, Keystore>, args: AccountScopedArgs) {
    ks.unload_alias_keys(&args.account_id);
}

#[tauri::command]
pub fn keystore_reformat_key_with_uids(
    _ks: State<'_, Keystore>,
    _args: AccountScopedArgs,
) -> serde_json::Value {
    serde_json::json!({ "ok": false, "code": "unknown" })
}

#[tauri::command]
pub async fn keystore_opaque_recovery_setup_start(
    ks: State<'_, Keystore>,
    args: OpaqueRecoverySetupStartArgs,
) -> Result<OpaqueRecoverySetupStartResponse, KeystoreError> {
    Ok(ks.opaque_recovery_setup_start(args).await)
}

#[tauri::command]
pub async fn keystore_opaque_recovery_setup_finish(
    ks: State<'_, Keystore>,
    args: OpaqueRecoverySetupFinishArgs,
) -> Result<RewrapResponse, KeystoreError> {
    Ok(ks.opaque_recovery_setup_finish(args).await)
}

#[tauri::command]
pub fn keystore_opaque_complete_recovery_unlock(
    ks: State<'_, Keystore>,
    args: OpaqueCompleteRecoveryUnlockArgs,
) -> PlainOkResponse {
    ks.opaque_complete_recovery_unlock(args)
}

#[tauri::command]
pub async fn keystore_opaque_prepare_credential_reset(
    ks: State<'_, Keystore>,
    args: OpaquePrepareCredentialResetArgs,
) -> Result<RegistrationRequestResponse, KeystoreError> {
    Ok(ks.opaque_prepare_credential_reset(args).await)
}

#[tauri::command]
pub async fn keystore_opaque_finish_credential_reset(
    ks: State<'_, Keystore>,
    args: OpaqueFinishCredentialResetArgs,
) -> Result<RewrapResponse, KeystoreError> {
    Ok(ks.opaque_finish_credential_reset(args).await)
}

#[tauri::command]
pub async fn keystore_opaque_password_change_start(
    ks: State<'_, Keystore>,
    args: OpaquePasswordChangeStartArgs,
) -> Result<OperationStartResponse, KeystoreError> {
    Ok(ks.opaque_password_change_start(args).await)
}

#[tauri::command]
pub async fn keystore_opaque_password_change_finish(
    ks: State<'_, Keystore>,
    args: OpaquePasswordChangeFinishArgs,
) -> Result<RewrapResponse, KeystoreError> {
    Ok(ks.opaque_password_change_finish(args).await)
}

#[tauri::command]
pub fn keystore_opaque_password_change_commit(
    ks: State<'_, Keystore>,
    args: OpaquePasswordChangeCommitArgs,
) -> OpaquePasswordChangeCommitResponse {
    ks.opaque_password_change_commit(args)
}

#[tauri::command]
pub fn keystore_discard_recovery(ks: State<'_, Keystore>) {
    ks.discard_recovery();
}

#[tauri::command]
pub fn keystore_abandon_password_change(ks: State<'_, Keystore>) {
    ks.abandon_password_change();
}

#[tauri::command]
pub fn keystore_create_alias_key(
    ks: State<'_, Keystore>,
    args: CreateAliasKeyArgs,
) -> CreateAliasKeyResponse {
    ks.create_alias_key(args)
}

#[tauri::command]
pub fn keystore_commit_reformatted_key(
    ks: State<'_, Keystore>,
    args: CommitReformattedKeyArgs,
) -> PlainOkResponse {
    ks.commit_reformatted_key(args)
}

#[tauri::command]
pub fn keystore_encrypt(ks: State<'_, Keystore>, args: EncryptArgs) -> EncryptResponse {
    ks.encrypt(args)
}

#[tauri::command]
pub fn keystore_encrypt_to_keys(
    ks: State<'_, Keystore>,
    args: EncryptToKeysArgs,
) -> EncryptToKeysResponse {
    ks.encrypt_to_keys(args)
}

#[tauri::command]
pub fn keystore_get_public_key(
    ks: State<'_, Keystore>,
    args: AccountScopedArgs,
) -> GetPublicKeyResponse {
    ks.get_public_key(&args.account_id)
}
