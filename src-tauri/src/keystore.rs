use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use thelemail_api::{ApiRequest, Net};
use thelemail_keystore::attframe::{self, DecryptedAttachmentHeader};
use thelemail_keystore::*;
use zeroize::Zeroizing;

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentArgs {
    pub account_id: String,
    pub url: String,
    #[serde(default)]
    pub attachment_id: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub key_fingerprint_hex: Option<String>,
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
    args: &AttachmentArgs,
) -> Result<Vec<u8>, &'static str> {
    let resp = net
        .fetch_blob(ApiRequest {
            url: args.url.clone(),
            method: "GET".to_owned(),
            headers: args.headers.clone(),
            body: None,
        })
        .await
        .map_err(|_| "network")?;
    if !(200..300).contains(&resp.status) {
        return Err("network");
    }
    ks.decrypt_bytes(
        &args.account_id,
        &resp.body.unwrap_or_default(),
        args.key_fingerprint_hex.as_deref(),
    )
}

#[tauri::command]
pub async fn keystore_attachment_header(
    net: State<'_, Net>,
    ks: State<'_, Keystore>,
    args: AttachmentArgs,
) -> Result<AttachmentHeaderResponse, KeystoreError> {
    let plain = match fetch_and_decrypt(&net, &ks, &args).await {
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
            && let Ok((header, payload)) = attframe::parse(&bytes)
        {
            return Ok(AttachmentBytesResponse::Ok {
                ok: true,
                header,
                payload,
            });
        }
    }

    let plain = match fetch_and_decrypt(&net, &ks, &args).await {
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
    let mut status = ks.status();
    for (account_id, email) in persisted_accounts() {
        match status
            .accounts
            .iter_mut()
            .find(|a| a.account_id == account_id)
        {
            Some(existing) => existing.has_persistent = true,
            None => status.accounts.push(AccountStatus {
                account_id,
                email,
                unlocked: false,
                has_persistent: true,
                auth_scheme: AuthScheme::OpaqueV1,
            }),
        }
    }
    status
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
pub async fn keystore_opaque_complete_login_unlock(
    app: AppHandle,
    args: OpaqueCompleteLoginUnlockArgs,
) -> Result<OpaqueCompleteLoginUnlockResponse, KeystoreError> {
    tauri::async_runtime::spawn_blocking(move || {
        let stored = read_key_history(&args.account_id);
        app.state::<Keystore>()
            .opaque_complete_login_unlock(args, stored)
    })
    .await
    .map_err(|_| KeystoreError::Protocol)
}

#[tauri::command]
pub async fn keystore_migration_finalized(
    app: AppHandle,
    args: AccountScopedArgs,
) -> Result<PlainOkResponse, KeystoreError> {
    tauri::async_runtime::spawn_blocking(move || {
        let stored = read_key_history(&args.account_id);
        app.state::<Keystore>()
            .migration_finalized(&args.account_id, stored)
    })
    .await
    .map_err(|_| KeystoreError::Protocol)
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
    net: State<'_, Net>,
    args: OpaqueFinishRegistrationArgs,
) -> Result<OpaqueFinishRegistrationResponse, KeystoreError> {
    Ok(ks
        .opaque_finish_registration(args, net.clock_offset_ms())
        .await)
}

#[tauri::command]
pub fn keystore_opaque_finalize_register(
    ks: State<'_, Keystore>,
    args: OpaqueFinalizeRegisterArgs,
) -> OpaqueFinalizeRegisterResponse {
    ks.opaque_finalize_register(args)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultFile {
    email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wrapped: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<SessionFile>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionFile {
    iv: String,
    cipher_text: String,
    mac: String,
    wrap_version: i64,
    keys: SavedKeys,
}

fn accounts_root() -> std::path::PathBuf {
    crate::mirror::Mirror::root().join("accounts")
}

fn persisted_accounts() -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(accounts_root()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Some(account_id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if crate::ids::account_id(&account_id).is_err() {
            continue;
        }
        let Ok(raw) = std::fs::read(entry.path().join("vault.bin")) else {
            continue;
        };
        if let Ok(file) = serde_json::from_slice::<VaultFile>(&raw) {
            out.push((account_id, file.email));
        }
    }
    out
}

fn vault_path(account_id: &str) -> Result<std::path::PathBuf, String> {
    crate::ids::account_id(account_id)?;
    let dir = crate::mirror::Mirror::root()
        .join("accounts")
        .join(account_id);
    if !dir.starts_with(crate::mirror::Mirror::root().join("accounts")) {
        return Err("invalid account".to_owned());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("vault.bin"))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyHistoryFile {
    keys: Vec<StoredAccountKey>,
}

static KEY_HISTORY_WRITES: Mutex<()> = Mutex::new(());

fn key_history_path(account_id: &str) -> Result<std::path::PathBuf, String> {
    Ok(vault_path(account_id)?.with_file_name("keys.json"))
}

fn read_key_history_at(path: &std::path::Path) -> Vec<StoredAccountKey> {
    std::fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<KeyHistoryFile>(&raw).ok())
        .map(|file| file.keys)
        .unwrap_or_default()
}

fn read_key_history(account_id: &str) -> Vec<StoredAccountKey> {
    key_history_path(account_id)
        .map(|path| read_key_history_at(&path))
        .unwrap_or_default()
}

fn merged_key_history(
    mut current: Vec<StoredAccountKey>,
    saved: Vec<StoredAccountKey>,
) -> Vec<StoredAccountKey> {
    for entry in &mut current {
        if entry.encrypted_private_key_armored.is_none() {
            entry.encrypted_private_key_armored = saved
                .iter()
                .find(|s| {
                    s.fingerprint_hex
                        .eq_ignore_ascii_case(&entry.fingerprint_hex)
                })
                .and_then(|s| s.encrypted_private_key_armored.clone());
        }
    }
    current.retain(|entry| entry.encrypted_private_key_armored.is_some());
    current
}

fn write_key_history_at(path: &std::path::Path, keys: Vec<StoredAccountKey>) -> Result<(), ()> {
    let _writes = KEY_HISTORY_WRITES.lock().expect("key history writes");
    let keys = merged_key_history(keys, read_key_history_at(path));
    if keys.is_empty() {
        return Ok(());
    }
    let encoded = serde_json::to_vec(&KeyHistoryFile { keys }).map_err(|_| ())?;
    let staged = path.with_extension("json.tmp");
    std::fs::write(&staged, encoded).map_err(|_| ())?;
    std::fs::rename(&staged, path).map_err(|_| {
        let _ = std::fs::remove_file(&staged);
    })
}

pub(crate) fn save_key_history(ks: &Keystore, account_id: &str) {
    let Some(keys) = ks.stored_account_keys(account_id) else {
        return;
    };
    let Ok(path) = key_history_path(account_id) else {
        return;
    };
    if write_key_history_at(&path, keys).is_err() {
        eprintln!("keystore: saving the key history for {account_id} failed");
    }
}

pub(crate) fn forget_persisted(halves: &PersistentHalves, account_id: &str) {
    halves.discard(
        &Keychain,
        vault_path(account_id).ok().as_deref(),
        account_id,
    );
}

pub(crate) trait LocalHalfStore {
    fn read(&self, account_id: &str) -> Option<Zeroizing<Vec<u8>>>;
    fn write(&self, account_id: &str, local_half: &[u8]) -> bool;
    fn forget(&self, account_id: &str);
}

struct Keychain;

impl LocalHalfStore for Keychain {
    fn read(&self, account_id: &str) -> Option<Zeroizing<Vec<u8>>> {
        match crate::keychain::local_half(account_id) {
            crate::keychain::Read::Found(encoded) => b64().decode(encoded).ok().map(Zeroizing::new),
            _ => None,
        }
    }

    fn write(&self, account_id: &str, local_half: &[u8]) -> bool {
        crate::keychain::put_local_half(account_id, &b64().encode(local_half)).is_ok()
    }

    fn forget(&self, account_id: &str) {
        let _ = crate::keychain::forget_local_half(account_id);
    }
}

#[derive(Default)]
pub struct PersistentHalves {
    server_halves: Mutex<HashMap<String, Zeroizing<Vec<u8>>>>,
    files: Mutex<()>,
}

impl PersistentHalves {
    fn remember(&self, account_id: &str, server_half: &[u8]) {
        self.server_halves
            .lock()
            .expect("persistent halves")
            .insert(account_id.to_owned(), Zeroizing::new(server_half.to_vec()));
    }

    fn get(&self, account_id: &str) -> Option<Zeroizing<Vec<u8>>> {
        self.server_halves
            .lock()
            .expect("persistent halves")
            .get(account_id)
            .cloned()
    }

    fn forget(&self, account_id: &str) {
        self.server_halves
            .lock()
            .expect("persistent halves")
            .remove(account_id);
    }

    fn forget_all(&self) {
        self.server_halves
            .lock()
            .expect("persistent halves")
            .clear();
    }

    fn enroll(
        &self,
        store: &impl LocalHalfStore,
        path: &std::path::Path,
        account_id: &str,
        persisted: impl FnOnce() -> Option<PersistedSession>,
        server_half: &[u8],
    ) -> bool {
        let _files = self.files.lock().expect("persistent files");
        let Some(persisted) = persisted() else {
            return false;
        };
        if !replace_session(store, path, account_id, &persisted) {
            return false;
        }
        self.remember(account_id, server_half);
        true
    }

    fn rewrap(
        &self,
        path: &std::path::Path,
        account_id: &str,
        keys: impl FnOnce() -> Option<SavedKeys>,
    ) -> bool {
        let _files = self.files.lock().expect("persistent files");
        if self.get(account_id).is_none() {
            return false;
        }
        let Ok(VaultFile {
            email,
            session: Some(mut session),
            ..
        }) = read_vault_file(path)
        else {
            return false;
        };
        let Some(keys) = keys() else {
            return false;
        };
        session.keys = keys;
        write_vault_file(
            path,
            &VaultFile {
                email,
                wrapped: None,
                session: Some(session),
            },
        )
        .is_ok()
    }

    fn restore(
        &self,
        ks: &Keystore,
        store: &impl LocalHalfStore,
        path: &std::path::Path,
        account_id: &str,
        server_half: &[u8],
    ) -> Result<String, &'static str> {
        let _files = self.files.lock().expect("persistent files");
        let local_half = store.read(account_id).ok_or("no_persistent")?;
        let raw = std::fs::read(path).map_err(|_| "no_persistent")?;
        let file = serde_json::from_slice::<VaultFile>(&raw).map_err(|_| "no_persistent")?;
        let email = match (file.session, file.wrapped) {
            (Some(session), _) => {
                let wrapped = WrappedSecret {
                    local_half: b64().encode(&*local_half),
                    iv: session.iv,
                    cipher_text: session.cipher_text,
                    mac: session.mac,
                };
                if !ks.restore(
                    account_id,
                    file.email.clone(),
                    &session.keys,
                    &wrapped,
                    session.wrap_version,
                    server_half,
                ) {
                    return Err("unwrap_failed");
                }
                file.email
            }
            (None, Some(_)) => {
                let legacy = std::str::from_utf8(&raw).map_err(|_| "no_persistent")?;
                let email = ks
                    .import_desktop_vault(account_id, legacy, &local_half, server_half)
                    .ok_or("unwrap_failed")?;
                let moved = ks
                    .persist(account_id, server_half)
                    .is_some_and(|persisted| replace_session(store, path, account_id, &persisted));
                if !moved {
                    eprintln!(
                        "keystore: moving the saved vault for {account_id} to the new format failed"
                    );
                }
                email
            }
            (None, None) => return Err("no_persistent"),
        };
        self.remember(account_id, server_half);
        Ok(email)
    }

    fn discard(
        &self,
        store: &impl LocalHalfStore,
        path: Option<&std::path::Path>,
        account_id: &str,
    ) {
        let _files = self.files.lock().expect("persistent files");
        self.forget(account_id);
        if let Some(path) = path {
            let _ = std::fs::remove_file(path);
        }
        store.forget(account_id);
    }
}

fn replace_session(
    store: &impl LocalHalfStore,
    path: &std::path::Path,
    account_id: &str,
    persisted: &PersistedSession,
) -> bool {
    let Ok(local_half) = b64().decode(&persisted.wrapped.local_half) else {
        return false;
    };
    let local_half = Zeroizing::new(local_half);
    let file = VaultFile {
        email: persisted.email.clone(),
        wrapped: None,
        session: Some(SessionFile {
            iv: persisted.wrapped.iv.clone(),
            cipher_text: persisted.wrapped.cipher_text.clone(),
            mac: persisted.wrapped.mac.clone(),
            wrap_version: persisted.wrap_version,
            keys: persisted.keys.clone(),
        }),
    };
    let staged = path.with_extension("bin.new");
    if write_file(&staged, &file).is_err() {
        return false;
    }
    let previous = store.read(account_id);
    if !store.write(account_id, &local_half) {
        let _ = std::fs::remove_file(&staged);
        return false;
    }
    if std::fs::rename(&staged, path).is_err() {
        let _ = std::fs::remove_file(&staged);
        match previous {
            Some(previous) => {
                store.write(account_id, &previous);
            }
            None => store.forget(account_id),
        }
        return false;
    }
    true
}

fn write_file(path: &std::path::Path, file: &VaultFile) -> Result<(), ()> {
    let encoded = serde_json::to_vec(file).map_err(|_| ())?;
    std::fs::write(path, encoded).map_err(|_| ())
}

fn write_vault_file(path: &std::path::Path, file: &VaultFile) -> Result<(), ()> {
    let staged = path.with_extension("bin.tmp");
    write_file(&staged, file)?;
    std::fs::rename(&staged, path).map_err(|_| {
        let _ = std::fs::remove_file(&staged);
    })
}

fn read_vault_file(path: &std::path::Path) -> Result<VaultFile, ()> {
    let raw = std::fs::read(path).map_err(|_| ())?;
    serde_json::from_slice::<VaultFile>(&raw).map_err(|_| ())
}

pub(crate) fn rewrap_persisted(ks: &Keystore, halves: &PersistentHalves, account_id: &str) {
    let Ok(path) = vault_path(account_id) else {
        return;
    };
    if path.exists() && !halves.rewrap(&path, account_id, || ks.saved_keys(account_id)) {
        eprintln!("keystore: rewriting the saved vault for {account_id} failed");
    }
}

#[tauri::command]
pub fn keystore_enroll_persistent(
    ks: State<'_, Keystore>,
    halves: State<'_, PersistentHalves>,
    args: EnrollPersistentArgs,
) {
    let Some(server_half) = args.server_half else {
        return;
    };
    let Ok(server_half) = b64().decode(&server_half) else {
        return;
    };
    let server_half = Zeroizing::new(server_half);
    let Ok(path) = vault_path(&args.account_id) else {
        return;
    };
    halves.enroll(
        &Keychain,
        &path,
        &args.account_id,
        || ks.persist(&args.account_id, &server_half),
        &server_half,
    );
}

#[tauri::command]
pub fn keystore_invalidate_persisted_vault(
    halves: State<'_, PersistentHalves>,
    args: AccountScopedArgs,
) {
    forget_persisted(&halves, &args.account_id);
}

#[tauri::command]
pub fn keystore_try_restore_from_persistent(
    ks: State<'_, Keystore>,
    halves: State<'_, PersistentHalves>,
    args: TryRestoreArgs,
) -> RestoreResponse {
    let restored = ks.try_restore_from_persistent(&args.account_id);
    if restored.ok {
        return restored;
    }

    let Some(server_half) = args.server_half.as_deref() else {
        return RestoreResponse::failed("server_half_unavailable");
    };
    let Ok(server_half) = b64().decode(server_half) else {
        return RestoreResponse::failed("no_persistent");
    };
    let server_half = Zeroizing::new(server_half);
    let Ok(path) = vault_path(&args.account_id) else {
        return RestoreResponse::failed("no_persistent");
    };
    match halves.restore(&ks, &Keychain, &path, &args.account_id, &server_half) {
        Ok(email) => RestoreResponse {
            ok: true,
            account_id: Some(args.account_id),
            email: Some(email),
            reason: None,
        },
        Err(reason) => RestoreResponse::failed(reason),
    }
}

#[tauri::command]
pub fn keystore_disable_persistent(halves: State<'_, PersistentHalves>, args: AccountScopedArgs) {
    forget_persisted(&halves, &args.account_id);
}

#[tauri::command]
pub fn keystore_clear(
    ks: State<'_, Keystore>,
    halves: State<'_, PersistentHalves>,
    args: AccountScopedArgs,
) {
    halves.forget(&args.account_id);
    ks.clear(&args.account_id);
}

#[tauri::command]
pub fn keystore_lock(
    ks: State<'_, Keystore>,
    halves: State<'_, PersistentHalves>,
    args: AccountScopedArgs,
) {
    halves.forget(&args.account_id);
    ks.lock(&args.account_id);
}

#[tauri::command]
pub fn keystore_clear_all(ks: State<'_, Keystore>, halves: State<'_, PersistentHalves>) {
    halves.forget_all();
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
    net: State<'_, Net>,
    args: CreateAliasKeyArgs,
) -> CreateAliasKeyResponse {
    ks.create_alias_key(args, net.clock_offset_ms())
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
pub fn keystore_sign_detached(
    ks: State<'_, Keystore>,
    args: SignDetachedArgs,
) -> SignDetachedResponse {
    ks.sign_detached(args)
}

#[tauri::command]
pub async fn keystore_load_account_keys(
    app: AppHandle,
    args: LoadAccountKeysArgs,
) -> Result<LoadAccountKeysResponse, KeystoreError> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Keystore>().load_account_keys(args))
        .await
        .map_err(|_| KeystoreError::Protocol)
}

#[tauri::command]
pub async fn keystore_prepare_key_rotation(
    app: AppHandle,
    args: PrepareKeyRotationArgs,
) -> Result<PrepareKeyRotationResponse, KeystoreError> {
    let clock_offset_ms = app.state::<Net>().clock_offset_ms();
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Keystore>()
            .prepare_key_rotation(args, clock_offset_ms)
    })
    .await
    .map_err(|_| KeystoreError::Protocol)
}

#[tauri::command]
pub fn keystore_commit_key_rotation(
    ks: State<'_, Keystore>,
    args: CommitKeyRotationArgs,
) -> CommitKeyRotationResponse {
    ks.commit_key_rotation(args)
}

#[tauri::command]
pub fn keystore_list_account_keys(
    ks: State<'_, Keystore>,
    args: AccountScopedArgs,
) -> ListAccountKeysResponse {
    ks.list_account_keys(&args.account_id)
}

#[tauri::command]
pub fn keystore_abandon_key_rotation(ks: State<'_, Keystore>, args: AccountScopedArgs) {
    ks.abandon_key_rotation(&args.account_id);
}

#[tauri::command]
pub fn keystore_get_public_key(
    ks: State<'_, Keystore>,
    args: AccountScopedArgs,
) -> GetPublicKeyResponse {
    ks.get_public_key(&args.account_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const ACCOUNT: &str = "0c0c0c0c-0c0c-4c0c-8c0c-0c0c0c0c0c0c";

    #[derive(Default)]
    struct MemoryHalves(Mutex<HashMap<String, Vec<u8>>>);

    impl LocalHalfStore for MemoryHalves {
        fn read(&self, account_id: &str) -> Option<Zeroizing<Vec<u8>>> {
            self.0
                .lock()
                .unwrap()
                .get(account_id)
                .cloned()
                .map(Zeroizing::new)
        }

        fn write(&self, account_id: &str, local_half: &[u8]) -> bool {
            self.0
                .lock()
                .unwrap()
                .insert(account_id.to_owned(), local_half.to_vec());
            true
        }

        fn forget(&self, account_id: &str) {
            self.0.lock().unwrap().remove(account_id);
        }
    }

    fn session(mark: u8, key_version: i64) -> PersistedSession {
        PersistedSession {
            wrapped: WrappedSecret {
                local_half: b64().encode([mark; 32]),
                iv: format!("iv-{mark}"),
                cipher_text: format!("ct-{mark}"),
                mac: format!("mac-{mark}"),
            },
            wrap_version: 2,
            keys: keys(key_version),
            email: "nora@thelemail.test".to_owned(),
        }
    }

    fn keys(key_version: i64) -> SavedKeys {
        SavedKeys {
            encrypted_private_key: format!("encrypted-{key_version}"),
            key_version,
            key_anchored: true,
            history_keys: (1..key_version)
                .map(|v| HistoryKey {
                    key_version: v,
                    encrypted_private_key: format!("encrypted-{v}"),
                })
                .collect(),
        }
    }

    fn stored(version: i64, encrypted: Option<&str>) -> StoredAccountKey {
        StoredAccountKey {
            key_version: version,
            fingerprint_hex: format!("{version:0>40}"),
            encrypted_private_key_armored: encrypted.map(str::to_owned),
            fixed: false,
        }
    }

    #[test]
    fn the_key_history_keeps_saved_keys_the_vault_holds_only_unlocked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys.json");

        write_key_history_at(&path, vec![stored(2, Some("v2")), stored(1, Some("v1"))]).unwrap();
        write_key_history_at(
            &path,
            vec![stored(3, Some("v3")), stored(2, None), stored(1, None)],
        )
        .unwrap();
        let saved = read_key_history_at(&path);
        let versions: Vec<_> = saved
            .iter()
            .map(|k| (k.key_version, k.encrypted_private_key_armored.as_deref()))
            .collect();
        assert_eq!(
            versions,
            vec![(3, Some("v3")), (2, Some("v2")), (1, Some("v1"))]
        );
        assert!(!path.with_extension("json.tmp").exists());

        write_key_history_at(&path, vec![stored(4, None)]).unwrap();
        assert_eq!(read_key_history_at(&path).len(), 3);
    }

    fn opened(store: &MemoryHalves, path: &std::path::Path) -> SessionFile {
        let session = read_vault_file(path)
            .ok()
            .and_then(|f| f.session)
            .expect("the saved vault is in the session format");
        let local_half = store.read(ACCOUNT).expect("local half");
        assert_eq!(
            session.iv,
            format!("iv-{}", local_half[0]),
            "the file belongs to the stored local half"
        );
        session
    }

    #[test]
    fn a_key_change_rewrites_the_saved_keys_under_the_same_halves() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.bin");
        let halves = PersistentHalves::default();
        let store = MemoryHalves::default();

        assert!(halves.enroll(&store, &path, ACCOUNT, || Some(session(1, 1)), &[7u8; 32]));
        assert_eq!(opened(&store, &path).keys.key_version, 1);

        assert!(halves.rewrap(&path, ACCOUNT, || Some(keys(3))));
        let reopened = opened(&store, &path);
        assert_eq!(reopened.keys.key_version, 3);
        assert_eq!(reopened.keys.history_keys.len(), 2);
        assert_eq!(reopened.iv, "iv-1");
        assert!(!path.with_extension("bin.tmp").exists());
        assert!(!path.with_extension("bin.new").exists());
    }

    #[test]
    fn a_rewrite_without_a_saved_vault_or_server_half_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.bin");
        let halves = PersistentHalves::default();
        let store = MemoryHalves::default();

        assert!(!halves.rewrap(&path, ACCOUNT, || Some(keys(2))));
        assert!(!path.exists());

        assert!(halves.enroll(&store, &path, ACCOUNT, || Some(session(1, 1)), &[1u8; 32]));
        halves.forget(ACCOUNT);
        assert!(!halves.rewrap(&path, ACCOUNT, || Some(keys(2))));
        assert_eq!(opened(&store, &path).keys.key_version, 1);
    }

    #[test]
    fn a_discarded_vault_is_not_brought_back_by_a_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.bin");
        let halves = PersistentHalves::default();
        let store = MemoryHalves::default();

        assert!(halves.enroll(&store, &path, ACCOUNT, || Some(session(1, 1)), &[2u8; 32]));
        halves.discard(&store, Some(&path), ACCOUNT);
        assert!(!halves.rewrap(&path, ACCOUNT, || Some(keys(2))));
        assert!(!path.exists());
        assert!(store.read(ACCOUNT).is_none());
    }

    #[test]
    fn enrolling_while_keys_change_always_leaves_a_vault_that_opens() {
        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join("vault.bin"));
        let halves = Arc::new(PersistentHalves::default());
        let store = Arc::new(MemoryHalves::default());
        let server_half = [9u8; 32];
        assert!(halves.enroll(
            &*store,
            &path,
            ACCOUNT,
            || Some(session(1, 1)),
            &server_half
        ));

        let enroller = {
            let (halves, store, path) = (halves.clone(), store.clone(), path.clone());
            std::thread::spawn(move || {
                for mark in 0..200u8 {
                    assert!(halves.enroll(
                        &*store,
                        &path,
                        ACCOUNT,
                        || Some(session(mark, 1)),
                        &server_half
                    ));
                }
            })
        };
        let rewriter = {
            let (halves, store, path) = (halves.clone(), store.clone(), path.clone());
            std::thread::spawn(move || {
                for version in 2..202 {
                    halves.rewrap(&path, ACCOUNT, || Some(keys(version)));
                    let _files = halves.files.lock().unwrap();
                    opened(&store, &path);
                }
            })
        };
        enroller.join().unwrap();
        rewriter.join().unwrap();
        opened(&store, &path);
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct LegacyFixture {
        local_half: String,
        server_half: String,
        cases: Vec<LegacyCase>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct LegacyCase {
        name: String,
        vault_file: String,
        messages: Vec<LegacyMessage>,
    }

    #[derive(Deserialize)]
    struct LegacyMessage {
        ciphertext: String,
        plaintext: String,
    }

    fn decrypts_all(ks: &Keystore, case: &LegacyCase) {
        for m in &case.messages {
            let ciphertext = b64().decode(&m.ciphertext).unwrap();
            let plain = ks
                .decrypt_bytes(ACCOUNT, &ciphertext, None)
                .unwrap_or_else(|code| panic!("{}: {code}", case.name));
            assert_eq!(plain, m.plaintext.as_bytes(), "{}", case.name);
        }
    }

    #[test]
    fn a_vault_saved_by_an_earlier_build_unlocks_and_moves_to_the_new_format() {
        let raw = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../fixtures/vault/legacy-vault-files.json"
        ))
        .unwrap();
        let fixture: LegacyFixture = serde_json::from_slice(&raw).unwrap();
        let local_half = b64().decode(&fixture.local_half).unwrap();
        let server_half = b64().decode(&fixture.server_half).unwrap();

        for case in &fixture.cases {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("vault.bin");
            std::fs::write(&path, &case.vault_file).unwrap();
            let store = MemoryHalves::default();
            store.write(ACCOUNT, &local_half);
            let halves = PersistentHalves::default();
            let ks = Keystore::new();

            let email = halves
                .restore(&ks, &store, &path, ACCOUNT, &server_half)
                .unwrap_or_else(|code| panic!("{}: {code}", case.name));
            assert_eq!(email, "nora@thelemail.test");
            decrypts_all(&ks, case);

            let migrated = read_vault_file(&path).unwrap();
            assert!(
                migrated.wrapped.is_none() && migrated.session.is_some(),
                "{}",
                case.name
            );
            assert_ne!(*store.read(ACCOUNT).unwrap(), local_half);
            let saved = std::fs::read_to_string(&path).unwrap();
            assert!(
                !saved.contains(&b64().encode([0x5au8; 32])),
                "the master key reached the file in the clear"
            );

            ks.lock(ACCOUNT);
            assert!(ks.decrypt_bytes(ACCOUNT, b"x", None).is_err());
            halves
                .restore(&ks, &store, &path, ACCOUNT, &server_half)
                .unwrap_or_else(|code| panic!("{} after the move: {code}", case.name));
            decrypts_all(&ks, case);

            ks.lock(ACCOUNT);
            let mut wrong = server_half.clone();
            wrong[0] ^= 1;
            assert_eq!(
                halves.restore(&ks, &store, &path, ACCOUNT, &wrong),
                Err("unwrap_failed")
            );
        }
    }
}
