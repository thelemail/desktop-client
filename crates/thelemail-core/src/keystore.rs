use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use thelemail_api::ApiRequest;
use thelemail_keystore::attframe::{self, DecryptedAttachmentHeader};
use thelemail_keystore::*;
use zeroize::Zeroizing;

use crate::Core;

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

pub trait AttachmentCache {
    fn get(&self, account_id: &str, attachment_id: &str) -> Option<Vec<u8>>;
    fn put(&self, account_id: &str, attachment_id: &str, plain: &[u8]);
}

async fn fetch_and_decrypt(core: &Core, args: &AttachmentArgs) -> Result<Vec<u8>, &'static str> {
    let resp = core
        .net()
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
    core.keystore().decrypt_bytes(
        &args.account_id,
        &resp.body.unwrap_or_default(),
        args.key_fingerprint_hex.as_deref(),
    )
}

pub async fn attachment_header(core: &Core, args: AttachmentArgs) -> AttachmentHeaderResponse {
    let plain = match fetch_and_decrypt(core, &args).await {
        Ok(p) => p,
        Err(code) => return AttachmentHeaderResponse::Err { ok: false, code },
    };
    match attframe::parse_header(&plain) {
        Ok((header, _)) => AttachmentHeaderResponse::Ok { ok: true, header },
        Err(_) => AttachmentHeaderResponse::Err {
            ok: false,
            code: "invalid_ciphertext",
        },
    }
}

pub async fn attachment_bytes(
    core: &Core,
    args: AttachmentArgs,
    cache: Option<&dyn AttachmentCache>,
) -> AttachmentBytesResponse {
    if let (Some(cache), Some(id)) = (cache, &args.attachment_id)
        && let Some(bytes) = cache.get(&args.account_id, id)
        && let Ok((header, payload)) = attframe::parse(&bytes)
    {
        return AttachmentBytesResponse::Ok {
            ok: true,
            header,
            payload,
        };
    }

    let plain = match fetch_and_decrypt(core, &args).await {
        Ok(p) => p,
        Err(code) => return AttachmentBytesResponse::Err { ok: false, code },
    };

    match attframe::parse(&plain) {
        Ok((header, payload)) => {
            if let (Some(cache), Some(id)) = (cache, &args.attachment_id) {
                cache.put(&args.account_id, id, &plain);
            }
            AttachmentBytesResponse::Ok {
                ok: true,
                header,
                payload,
            }
        }
        Err(_) => AttachmentBytesResponse::Err {
            ok: false,
            code: "invalid_ciphertext",
        },
    }
}

pub fn status(core: &Core) -> StatusResponse {
    let mut status = core.keystore().status();
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

pub async fn opaque_complete_login_unlock(
    core: &Arc<Core>,
    args: OpaqueCompleteLoginUnlockArgs,
) -> Result<OpaqueCompleteLoginUnlockResponse, KeystoreError> {
    let core = core.clone();
    tokio::task::spawn_blocking(move || {
        let stored = read_key_history(&args.account_id);
        core.keystore().opaque_complete_login_unlock(args, stored)
    })
    .await
    .map_err(|_| KeystoreError::Protocol)
}

pub async fn migration_finalized(
    core: &Arc<Core>,
    args: AccountScopedArgs,
) -> Result<PlainOkResponse, KeystoreError> {
    let core = core.clone();
    tokio::task::spawn_blocking(move || {
        let stored = read_key_history(&args.account_id);
        core.keystore()
            .migration_finalized(&args.account_id, stored)
    })
    .await
    .map_err(|_| KeystoreError::Protocol)
}

pub async fn opaque_finish_registration(
    core: &Core,
    args: OpaqueFinishRegistrationArgs,
) -> OpaqueFinishRegistrationResponse {
    core.keystore()
        .opaque_finish_registration(args, core.net().clock_offset_ms())
        .await
}

pub fn create_alias_key(core: &Core, args: CreateAliasKeyArgs) -> CreateAliasKeyResponse {
    core.keystore()
        .create_alias_key(args, core.net().clock_offset_ms())
}

pub async fn load_account_keys(
    core: &Arc<Core>,
    args: LoadAccountKeysArgs,
) -> Result<LoadAccountKeysResponse, KeystoreError> {
    let core = core.clone();
    tokio::task::spawn_blocking(move || core.keystore().load_account_keys(args))
        .await
        .map_err(|_| KeystoreError::Protocol)
}

pub async fn prepare_key_rotation(
    core: &Arc<Core>,
    args: PrepareKeyRotationArgs,
) -> Result<PrepareKeyRotationResponse, KeystoreError> {
    let clock_offset_ms = core.net().clock_offset_ms();
    let core = core.clone();
    tokio::task::spawn_blocking(move || core.keystore().prepare_key_rotation(args, clock_offset_ms))
        .await
        .map_err(|_| KeystoreError::Protocol)
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
    crate::paths::accounts_root()
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
    let dir = crate::paths::accounts_root().join(account_id);
    if !dir.starts_with(crate::paths::accounts_root()) {
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

pub(crate) fn save_key_history(core: &Core, account_id: &str) {
    let Some(keys) = core.keystore().stored_account_keys(account_id) else {
        return;
    };
    let Ok(path) = key_history_path(account_id) else {
        return;
    };
    if write_key_history_at(&path, keys).is_err() {
        eprintln!("keystore: saving the key history for {account_id} failed");
    }
}

pub fn forget_persisted(core: &Core, account_id: &str) {
    core.halves().discard(
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

pub(crate) fn rewrap_persisted(core: &Core, account_id: &str) {
    let Ok(path) = vault_path(account_id) else {
        return;
    };
    if path.exists()
        && !core
            .halves()
            .rewrap(&path, account_id, || core.keystore().saved_keys(account_id))
    {
        eprintln!("keystore: rewriting the saved vault for {account_id} failed");
    }
}

pub fn enroll_persistent(core: &Core, args: EnrollPersistentArgs) {
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
    core.halves().enroll(
        &Keychain,
        &path,
        &args.account_id,
        || core.keystore().persist(&args.account_id, &server_half),
        &server_half,
    );
}

pub fn try_restore_from_persistent(core: &Core, args: TryRestoreArgs) -> RestoreResponse {
    let restored = core
        .keystore()
        .try_restore_from_persistent(&args.account_id);
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
    match core.halves().restore(
        core.keystore(),
        &Keychain,
        &path,
        &args.account_id,
        &server_half,
    ) {
        Ok(email) => RestoreResponse {
            ok: true,
            account_id: Some(args.account_id),
            email: Some(email),
            reason: None,
        },
        Err(reason) => RestoreResponse::failed(reason),
    }
}

pub fn clear_account(core: &Core, account_id: &str) {
    core.halves().forget(account_id);
    core.keystore().clear(account_id);
}

pub fn lock(core: &Core, account_id: &str) {
    core.halves().forget(account_id);
    core.keystore().lock(account_id);
}

pub fn clear_all(core: &Core) {
    core.halves().forget_all();
    core.keystore().clear_all();
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
            "/../../fixtures/vault/legacy-vault-files.json"
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
