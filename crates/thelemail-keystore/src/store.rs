use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thelemail_trust::{Empty, Family, TrustError};
use tokio::sync::{Semaphore, broadcast};
use zeroize::Zeroizing;

use crate::protocol::*;

const KEY_ROTATION_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_ENDORSING_KEYS: i64 = 16;
const KEY_ALGORITHM: &str = "openpgp-curve25519-v6";
const MASTER_KEY_MISMATCH: &str = "master key id does not match";
const KEY_LIST_INVALID: &str = "key_list_invalid";

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn trust<T: DeserializeOwned>(family: Family, request: &impl Serialize) -> Result<T, TrustError> {
    thelemail_trust::call(family, request)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PasswordRequest<'a> {
    op: &'static str,
    password: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Started {
    operation_id: String,
    message: String,
}

#[derive(Deserialize)]
struct LoginFinished {
    ke3: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registered {
    opaque_record: String,
    wrapped_master_key: String,
    master_key_id: String,
    opaque_params_version: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeneratedKey {
    public_key: String,
    encrypted_private_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyState {
    key_version: i64,
    fingerprint: String,
    encrypted_private_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeysState {
    current: KeyState,
    history: Vec<KeyState>,
    changed: bool,
    rejected: Vec<i64>,
    unverified: Vec<i64>,
    anchored: bool,
}

impl KeysState {
    fn fingerprints(&self) -> Vec<String> {
        std::iter::once(&self.current)
            .chain(&self.history)
            .map(|k| k.fingerprint.clone())
            .collect()
    }

    fn holds(&self, fingerprint_hex: &str) -> bool {
        std::iter::once(&self.current)
            .chain(&self.history)
            .any(|k| k.fingerprint == fingerprint_hex)
    }

    fn history_is_complete(&self) -> bool {
        let current = self.current.key_version;
        if current < 1 {
            return false;
        }
        let oldest = (current - MAX_ENDORSING_KEYS + 1).max(1);
        (oldest..current).all(|version| self.history.iter().any(|k| k.key_version == version))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Endorsement {
    previous_key_fingerprint: String,
    signature: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreparedRotation {
    public_key_armored: String,
    encrypted_private_key: String,
    fingerprint: String,
    expected_key_version: i64,
    transitions: Vec<Endorsement>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicKey {
    public_key: String,
    fingerprint: String,
    key_version: i64,
}

#[derive(Deserialize)]
struct Phrase {
    phrase: String,
}

#[derive(Deserialize)]
struct Decrypted {
    items: Vec<DecryptedItem>,
}

#[derive(Deserialize)]
struct DecryptedItem {
    data: Option<String>,
    signature: Option<Verdict>,
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Verdict {
    state: String,
    fingerprint: Option<String>,
    signed_at_millis: Option<i64>,
}

#[derive(Deserialize)]
struct Encrypted {
    data: Option<String>,
    armored: Option<String>,
}

#[derive(Deserialize)]
struct SignedData {
    signature: String,
    fingerprint: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeneratedAlias {
    public_key_armored: String,
    fingerprint_hex: String,
    grants: HashMap<String, String>,
}

#[derive(Deserialize)]
struct Accepted {
    accepted: Vec<bool>,
}

#[derive(Deserialize)]
struct Inspected {
    fingerprint: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Persisted {
    wrapped: WrappedSecret,
    wrap_version: i64,
}

#[derive(Deserialize)]
struct ImportedVault {
    email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WrappedSecret {
    pub local_half: String,
    pub iv: String,
    pub cipher_text: String,
    pub mac: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryKey {
    pub key_version: i64,
    pub encrypted_private_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedKeys {
    pub encrypted_private_key: String,
    pub key_version: i64,
    #[serde(default)]
    pub key_anchored: bool,
    #[serde(default)]
    pub history_keys: Vec<HistoryKey>,
}

pub struct PersistedSession {
    pub wrapped: WrappedSecret,
    pub wrap_version: i64,
    pub keys: SavedKeys,
    pub email: String,
}

#[derive(Clone, Copy, PartialEq)]
enum RegKind {
    Register,
    RecoverySetup,
    PasswordChange,
}

struct PendingReg {
    kind: RegKind,
    email: String,
    account_id: Option<String>,
    finished: bool,
}

struct PendingLogin {
    email: Option<String>,
    recovery: bool,
    account_id: Option<String>,
    recovered: bool,
    reset_operation: Option<String>,
}

struct LoadedAlias {
    alias_id: String,
    key_version: i64,
    grant: AliasKeyGrantInput,
}

struct PendingRotation {
    fingerprint_hex: String,
    key_version: i64,
    expires_at: Instant,
}

struct Account {
    email: String,
    auth_scheme: AuthScheme,
    aliases: HashMap<String, LoadedAlias>,
    unopened: Vec<AliasKeyGrantInput>,
    rotation: Option<PendingRotation>,
}

impl Account {
    fn new(email: String, auth_scheme: AuthScheme) -> Self {
        Self {
            email,
            auth_scheme,
            aliases: HashMap::new(),
            unopened: Vec::new(),
            rotation: None,
        }
    }

    fn alias_signer(&self, alias_id: &str) -> Option<String> {
        self.aliases
            .iter()
            .filter(|(_, alias)| alias.alias_id == alias_id)
            .max_by_key(|(_, alias)| alias.key_version)
            .map(|(fingerprint, _)| fingerprint.clone())
    }
}

pub struct Keystore {
    accounts: Mutex<HashMap<String, Account>>,
    pending: Mutex<HashMap<String, PendingLogin>>,
    pending_reg: Mutex<HashMap<String, PendingReg>>,
    ksf_permit: Semaphore,
    events: broadcast::Sender<Broadcast>,
}

impl Default for Keystore {
    fn default() -> Self {
        Self::new()
    }
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(work).await.ok()
}

fn opaque_abandon(operation_id: &str) {
    let _ = trust::<Empty>(
        Family::Opaque,
        &json!({"op": "abandon", "operationId": operation_id}),
    );
}

fn lock_session(account_id: &str) {
    let _ = trust::<Empty>(
        Family::Session,
        &json!({"op": "lock", "accountId": account_id}),
    );
}

fn start_registration(password: &str) -> Result<Started, TrustError> {
    trust(
        Family::Opaque,
        &PasswordRequest {
            op: "startRegistration",
            password,
        },
    )
}

fn finish_registration(
    operation_id: &str,
    account_id: &str,
    registration_response: &str,
    recovery: bool,
    reuse: bool,
) -> Result<Registered, TrustError> {
    trust(
        Family::Opaque,
        &json!({
            "op": "finishRegistration",
            "operationId": operation_id,
            "accountId": account_id,
            "message": registration_response,
            "recovery": recovery,
            "reuseAccountId": if reuse { account_id } else { "" },
        }),
    )
}

fn account_keys(account_id: &str) -> Option<KeysState> {
    trust(
        Family::Session,
        &json!({"op": "accountKeys", "accountId": account_id}),
    )
    .ok()
}

fn fingerprint_of(armored: &str) -> Option<String> {
    trust::<Inspected>(Family::Keys, &json!({"op": "inspect", "armored": armored}))
        .ok()
        .map(|k| k.fingerprint)
}

fn hex_decode(value: &str) -> Vec<u8> {
    hex::decode(value).unwrap_or_default()
}

fn rewrap_response(registered: Registered) -> RewrapResponse {
    RewrapResponse::Ok {
        ok: true,
        opaque_record: registered.opaque_record,
        wrapped_master_key: registered.wrapped_master_key,
        master_key_id: registered.master_key_id,
        opaque_params_version: u32::try_from(registered.opaque_params_version).unwrap_or(1),
    }
}

struct LoginKeys {
    opened: SavedKeys,
    promoted: Option<SavedKeys>,
}

fn login_keys(login_encrypted: &str, stored: &[StoredAccountKey]) -> LoginKeys {
    let login_only = SavedKeys {
        encrypted_private_key: login_encrypted.to_owned(),
        key_version: 0,
        key_anchored: false,
        history_keys: Vec::new(),
    };
    let unseeded = || LoginKeys {
        opened: login_only.clone(),
        promoted: None,
    };
    let Some(login_fp) = fingerprint_of(login_encrypted) else {
        return unseeded();
    };
    let mut versions = HashSet::new();
    let mut seen = HashSet::from([login_fp.clone()]);
    let mut login = None;
    let mut others: Vec<(i64, bool, String)> = Vec::new();
    for entry in stored {
        let fingerprint_hex = entry.fingerprint_hex.to_lowercase();
        if entry.key_version < 1 || !versions.insert(entry.key_version) {
            return unseeded();
        }
        if fingerprint_hex == login_fp {
            login = Some(entry);
            continue;
        }
        let Some(encrypted) = entry.encrypted_private_key_armored.clone() else {
            continue;
        };
        if !seen.insert(fingerprint_hex) {
            return unseeded();
        }
        others.push((entry.key_version, entry.fixed, encrypted));
    }
    others.sort_by_key(|(version, _, _)| *version);
    let history = |entries: &[(i64, bool, String)]| {
        entries
            .iter()
            .map(|(key_version, _, encrypted_private_key)| HistoryKey {
                key_version: *key_version,
                encrypted_private_key: encrypted_private_key.clone(),
            })
            .collect::<Vec<_>>()
    };

    let (login_version, login_fixed) = login.map_or((0, false), |l| (l.key_version, l.fixed));
    let opened = SavedKeys {
        encrypted_private_key: login_encrypted.to_owned(),
        key_version: login_version,
        key_anchored: login_fixed,
        history_keys: history(&others),
    };
    let Some((best_version, best_fixed, best_encrypted)) = others
        .last()
        .filter(|(version, _, _)| *version > login_version)
        .cloned()
    else {
        return LoginKeys {
            opened,
            promoted: None,
        };
    };
    let mut rest = others[..others.len() - 1].to_vec();
    if login.is_some() {
        rest.push((login_version, login_fixed, login_encrypted.to_owned()));
    }
    LoginKeys {
        opened,
        promoted: Some(SavedKeys {
            encrypted_private_key: best_encrypted,
            key_version: best_version,
            key_anchored: best_fixed,
            history_keys: history(&rest),
        }),
    }
}

fn reopen(account_id: &str, keys: &SavedKeys) -> bool {
    trust::<Empty>(
        Family::Session,
        &json!({
            "op": "reopen",
            "accountId": account_id,
            "encryptedPrivateKey": keys.encrypted_private_key,
            "keyVersion": keys.key_version,
            "keyAnchored": keys.key_anchored,
            "historyKeys": keys.history_keys,
        }),
    )
    .is_ok()
}

fn verdict(v: Verdict) -> SignatureVerdict {
    SignatureVerdict {
        state: match v.state.as_str() {
            "valid" => "valid",
            "invalid" => "invalid",
            "none" => "none",
            _ => "unknown_key",
        },
        key_fingerprint_hex: v.fingerprint,
        signed_at_millis: v.signed_at_millis,
    }
}

fn decrypt_failure(code: Option<&str>) -> &'static str {
    match code {
        Some("no_matching_key") => "no_matching_key",
        _ => "invalid_ciphertext",
    }
}

impl Keystore {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            accounts: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            pending_reg: Mutex::new(HashMap::new()),
            ksf_permit: Semaphore::new(1),
            events,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Broadcast> {
        self.events.subscribe()
    }

    fn emit(&self, event: Broadcast) {
        let _ = self.events.send(event);
    }

    fn unlocked(&self, account_id: &str) -> bool {
        self.accounts
            .lock()
            .expect("keystore accounts")
            .contains_key(account_id)
    }

    fn email_of(&self, account_id: &str) -> Option<String> {
        self.accounts
            .lock()
            .expect("keystore accounts")
            .get(account_id)
            .map(|a| a.email.clone())
    }

    fn adopt(&self, account_id: &str, email: String, auth_scheme: AuthScheme) {
        self.accounts.lock().expect("keystore accounts").insert(
            account_id.to_owned(),
            Account::new(email.clone(), auth_scheme),
        );
        self.emit(Broadcast::VaultChanged {
            account_id: account_id.to_owned(),
            email,
        });
    }

    pub fn status(&self) -> StatusResponse {
        let accounts = self.accounts.lock().expect("keystore accounts");
        StatusResponse {
            accounts: accounts
                .iter()
                .map(|(account_id, a)| AccountStatus {
                    account_id: account_id.clone(),
                    email: a.email.clone(),
                    unlocked: true,
                    has_persistent: false,
                    auth_scheme: a.auth_scheme,
                })
                .collect(),
        }
    }

    pub async fn opaque_start_auth(
        &self,
        args: OpaqueStartAuthArgs,
    ) -> Result<OpaqueStartAuthResponse, KeystoreError> {
        let password = Zeroizing::new(args.password);
        let start: Started = trust(
            Family::Opaque,
            &PasswordRequest {
                op: "startLogin",
                password: &password,
            },
        )
        .map_err(|_| KeystoreError::Protocol)?;
        self.pending.lock().expect("keystore pending").insert(
            start.operation_id.clone(),
            PendingLogin {
                email: args.email,
                recovery: args.recovery,
                account_id: None,
                recovered: false,
                reset_operation: None,
            },
        );
        Ok(OpaqueStartAuthResponse {
            operation_id: start.operation_id,
            ke1: start.message,
        })
    }

    pub async fn opaque_finish_auth(&self, args: OpaqueFinishAuthArgs) -> OpaqueFinishAuthResponse {
        let op = self
            .pending
            .lock()
            .expect("keystore pending")
            .remove(&args.operation_id);
        let Some(mut op) = op else {
            return OpaqueFinishAuthResponse::err("no_pending_operation");
        };
        if op.account_id.is_some() {
            opaque_abandon(&args.operation_id);
            return OpaqueFinishAuthResponse::err("no_pending_operation");
        }
        let recovery = args.recovery || op.recovery;
        let request = json!({
            "op": "finishLogin",
            "operationId": args.operation_id,
            "accountId": args.account_id,
            "message": args.ke2,
            "recovery": recovery,
        });

        let _permit = self.ksf_permit.acquire().await.expect("ksf permit");
        let finished = blocking(move || trust::<LoginFinished>(Family::Opaque, &request)).await;
        drop(_permit);

        let Some(Ok(finished)) = finished else {
            return OpaqueFinishAuthResponse::err("invalid_credentials");
        };
        op.recovery = recovery;
        op.account_id = Some(args.account_id);
        self.pending
            .lock()
            .expect("keystore pending")
            .insert(args.operation_id, op);
        OpaqueFinishAuthResponse::ok(finished.ke3)
    }

    pub fn opaque_complete_login_unlock(
        &self,
        args: OpaqueCompleteLoginUnlockArgs,
        stored: Vec<StoredAccountKey>,
    ) -> OpaqueCompleteLoginUnlockResponse {
        let op = self
            .pending
            .lock()
            .expect("keystore pending")
            .remove(&args.operation_id);
        let Some(op) = op else {
            return OpaqueCompleteLoginUnlockResponse::err("no_pending_operation");
        };
        if op.recovery || op.account_id.as_deref() != Some(args.account_id.as_str()) {
            opaque_abandon(&args.operation_id);
            return OpaqueCompleteLoginUnlockResponse::err("no_pending_operation");
        }

        let keys = if args.server_auth_scheme == AuthScheme::OpaqueV1 {
            login_keys(&args.encrypted_private_key, &stored)
        } else {
            login_keys(&args.encrypted_private_key, &[])
        };
        let unlocked = trust::<Empty>(
            Family::Opaque,
            &json!({
                "op": "completeLoginUnlock",
                "operationId": args.operation_id,
                "accountId": args.account_id,
                "wrappedMasterKey": args.wrapped_master_key,
                "masterKeyId": args.master_key_id,
                "encryptedPrivateKey": keys.opened.encrypted_private_key,
                "keyVersion": keys.opened.key_version,
                "keyAnchored": keys.opened.key_anchored,
                "historyKeys": keys.opened.history_keys,
            }),
        );
        if let Err(err) = unlocked {
            return OpaqueCompleteLoginUnlockResponse::err(
                if err.message().contains(MASTER_KEY_MISMATCH) {
                    "master_key_mismatch"
                } else {
                    "unwrap_failed"
                },
            );
        }

        if let Some(promoted) = &keys.promoted {
            reopen(&args.account_id, promoted);
        }
        let email = op.email.unwrap_or_default();
        self.adopt(&args.account_id, email.clone(), args.server_auth_scheme);
        OpaqueCompleteLoginUnlockResponse::ok(args.account_id, email)
    }

    pub fn migration_finalized(
        &self,
        account_id: &str,
        stored: Vec<StoredAccountKey>,
    ) -> PlainOkResponse {
        let mut accounts = self.accounts.lock().expect("keystore accounts");
        let Some(account) = accounts.get_mut(account_id) else {
            return PlainOkResponse::err("locked");
        };
        if account.auth_scheme != AuthScheme::SrpV1 {
            return PlainOkResponse::err("not_staged");
        }
        account.auth_scheme = AuthScheme::OpaqueV1;
        let email = account.email.clone();
        drop(accounts);

        if let Some(state) = account_keys(account_id) {
            let keys = login_keys(&state.current.encrypted_private_key, &stored);
            let seeded = keys.promoted.unwrap_or(keys.opened);
            reopen(account_id, &seeded);
        }

        self.emit(Broadcast::VaultChanged {
            account_id: account_id.to_owned(),
            email,
        });
        PlainOkResponse::ok()
    }

    pub fn try_restore_from_persistent(&self, account_id: &str) -> RestoreResponse {
        match self.email_of(account_id) {
            Some(email) => RestoreResponse {
                ok: true,
                account_id: Some(account_id.to_owned()),
                email: Some(email),
                reason: None,
            },
            None => RestoreResponse::failed("no_persistent"),
        }
    }

    pub fn stored_account_keys(&self, account_id: &str) -> Option<Vec<StoredAccountKey>> {
        let opaque = self
            .accounts
            .lock()
            .expect("keystore accounts")
            .get(account_id)
            .is_some_and(|a| a.auth_scheme == AuthScheme::OpaqueV1);
        if !opaque {
            return None;
        }
        let state = account_keys(account_id)?;
        let current = (state.current.key_version > 0).then(|| StoredAccountKey {
            key_version: state.current.key_version,
            fingerprint_hex: state.current.fingerprint.clone(),
            encrypted_private_key_armored: Some(state.current.encrypted_private_key.clone()),
            fixed: state.anchored,
        });
        Some(
            current
                .into_iter()
                .chain(state.history.iter().map(|k| StoredAccountKey {
                    key_version: k.key_version,
                    fingerprint_hex: k.fingerprint.clone(),
                    encrypted_private_key_armored: Some(k.encrypted_private_key.clone()),
                    fixed: state.anchored,
                }))
                .collect(),
        )
    }

    pub fn saved_keys(&self, account_id: &str) -> Option<SavedKeys> {
        if !self.unlocked(account_id) {
            return None;
        }
        let state = account_keys(account_id)?;
        Some(SavedKeys {
            encrypted_private_key: state.current.encrypted_private_key,
            key_version: state.current.key_version,
            key_anchored: state.anchored,
            history_keys: state
                .history
                .into_iter()
                .map(|k| HistoryKey {
                    key_version: k.key_version,
                    encrypted_private_key: k.encrypted_private_key,
                })
                .collect(),
        })
    }

    pub fn persist(&self, account_id: &str, server_half: &[u8]) -> Option<PersistedSession> {
        let email = self.email_of(account_id)?;
        let keys = self.saved_keys(account_id)?;
        let persisted: Persisted = trust(
            Family::Session,
            &json!({
                "op": "persist",
                "accountId": account_id,
                "serverHalf": b64().encode(server_half),
            }),
        )
        .ok()?;
        Some(PersistedSession {
            wrapped: persisted.wrapped,
            wrap_version: persisted.wrap_version,
            keys,
            email,
        })
    }

    pub fn restore(
        &self,
        account_id: &str,
        email: String,
        keys: &SavedKeys,
        wrapped: &WrappedSecret,
        wrap_version: i64,
        server_half: &[u8],
    ) -> bool {
        let restored = trust::<Empty>(
            Family::Session,
            &json!({
                "op": "restore",
                "accountId": account_id,
                "encryptedPrivateKey": keys.encrypted_private_key,
                "keyVersion": keys.key_version,
                "keyAnchored": keys.key_anchored,
                "historyKeys": keys.history_keys,
                "wrapped": wrapped,
                "wrapVersion": wrap_version,
                "serverHalf": b64().encode(server_half),
            }),
        );
        if restored.is_err() {
            return false;
        }
        self.adopt(account_id, email, AuthScheme::OpaqueV1);
        true
    }

    pub fn import_desktop_vault(
        &self,
        account_id: &str,
        vault_file: &str,
        local_half: &[u8],
        server_half: &[u8],
    ) -> Option<String> {
        let imported: ImportedVault = trust(
            Family::Session,
            &json!({
                "op": "importDesktopVault",
                "accountId": account_id,
                "vaultFile": vault_file,
                "localHalf": b64().encode(local_half),
                "serverHalf": b64().encode(server_half),
            }),
        )
        .ok()?;
        self.adopt(account_id, imported.email.clone(), AuthScheme::OpaqueV1);
        Some(imported.email)
    }

    pub fn clear(&self, account_id: &str) {
        self.accounts
            .lock()
            .expect("keystore accounts")
            .remove(account_id);
        lock_session(account_id);
        self.emit(Broadcast::Cleared {
            account_id: account_id.to_owned(),
        });
    }

    pub async fn opaque_start_registration(
        &self,
        args: OpaqueStartRegistrationArgs,
    ) -> Result<OpaqueStartRegistrationResponse, KeystoreError> {
        let password = Zeroizing::new(args.password);
        let start = start_registration(&password).map_err(|_| KeystoreError::Protocol)?;
        self.pending_reg
            .lock()
            .expect("keystore pending registration")
            .insert(
                start.operation_id.clone(),
                PendingReg {
                    kind: RegKind::Register,
                    email: args.email,
                    account_id: None,
                    finished: false,
                },
            );
        Ok(OpaqueStartRegistrationResponse {
            operation_id: start.operation_id,
            registration_request: start.message,
        })
    }

    fn take_reg(&self, operation_id: &str, kind: RegKind) -> Option<PendingReg> {
        let mut pending = self
            .pending_reg
            .lock()
            .expect("keystore pending registration");
        if pending.get(operation_id).is_some_and(|op| op.kind == kind) {
            pending.remove(operation_id)
        } else {
            None
        }
    }

    pub async fn opaque_finish_registration(
        &self,
        args: OpaqueFinishRegistrationArgs,
        clock_offset_ms: i64,
    ) -> OpaqueFinishRegistrationResponse {
        let failed = OpaqueFinishRegistrationResponse::Err {
            ok: false,
            code: "no_pending_operation",
        };
        let Some(mut op) = self.take_reg(&args.operation_id, RegKind::Register) else {
            return failed;
        };
        if op.finished {
            return failed;
        }
        let operation_id = args.operation_id.clone();
        let account_id = args.account_id.clone();
        let email = op.email.clone();
        let _permit = self.ksf_permit.acquire().await.expect("ksf permit");
        let finished = blocking(move || {
            let registered = finish_registration(
                &operation_id,
                &account_id,
                &args.registration_response,
                false,
                false,
            )?;
            let generated = trust::<GeneratedKey>(
                Family::Session,
                &json!({
                    "op": "generateAccountKey",
                    "accountId": account_id,
                    "email": email,
                    "serverClockOffsetMs": clock_offset_ms,
                }),
            );
            if generated.is_err() {
                lock_session(&account_id);
            }
            Ok::<_, TrustError>((registered, generated?))
        })
        .await;
        drop(_permit);

        let Some(Ok((registered, generated))) = finished else {
            return failed;
        };
        op.account_id = Some(args.account_id);
        op.finished = true;
        self.pending_reg
            .lock()
            .expect("keystore pending registration")
            .insert(args.operation_id, op);

        OpaqueFinishRegistrationResponse::Ok {
            ok: true,
            opaque_record: registered.opaque_record,
            wrapped_master_key: registered.wrapped_master_key,
            master_key_id: registered.master_key_id,
            opaque_params_version: registered.opaque_params_version,
            public_key: generated.public_key,
            encrypted_private_key: generated.encrypted_private_key,
            key_algorithm: KEY_ALGORITHM,
        }
    }

    pub fn opaque_finalize_register(
        &self,
        args: OpaqueFinalizeRegisterArgs,
    ) -> OpaqueFinalizeRegisterResponse {
        let Some(op) = self.take_reg(&args.operation_id, RegKind::Register) else {
            return OpaqueFinalizeRegisterResponse::Err {
                ok: false,
                code: "no_pending_operation",
            };
        };
        if !op.finished {
            return OpaqueFinalizeRegisterResponse::Err {
                ok: false,
                code: "unwrap_failed",
            };
        }
        if op.account_id.as_deref() != Some(args.account_id.as_str()) {
            if let Some(account_id) = op.account_id.as_deref() {
                lock_session(account_id);
            }
            return OpaqueFinalizeRegisterResponse::Err {
                ok: false,
                code: "no_pending_operation",
            };
        }
        self.adopt(&args.account_id, op.email.clone(), AuthScheme::OpaqueV1);
        OpaqueFinalizeRegisterResponse::Ok {
            ok: true,
            account_id: args.account_id,
            email: op.email,
        }
    }

    pub async fn opaque_recovery_setup_start(
        &self,
        args: OpaqueRecoverySetupStartArgs,
    ) -> OpaqueRecoverySetupStartResponse {
        let locked = OpaqueRecoverySetupStartResponse::Err {
            ok: false,
            code: "locked",
        };
        if !self.unlocked(&args.account_id) {
            return locked;
        }
        let Ok(phrase) = trust::<Phrase>(Family::Session, &json!({"op": "recoveryPhrase"})) else {
            return locked;
        };
        let phrase = Zeroizing::new(phrase.phrase);
        let Ok(start) = start_registration(&phrase) else {
            return locked;
        };
        self.pending_reg
            .lock()
            .expect("keystore pending registration")
            .insert(
                start.operation_id.clone(),
                PendingReg {
                    kind: RegKind::RecoverySetup,
                    email: String::new(),
                    account_id: Some(args.account_id),
                    finished: false,
                },
            );
        OpaqueRecoverySetupStartResponse::Ok {
            ok: true,
            operation_id: start.operation_id,
            phrase: phrase.to_string(),
            registration_request: start.message,
        }
    }

    pub async fn opaque_recovery_setup_finish(
        &self,
        args: OpaqueRecoverySetupFinishArgs,
    ) -> RewrapResponse {
        if !self.unlocked(&args.account_id) {
            return RewrapResponse::err("locked");
        }
        let Some(op) = self.take_reg(&args.operation_id, RegKind::RecoverySetup) else {
            return RewrapResponse::err("no_pending_operation");
        };
        if op.account_id.as_deref() != Some(args.account_id.as_str()) {
            opaque_abandon(&args.operation_id);
            return RewrapResponse::err("no_pending_operation");
        }

        let _permit = self.ksf_permit.acquire().await.expect("ksf permit");
        let finished = blocking(move || {
            finish_registration(
                &args.operation_id,
                &args.account_id,
                &args.registration_response,
                true,
                true,
            )
        })
        .await;
        drop(_permit);

        match finished {
            Some(Ok(registered)) => rewrap_response(registered),
            _ => RewrapResponse::err("no_pending_operation"),
        }
    }

    pub fn opaque_complete_recovery_unlock(
        &self,
        args: OpaqueCompleteRecoveryUnlockArgs,
    ) -> PlainOkResponse {
        let mut pending = self.pending.lock().expect("keystore pending");
        let Some(op) = pending.get_mut(&args.operation_id) else {
            return PlainOkResponse::err("no_pending_operation");
        };
        let Some(account_id) = op.account_id.clone() else {
            return PlainOkResponse::err("no_pending_operation");
        };
        if !op.recovery || op.recovered {
            return PlainOkResponse::err("no_pending_operation");
        }
        let unwrapped = trust::<Empty>(
            Family::Opaque,
            &json!({
                "op": "unwrapRecovery",
                "operationId": args.operation_id,
                "accountId": account_id,
                "wrappedMasterKey": args.wrapped_master_key,
            }),
        );
        if unwrapped.is_err() {
            pending.remove(&args.operation_id);
            return PlainOkResponse::err("invalid_credentials");
        }
        op.recovered = true;
        PlainOkResponse::ok()
    }

    pub async fn opaque_prepare_credential_reset(
        &self,
        args: OpaquePrepareCredentialResetArgs,
    ) -> RegistrationRequestResponse {
        let failed = RegistrationRequestResponse::Err {
            ok: false,
            code: "no_pending_reset",
        };
        let ready = self
            .pending
            .lock()
            .expect("keystore pending")
            .get(&args.operation_id)
            .is_some_and(|op| op.recovered);
        if !ready {
            return failed;
        }
        let new_password = Zeroizing::new(args.new_password);
        let Ok(start) = start_registration(&new_password) else {
            return failed;
        };
        let mut pending = self.pending.lock().expect("keystore pending");
        let Some(op) = pending.get_mut(&args.operation_id) else {
            opaque_abandon(&start.operation_id);
            return failed;
        };
        if let Some(previous) = op.reset_operation.replace(start.operation_id) {
            opaque_abandon(&previous);
        }
        RegistrationRequestResponse::Ok {
            ok: true,
            registration_request: start.message,
        }
    }

    pub async fn opaque_finish_credential_reset(
        &self,
        args: OpaqueFinishCredentialResetArgs,
    ) -> RewrapResponse {
        let op = self
            .pending
            .lock()
            .expect("keystore pending")
            .remove(&args.operation_id);
        let Some(op) = op else {
            return RewrapResponse::err("no_pending_reset");
        };
        let (Some(reset_operation), Some(account_id), true) =
            (op.reset_operation, op.account_id, op.recovered)
        else {
            return RewrapResponse::err("no_pending_reset");
        };

        let account = account_id.clone();
        let _permit = self.ksf_permit.acquire().await.expect("ksf permit");
        let finished = blocking(move || {
            finish_registration(
                &reset_operation,
                &account,
                &args.registration_response,
                false,
                true,
            )
        })
        .await;
        drop(_permit);

        if !self.unlocked(&account_id) {
            lock_session(&account_id);
        }
        match finished {
            Some(Ok(registered)) => rewrap_response(registered),
            _ => RewrapResponse::err("no_pending_reset"),
        }
    }

    pub async fn opaque_password_change_start(
        &self,
        args: OpaquePasswordChangeStartArgs,
    ) -> OperationStartResponse {
        let locked = OperationStartResponse::Err {
            ok: false,
            code: "locked",
        };
        if !self.unlocked(&args.account_id) {
            return locked;
        }
        let new_password = Zeroizing::new(args.new_password);
        let Ok(start) = start_registration(&new_password) else {
            return locked;
        };
        self.pending_reg
            .lock()
            .expect("keystore pending registration")
            .insert(
                start.operation_id.clone(),
                PendingReg {
                    kind: RegKind::PasswordChange,
                    email: String::new(),
                    account_id: Some(args.account_id),
                    finished: false,
                },
            );
        OperationStartResponse::Ok {
            ok: true,
            operation_id: start.operation_id,
            registration_request: start.message,
        }
    }

    pub async fn opaque_password_change_finish(
        &self,
        args: OpaquePasswordChangeFinishArgs,
    ) -> RewrapResponse {
        if !self.unlocked(&args.account_id) {
            return RewrapResponse::err("locked");
        }
        let Some(mut op) = self.take_reg(&args.operation_id, RegKind::PasswordChange) else {
            return RewrapResponse::err("no_pending_operation");
        };
        if op.finished || op.account_id.as_deref() != Some(args.account_id.as_str()) {
            opaque_abandon(&args.operation_id);
            return RewrapResponse::err("no_pending_operation");
        }

        let operation_id = args.operation_id.clone();
        let account_id = args.account_id.clone();
        let _permit = self.ksf_permit.acquire().await.expect("ksf permit");
        let finished = blocking(move || {
            finish_registration(
                &operation_id,
                &account_id,
                &args.registration_response,
                false,
                true,
            )
        })
        .await;
        drop(_permit);

        let Some(Ok(registered)) = finished else {
            return RewrapResponse::err("no_pending_operation");
        };
        op.finished = true;
        self.pending_reg
            .lock()
            .expect("keystore pending registration")
            .insert(args.operation_id, op);
        rewrap_response(registered)
    }

    pub fn opaque_password_change_commit(
        &self,
        args: OpaquePasswordChangeCommitArgs,
    ) -> OpaquePasswordChangeCommitResponse {
        let op = self.take_reg(&args.operation_id, RegKind::PasswordChange);
        if !op.is_some_and(|op| op.finished) {
            return OpaquePasswordChangeCommitResponse::Err {
                ok: false,
                code: "no_pending_operation",
            };
        }
        let Some(email) = self.email_of(&args.account_id) else {
            return OpaquePasswordChangeCommitResponse::Err {
                ok: false,
                code: "locked",
            };
        };
        self.emit(Broadcast::VaultChanged {
            account_id: args.account_id,
            email,
        });
        OpaquePasswordChangeCommitResponse::Ok {
            ok: true,
            persisted: false,
        }
    }

    pub fn create_alias_key(
        &self,
        args: CreateAliasKeyArgs,
        clock_offset_ms: i64,
    ) -> CreateAliasKeyResponse {
        if args.recipients.is_empty() {
            return CreateAliasKeyResponse::err("no_recipients");
        }
        if !self.unlocked(&args.account_id) {
            return CreateAliasKeyResponse::err("locked");
        }
        let mut fingerprints = Vec::with_capacity(args.recipients.len());
        for recipient in &args.recipients {
            match fingerprint_of(&recipient.public_key_armored) {
                Some(fingerprint) => fingerprints.push(fingerprint),
                None => return CreateAliasKeyResponse::err("invalid_recipient_key"),
            }
        }
        let recipients: Vec<_> = args
            .recipients
            .iter()
            .map(|r| json!({"accountId": r.account_id, "publicKeyArmored": r.public_key_armored}))
            .collect();
        let generated = trust::<GeneratedAlias>(
            Family::Session,
            &json!({
                "op": "generateAlias",
                "accountId": args.account_id,
                "displayName": args.display_name,
                "email": args.email,
                "recipients": recipients,
                "serverClockOffsetMs": clock_offset_ms,
            }),
        );
        let mut generated = match generated {
            Ok(generated) => generated,
            Err(_) if !self.unlocked(&args.account_id) => {
                return CreateAliasKeyResponse::err("locked");
            }
            Err(_) => return CreateAliasKeyResponse::err("invalid_recipient_key"),
        };
        let mut grants = Vec::with_capacity(args.recipients.len());
        for (recipient, fingerprint) in args.recipients.iter().zip(fingerprints) {
            let Some(wrapped) = generated.grants.get(&recipient.account_id).cloned() else {
                return CreateAliasKeyResponse::err("invalid_recipient_key");
            };
            grants.push(CreatedAliasKeyGrant {
                account_id: recipient.account_id.clone(),
                wrapped_private_key_armored: wrapped,
                member_key_fingerprint_hex: fingerprint,
            });
        }
        generated.grants.clear();
        CreateAliasKeyResponse::Ok {
            ok: true,
            public_key_armored: generated.public_key_armored,
            key_fingerprint_hex: generated.fingerprint_hex,
            grants,
        }
    }

    pub fn commit_reformatted_key(&self, args: CommitReformattedKeyArgs) -> PlainOkResponse {
        if !self.unlocked(&args.account_id) {
            return PlainOkResponse::err("locked");
        }
        PlainOkResponse::err("invalid")
    }

    pub fn abandon(&self, operation_id: &str) {
        let reg = self
            .pending_reg
            .lock()
            .expect("keystore pending registration")
            .remove(operation_id);
        if let Some(reg) = reg
            && reg.kind == RegKind::Register
            && reg.finished
            && let Some(account_id) = reg.account_id
            && !self.unlocked(&account_id)
        {
            lock_session(&account_id);
        }
        let login = self
            .pending
            .lock()
            .expect("keystore pending")
            .remove(operation_id);
        if let Some(login) = login {
            self.drop_login(login);
        }
        opaque_abandon(operation_id);
    }

    fn drop_login(&self, login: PendingLogin) {
        if let Some(reset) = login.reset_operation {
            opaque_abandon(&reset);
        }
        if login.recovered
            && let Some(account_id) = login.account_id
            && !self.unlocked(&account_id)
        {
            lock_session(&account_id);
        }
    }

    pub fn discard_recovery(&self) {
        let dropped: Vec<(String, PendingLogin)> = {
            let mut pending = self.pending.lock().expect("keystore pending");
            let ids: Vec<String> = pending
                .iter()
                .filter(|(_, op)| op.recovery)
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| pending.remove(&id).map(|op| (id, op)))
                .collect()
        };
        for (id, login) in dropped {
            opaque_abandon(&id);
            self.drop_login(login);
        }
        self.drop_regs(RegKind::RecoverySetup);
    }

    fn drop_regs(&self, kind: RegKind) {
        let ids: Vec<String> = {
            let mut pending = self
                .pending_reg
                .lock()
                .expect("keystore pending registration");
            let ids: Vec<String> = pending
                .iter()
                .filter(|(_, op)| op.kind == kind)
                .map(|(id, _)| id.clone())
                .collect();
            for id in &ids {
                pending.remove(id);
            }
            ids
        };
        for id in ids {
            opaque_abandon(&id);
        }
    }

    pub fn abandon_password_change(&self) {
        self.drop_regs(RegKind::PasswordChange);
    }

    pub fn lock(&self, account_id: &str) {
        self.accounts
            .lock()
            .expect("keystore accounts")
            .remove(account_id);
        lock_session(account_id);
        self.emit(Broadcast::Locked {
            account_id: account_id.to_owned(),
        });
    }

    pub fn clear_all(&self) {
        self.accounts.lock().expect("keystore accounts").clear();
        let logins: Vec<String> = self
            .pending
            .lock()
            .expect("keystore pending")
            .drain()
            .map(|(id, _)| id)
            .collect();
        for id in logins {
            opaque_abandon(&id);
        }
        let _ = trust::<Empty>(Family::Session, &json!({"op": "lockAll"}));
        self.emit(Broadcast::ClearedAll);
    }

    fn decrypt_one(
        &self,
        account_id: &str,
        ciphertext: &[u8],
        verification_keys: &[String],
    ) -> Result<(Vec<u8>, Option<SignatureVerdict>), &'static str> {
        if !self.unlocked(account_id) {
            return Err("locked");
        }
        let item = if verification_keys.is_empty() {
            json!({"ciphertext": b64().encode(ciphertext)})
        } else {
            json!({"ciphertext": b64().encode(ciphertext), "senderKeys": verification_keys})
        };
        let decrypted = trust::<Decrypted>(
            Family::Mail,
            &json!({"op": "decrypt", "accountId": account_id, "items": [item]}),
        )
        .map_err(|_| "locked")?;
        let Some(item) = decrypted.items.into_iter().next() else {
            return Err("invalid_ciphertext");
        };
        let Some(data) = item.data else {
            return Err(decrypt_failure(item.error.as_deref()));
        };
        let plain = b64().decode(data).map_err(|_| "invalid_ciphertext")?;
        Ok((plain, item.signature.map(verdict)))
    }

    pub fn decrypt(&self, args: DecryptArgs) -> DecryptResponse {
        if !self.unlocked(&args.account_id) {
            return DecryptResponse::err("locked");
        }
        let ciphertext = match (&args.ciphertext_binary, &args.ciphertext_armored) {
            (Some(bytes), _) => bytes.clone(),
            (None, Some(armored)) => match b64().decode(armored) {
                Ok(b) => b,
                Err(_) => armored.as_bytes().to_vec(),
            },
            (None, None) => return DecryptResponse::err("invalid_ciphertext"),
        };
        let verification_keys = args.verification_keys_armored.unwrap_or_default();
        match self.decrypt_one(&args.account_id, &ciphertext, &verification_keys) {
            Ok((plain, signature)) => {
                if args.binary {
                    DecryptResponse::Binary {
                        ok: true,
                        plaintext_binary: plain,
                        signature,
                    }
                } else {
                    match String::from_utf8(plain) {
                        Ok(text) => DecryptResponse::Text {
                            ok: true,
                            plaintext: text,
                            signature,
                        },
                        Err(_) => DecryptResponse::err("invalid_ciphertext"),
                    }
                }
            }
            Err(code) => DecryptResponse::err(code),
        }
    }

    pub fn load_alias_keys(&self, args: LoadAliasKeysArgs) -> LoadAliasKeysResponse {
        let locked = LoadAliasKeysResponse::Err {
            ok: false,
            code: "locked",
        };
        let mut accounts = self.accounts.lock().expect("keystore accounts");
        let Some(account) = accounts.get_mut(&args.account_id) else {
            return locked;
        };

        let mut loaded = Vec::new();
        let mut fresh = Vec::new();
        for grant in args.grants {
            let want = grant.alias_key_fingerprint_hex.to_lowercase();
            if account.aliases.contains_key(&want) {
                loaded.push(want);
            } else {
                fresh.push(grant);
            }
        }
        let Some(accepted) = sync_aliases(&args.account_id, account, &fresh) else {
            return locked;
        };

        let mut failed = Vec::new();
        for (grant, accepted) in fresh.into_iter().zip(accepted) {
            let want = grant.alias_key_fingerprint_hex.to_lowercase();
            account
                .unopened
                .retain(|g| g.alias_key_fingerprint_hex.to_lowercase() != want);
            failed_or_loaded(account, grant, accepted, &mut loaded, &mut failed);
        }
        LoadAliasKeysResponse::Ok {
            ok: true,
            loaded,
            failed,
        }
    }

    pub fn unload_alias_keys(&self, account_id: &str) {
        let mut accounts = self.accounts.lock().expect("keystore accounts");
        if let Some(account) = accounts.get_mut(account_id) {
            account.aliases.clear();
            account.unopened.clear();
            let _ = sync_aliases(account_id, account, &[]);
        }
    }

    fn retry_unopened_aliases(&self, account_id: &str) {
        let mut accounts = self.accounts.lock().expect("keystore accounts");
        let Some(account) = accounts.get_mut(account_id) else {
            return;
        };
        if account.unopened.is_empty() {
            return;
        }
        let retry = std::mem::take(&mut account.unopened);
        let Some(accepted) = sync_aliases(account_id, account, &retry) else {
            account.unopened = retry;
            return;
        };
        let (mut loaded, mut failed) = (Vec::new(), Vec::new());
        for (grant, accepted) in retry.into_iter().zip(accepted) {
            failed_or_loaded(account, grant, accepted, &mut loaded, &mut failed);
        }
    }

    pub fn decrypt_bytes(
        &self,
        account_id: &str,
        ciphertext: &[u8],
        _key_fingerprint_hex: Option<&str>,
    ) -> Result<Vec<u8>, &'static str> {
        self.decrypt_one(account_id, ciphertext, &[])
            .map(|(plain, _)| plain)
    }

    fn signer_for(
        &self,
        account_id: &str,
        sign: bool,
        alias_id: Option<&str>,
    ) -> Result<Option<String>, ()> {
        let accounts = self.accounts.lock().expect("keystore accounts");
        let account = accounts.get(account_id).ok_or(())?;
        if !sign {
            return Ok(None);
        }
        match alias_id {
            None => Ok(Some(String::new())),
            Some(alias_id) => account.alias_signer(alias_id).map(Some).ok_or(()),
        }
    }

    fn encrypt_with(
        &self,
        account_id: &str,
        recipients: &[String],
        plaintext: &[u8],
        sign: bool,
        alias_id: Option<&str>,
        armor: bool,
    ) -> Result<Encrypted, &'static str> {
        let signer = self
            .signer_for(account_id, sign, alias_id)
            .map_err(|_| "locked")?;
        trust::<Encrypted>(
            Family::Mail,
            &json!({
                "op": "encrypt",
                "accountId": account_id,
                "plaintext": b64().encode(plaintext),
                "recipients": recipients,
                "signWith": signer.clone().unwrap_or_default(),
                "unsigned": signer.is_none(),
                "armor": armor,
            }),
        )
        .map_err(|_| {
            if self.unlocked(account_id) {
                "invalid_recipient_key"
            } else {
                "locked"
            }
        })
    }

    pub fn encrypt(&self, args: EncryptArgs) -> EncryptResponse {
        let encrypted = self.encrypt_with(
            &args.account_id,
            std::slice::from_ref(&args.recipient_public_key_armored),
            &args.plaintext,
            args.sign_with_vault_key,
            args.alias_id.as_deref(),
            false,
        );
        match encrypted.map(|e| e.data.and_then(|d| b64().decode(d).ok())) {
            Ok(Some(ciphertext)) => EncryptResponse::Ok {
                ok: true,
                ciphertext,
            },
            Ok(None) => EncryptResponse::Err {
                ok: false,
                code: "invalid_recipient_key",
            },
            Err(code) => EncryptResponse::Err { ok: false, code },
        }
    }

    pub fn encrypt_to_keys(&self, args: EncryptToKeysArgs) -> EncryptToKeysResponse {
        if !self.unlocked(&args.account_id) {
            return EncryptToKeysResponse::Err {
                ok: false,
                code: "locked",
            };
        }
        if args.recipient_public_keys_armored.is_empty() {
            return EncryptToKeysResponse::Err {
                ok: false,
                code: "no_recipients",
            };
        }
        let encrypted = self.encrypt_with(
            &args.account_id,
            &args.recipient_public_keys_armored,
            &args.plaintext,
            args.sign_with_vault_key,
            args.alias_id.as_deref(),
            true,
        );
        match encrypted.map(|e| e.armored) {
            Ok(Some(armored)) => EncryptToKeysResponse::Ok { ok: true, armored },
            Ok(None) => EncryptToKeysResponse::Err {
                ok: false,
                code: "invalid_recipient_key",
            },
            Err(code) => EncryptToKeysResponse::Err { ok: false, code },
        }
    }

    pub fn sign_detached(&self, args: SignDetachedArgs) -> SignDetachedResponse {
        if !self.unlocked(&args.account_id) {
            return SignDetachedResponse::Err {
                ok: false,
                code: "locked",
            };
        }
        let signed = trust::<SignedData>(
            Family::Mail,
            &json!({
                "op": "signDetached",
                "accountId": args.account_id,
                "data": b64().encode(&args.data),
            }),
        );
        match signed.ok().and_then(|s| {
            b64()
                .decode(s.signature)
                .ok()
                .map(|signature| (signature, s.fingerprint))
        }) {
            Some((signature, key_fingerprint_hex)) => SignDetachedResponse::Ok {
                ok: true,
                signature,
                key_fingerprint_hex,
            },
            None => SignDetachedResponse::Err {
                ok: false,
                code: "unknown",
            },
        }
    }

    pub fn get_public_key(&self, account_id: &str) -> GetPublicKeyResponse {
        let locked = GetPublicKeyResponse::Err {
            ok: false,
            code: "locked",
        };
        if !self.unlocked(account_id) {
            return locked;
        }
        match trust::<PublicKey>(
            Family::Session,
            &json!({"op": "publicKey", "accountId": account_id}),
        ) {
            Ok(key) => GetPublicKeyResponse::Ok {
                ok: true,
                public_key_armored: key.public_key,
                fingerprint: hex_decode(&key.fingerprint),
                key_version: (key.key_version > 0).then_some(key.key_version),
            },
            Err(_) => locked,
        }
    }

    pub fn load_account_keys(&self, args: LoadAccountKeysArgs) -> LoadAccountKeysResponse {
        match self
            .accounts
            .lock()
            .expect("keystore accounts")
            .get(&args.account_id)
        {
            None => return LoadAccountKeysResponse::err("locked"),
            Some(a) if a.auth_scheme != AuthScheme::OpaqueV1 => {
                return LoadAccountKeysResponse::err("not_opaque");
            }
            Some(_) => {}
        }
        let Some(before) = account_keys(&args.account_id) else {
            return LoadAccountKeysResponse::err("locked");
        };

        let inputs: Vec<_> = args
            .keys
            .iter()
            .map(|k| {
                json!({
                    "keyVersion": k.key_version,
                    "keyFingerprint": k.fingerprint_hex.to_lowercase(),
                    "encryptedPrivateKey": k.encrypted_private_key_armored,
                    "isCurrent": k.is_current,
                    "currentVersion": args.current_version,
                    "transitions": k.transitions.iter().map(|t| json!({
                        "previousKeyFingerprint": t.previous_key_fingerprint_hex.to_lowercase(),
                        "signature": b64().encode(&t.signature),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let state = match trust::<KeysState>(
            Family::Session,
            &json!({"op": "loadAccountKeys", "accountId": args.account_id, "accountKeys": inputs}),
        ) {
            Ok(state) => state,
            Err(err) if err.message().contains(KEY_LIST_INVALID) => {
                return LoadAccountKeysResponse::err("key_list_invalid");
            }
            Err(_) if !self.unlocked(&args.account_id) => {
                return LoadAccountKeysResponse::err("locked");
            }
            Err(_) => return LoadAccountKeysResponse::err("key_list_invalid"),
        };

        let added = std::iter::once(&state.current)
            .chain(&state.history)
            .any(|k| !before.holds(&k.fingerprint));
        let current_changed = state.current.fingerprint != before.current.fingerprint;
        let loaded = args
            .keys
            .iter()
            .map(|k| k.fingerprint_hex.to_lowercase())
            .filter(|fp| state.holds(fp))
            .collect();

        if let Some(account) = self
            .accounts
            .lock()
            .expect("keystore accounts")
            .get_mut(&args.account_id)
            && account
                .rotation
                .as_ref()
                .is_some_and(|r| state.holds(&r.fingerprint_hex))
        {
            account.rotation = None;
        }
        if added {
            self.retry_unopened_aliases(&args.account_id);
        }

        let event = Broadcast::AccountKeysChanged {
            account_id: args.account_id.clone(),
            fingerprints: state.fingerprints(),
            current_fingerprint_hex: state.current.fingerprint.clone(),
            key_version: state.current.key_version,
            current_changed,
            changed: state.changed || added,
        };
        let response = LoadAccountKeysResponse::Ok {
            ok: true,
            loaded,
            failed: state.rejected,
            unverified: state.unverified,
            current_fingerprint_hex: state.current.fingerprint,
            key_version: state.current.key_version,
            current_changed,
        };
        self.emit(event);
        response
    }

    pub fn prepare_key_rotation(
        &self,
        args: PrepareKeyRotationArgs,
        clock_offset_ms: i64,
    ) -> PrepareKeyRotationResponse {
        match self
            .accounts
            .lock()
            .expect("keystore accounts")
            .get(&args.account_id)
        {
            None => return PrepareKeyRotationResponse::err("locked"),
            Some(a) if a.auth_scheme != AuthScheme::OpaqueV1 => {
                return PrepareKeyRotationResponse::err("not_opaque");
            }
            Some(_) => {}
        }
        let Some(state) = account_keys(&args.account_id) else {
            return PrepareKeyRotationResponse::err("locked");
        };
        if state.current.key_version < 1 {
            return PrepareKeyRotationResponse::err("key_history_incomplete");
        }
        if args.expected_key_version != state.current.key_version {
            return PrepareKeyRotationResponse::err("stale_key_version");
        }
        if args.emails.iter().all(|e| e.trim().is_empty()) {
            return PrepareKeyRotationResponse::err("no_emails");
        }
        if !state.history_is_complete() {
            return PrepareKeyRotationResponse::err("key_history_incomplete");
        }

        let Ok(prepared) = trust::<PreparedRotation>(
            Family::Session,
            &json!({
                "op": "prepareRotation",
                "accountId": args.account_id,
                "emails": args.emails,
                "serverClockOffsetMs": clock_offset_ms,
            }),
        ) else {
            return PrepareKeyRotationResponse::err("unknown");
        };
        if prepared.expected_key_version != state.current.key_version {
            let _ = trust::<Empty>(
                Family::Session,
                &json!({"op": "abandonRotation", "accountId": args.account_id}),
            );
            return PrepareKeyRotationResponse::err("stale_key_version");
        }
        let mut transitions = Vec::with_capacity(prepared.transitions.len());
        for t in prepared.transitions {
            let Ok(signature) = b64().decode(t.signature) else {
                return PrepareKeyRotationResponse::err("unknown");
            };
            transitions.push(PreparedKeyTransition {
                previous_key_fingerprint_hex: t.previous_key_fingerprint,
                signature,
            });
        }
        let key_version = state.current.key_version + 1;

        let mut accounts = self.accounts.lock().expect("keystore accounts");
        let Some(account) = accounts.get_mut(&args.account_id) else {
            return PrepareKeyRotationResponse::err("locked");
        };
        account.rotation = Some(PendingRotation {
            fingerprint_hex: prepared.fingerprint.clone(),
            key_version,
            expires_at: Instant::now() + KEY_ROTATION_TTL,
        });

        PrepareKeyRotationResponse::Ok {
            ok: true,
            public_key_armored: prepared.public_key_armored,
            encrypted_private_key: prepared.encrypted_private_key,
            fingerprint_hex: prepared.fingerprint,
            key_version,
            transitions,
        }
    }

    pub fn commit_key_rotation(&self, args: CommitKeyRotationArgs) -> CommitKeyRotationResponse {
        let pending = {
            let mut accounts = self.accounts.lock().expect("keystore accounts");
            let Some(account) = accounts.get_mut(&args.account_id) else {
                return CommitKeyRotationResponse::err("locked");
            };
            let Some(pending) = account.rotation.take() else {
                return CommitKeyRotationResponse::err("no_pending_rotation");
            };
            if pending.fingerprint_hex != args.fingerprint_hex.to_lowercase() {
                account.rotation = Some(pending);
                return CommitKeyRotationResponse::err("no_pending_rotation");
            }
            pending
        };
        let expired = pending.expires_at <= Instant::now();
        let current = account_keys(&args.account_id).map(|s| s.current.key_version);
        if expired || current != Some(pending.key_version - 1) {
            let _ = trust::<Empty>(
                Family::Session,
                &json!({"op": "abandonRotation", "accountId": args.account_id}),
            );
            return CommitKeyRotationResponse::err("no_pending_rotation");
        }
        let Ok(state) = trust::<KeysState>(
            Family::Session,
            &json!({
                "op": "commitRotation",
                "accountId": args.account_id,
                "fingerprint": pending.fingerprint_hex,
                "keyVersion": pending.key_version,
            }),
        ) else {
            return CommitKeyRotationResponse::err("no_pending_rotation");
        };

        let Some(email) = self.email_of(&args.account_id) else {
            return CommitKeyRotationResponse::err("locked");
        };
        self.emit(Broadcast::AccountKeysChanged {
            account_id: args.account_id.clone(),
            fingerprints: state.fingerprints(),
            current_fingerprint_hex: state.current.fingerprint.clone(),
            key_version: state.current.key_version,
            current_changed: true,
            changed: true,
        });
        self.emit(Broadcast::VaultChanged {
            account_id: args.account_id,
            email,
        });
        CommitKeyRotationResponse::Ok {
            ok: true,
            key_version: state.current.key_version,
        }
    }

    pub fn list_account_keys(&self, account_id: &str) -> ListAccountKeysResponse {
        let locked = ListAccountKeysResponse::Err {
            ok: false,
            code: "locked",
        };
        if !self.unlocked(account_id) {
            return locked;
        }
        let Some(state) = account_keys(account_id) else {
            return locked;
        };
        let keys = std::iter::once(AccountKeySummary {
            fingerprint_hex: state.current.fingerprint.clone(),
            key_version: state.current.key_version,
            is_current: true,
        })
        .chain(state.history.iter().map(|k| AccountKeySummary {
            fingerprint_hex: k.fingerprint.clone(),
            key_version: k.key_version,
            is_current: false,
        }))
        .collect();
        ListAccountKeysResponse::Ok {
            ok: true,
            keys,
            key_version: state.current.key_version,
        }
    }

    pub fn abandon_key_rotation(&self, account_id: &str) {
        let held = self
            .accounts
            .lock()
            .expect("keystore accounts")
            .get_mut(account_id)
            .map(|a| a.rotation.take());
        if held.is_some() {
            let _ = trust::<Empty>(
                Family::Session,
                &json!({"op": "abandonRotation", "accountId": account_id}),
            );
        }
    }
}

fn sync_aliases(
    account_id: &str,
    account: &Account,
    fresh: &[AliasKeyGrantInput],
) -> Option<Vec<bool>> {
    let held: Vec<&AliasKeyGrantInput> = account.aliases.values().map(|a| &a.grant).collect();
    let grants: Vec<_> = held
        .iter()
        .copied()
        .chain(fresh)
        .map(|g| {
            json!({
                "fingerprintHex": g.alias_key_fingerprint_hex.to_lowercase(),
                "wrappedPrivateKeyArmored": g.wrapped_private_key_armored,
                "memberKeyFingerprintHex": g.member_key_fingerprint_hex.as_deref().unwrap_or_default().to_lowercase(),
            })
        })
        .collect();
    let accepted = trust::<Accepted>(
        Family::Session,
        &json!({"op": "loadAliases", "accountId": account_id, "grants": grants}),
    )
    .ok()?;
    accepted
        .accepted
        .get(held.len()..)
        .map(<[bool]>::to_vec)
        .filter(|tail| tail.len() == fresh.len())
}

fn failed_or_loaded(
    account: &mut Account,
    grant: AliasKeyGrantInput,
    accepted: bool,
    loaded: &mut Vec<String>,
    failed: &mut Vec<FailedGrant>,
) {
    let want = grant.alias_key_fingerprint_hex.to_lowercase();
    if accepted {
        loaded.push(want.clone());
        account.aliases.insert(
            want,
            LoadedAlias {
                alias_id: grant.alias_id.clone(),
                key_version: grant.key_version,
                grant,
            },
        );
        return;
    }
    failed.push(FailedGrant {
        alias_id: grant.alias_id.clone(),
        key_version: grant.key_version,
    });
    account.unopened.push(grant);
}

#[derive(Debug, thiserror::Error)]
pub enum KeystoreError {
    #[error("protocol failure")]
    Protocol,
}

impl serde::Serialize for KeystoreError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
