use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine as _;
use rand::RngCore;
use rand::rngs::OsRng;
use thelemail_crypto::amk::{
    Amk, derive_master_key_id, derive_pgp_passphrase, unwrap_master_key, wrap_master_key,
};
use thelemail_crypto::opaque::{
    LoginState, RegistrationState, finish_login, finish_registration, start_login,
    start_registration,
};
use thelemail_crypto::openpgp::{
    SignatureCheck, SignatureState, UnlockedKey, generate_account_key, generate_alias_key,
    public_key_fingerprint_hex,
};
use tokio::sync::{Semaphore, broadcast};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::protocol::*;

const CLIENT_IDENTITY_PREFIX: &str = "thelemail/auth/opaque/v1:";
const RECOVERY_INFIX: &str = "recovery:";
const SERVER_IDENTITY: &str = "thelemail.com";

fn client_identity(account_id: &str, recovery: bool) -> String {
    if recovery {
        format!("{CLIENT_IDENTITY_PREFIX}{RECOVERY_INFIX}{account_id}")
    } else {
        format!("{CLIENT_IDENTITY_PREFIX}{account_id}")
    }
}

fn b64_std() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

#[derive(Clone, Copy, PartialEq)]
enum RegKind {
    Register,
    RecoverySetup,
    PasswordChange,
}

struct PendingReg {
    kind: RegKind,
    state: Option<RegistrationState>,
    password: Zeroizing<String>,
    email: String,
    account_id: Option<String>,
    unlocked: Option<UnlockedKey>,
    public_key_armored: Option<String>,
    amk: Option<Amk>,
    wrapped_master_key: Option<String>,
    master_key_id: Option<String>,
}

struct PendingOp {
    state: Option<LoginState>,
    password: Zeroizing<String>,
    email: Option<String>,
    recovery: bool,
    account_id: Option<String>,
    export_key: Option<Zeroizing<Vec<u8>>>,
    amk: Option<Amk>,
    unlocked: Option<UnlockedKey>,
    reset_state: Option<RegistrationState>,
    new_password: Option<Zeroizing<String>>,
}

struct AliasKey {
    key: UnlockedKey,
    alias_id: String,
    key_version: i64,
}

struct Vault {
    account_id: String,
    email: String,
    auth_scheme: AuthScheme,
    key: UnlockedKey,
    public_key_armored: String,
    alias_keys: HashMap<String, AliasKey>,
    amk: Amk,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("account_id", &self.account_id)
            .finish_non_exhaustive()
    }
}

pub struct Keystore {
    vaults: Mutex<HashMap<String, Vault>>,
    pending: Mutex<HashMap<String, PendingOp>>,
    pending_reg: Mutex<HashMap<String, PendingReg>>,
    ksf_permit: Semaphore,
    events: broadcast::Sender<Broadcast>,
}

impl Default for Keystore {
    fn default() -> Self {
        Self::new()
    }
}

impl Keystore {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            vaults: Mutex::new(HashMap::new()),
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

    pub fn status(&self) -> StatusResponse {
        let vaults = self.vaults.lock().expect("keystore vaults");
        StatusResponse {
            accounts: vaults
                .values()
                .map(|v| AccountStatus {
                    account_id: v.account_id.clone(),
                    email: v.email.clone(),
                    unlocked: true,
                    has_persistent: false,
                    auth_scheme: v.auth_scheme,
                })
                .collect(),
        }
    }

    pub async fn opaque_start_auth(
        &self,
        args: OpaqueStartAuthArgs,
    ) -> Result<OpaqueStartAuthResponse, KeystoreError> {
        let start = start_login(&args.password).map_err(|_| KeystoreError::Protocol)?;
        let operation_id = Uuid::new_v4().to_string();
        let ke1 = b64_std().encode(&start.ke1);

        self.pending.lock().expect("keystore pending").insert(
            operation_id.clone(),
            PendingOp {
                state: Some(start.state),
                password: Zeroizing::new(args.password),
                email: args.email,
                recovery: args.recovery,
                account_id: None,
                export_key: None,
                amk: None,
                unlocked: None,
                reset_state: None,
                new_password: None,
            },
        );

        Ok(OpaqueStartAuthResponse { operation_id, ke1 })
    }

    pub async fn opaque_finish_auth(&self, args: OpaqueFinishAuthArgs) -> OpaqueFinishAuthResponse {
        let op = self
            .pending
            .lock()
            .expect("keystore pending")
            .remove(&args.operation_id);
        let Some(op) = op else {
            return OpaqueFinishAuthResponse::err("no_pending_operation");
        };

        let Ok(ke2) = b64_std().decode(&args.ke2) else {
            return OpaqueFinishAuthResponse::err("invalid_credentials");
        };

        let Some(state) = op.state else {
            return OpaqueFinishAuthResponse::err("no_pending_operation");
        };
        let recovery = args.recovery || op.recovery;
        let identity = client_identity(&args.account_id, recovery);

        let _permit = self.ksf_permit.acquire().await.expect("ksf permit");
        let finished = finish_login(state, &op.password, &ke2, &identity, SERVER_IDENTITY);
        drop(_permit);

        let Ok(finished) = finished else {
            return OpaqueFinishAuthResponse::err("invalid_credentials");
        };

        let ke3 = b64_std().encode(&finished.ke3);
        self.pending.lock().expect("keystore pending").insert(
            args.operation_id,
            PendingOp {
                state: None,
                password: op.password,
                email: op.email,
                recovery,
                account_id: Some(args.account_id),
                export_key: Some(Zeroizing::new(finished.export_key)),
                amk: None,
                unlocked: None,
                reset_state: None,
                new_password: None,
            },
        );

        OpaqueFinishAuthResponse::ok(ke3)
    }

    pub fn opaque_complete_login_unlock(
        &self,
        args: OpaqueCompleteLoginUnlockArgs,
    ) -> OpaqueCompleteLoginUnlockResponse {
        let op = self
            .pending
            .lock()
            .expect("keystore pending")
            .remove(&args.operation_id);
        let Some(op) = op else {
            return OpaqueCompleteLoginUnlockResponse::err("no_pending_operation");
        };
        if op.account_id.as_deref() != Some(args.account_id.as_str()) {
            return OpaqueCompleteLoginUnlockResponse::err("no_pending_operation");
        }
        let Some(export_key) = op.export_key else {
            return OpaqueCompleteLoginUnlockResponse::err("no_pending_operation");
        };

        let Ok(wrapped) = b64_std().decode(&args.wrapped_master_key) else {
            return OpaqueCompleteLoginUnlockResponse::err("unwrap_failed");
        };
        let Ok(amk) = unwrap_master_key(&export_key, &wrapped, false) else {
            return OpaqueCompleteLoginUnlockResponse::err("unwrap_failed");
        };

        let derived = b64_std().encode(derive_master_key_id(&amk));
        if !constant_time_eq(derived.as_bytes(), args.master_key_id.as_bytes()) {
            return OpaqueCompleteLoginUnlockResponse::err("master_key_mismatch");
        }

        let passphrase = derive_pgp_passphrase(&amk);
        let Ok(key) = UnlockedKey::open(&args.encrypted_private_key, passphrase.expose()) else {
            return OpaqueCompleteLoginUnlockResponse::err("unwrap_failed");
        };

        let email = op.email.unwrap_or_default();
        let public_key_armored = key.public_key_armored().unwrap_or_default();

        self.vaults.lock().expect("keystore vaults").insert(
            args.account_id.clone(),
            Vault {
                account_id: args.account_id.clone(),
                email: email.clone(),
                auth_scheme: args.server_auth_scheme,
                key,
                public_key_armored,
                alias_keys: HashMap::new(),
                amk,
            },
        );

        self.emit(Broadcast::VaultChanged {
            account_id: args.account_id.clone(),
            email: email.clone(),
        });
        OpaqueCompleteLoginUnlockResponse::ok(args.account_id, email)
    }

    pub fn try_restore_from_persistent(&self, account_id: &str) -> RestoreResponse {
        let vaults = self.vaults.lock().expect("keystore vaults");
        match vaults.get(account_id) {
            Some(v) => RestoreResponse {
                ok: true,
                account_id: Some(v.account_id.clone()),
                email: Some(v.email.clone()),
                reason: None,
            },
            None => RestoreResponse {
                ok: false,
                account_id: None,
                email: None,
                reason: Some("no_persistent"),
            },
        }
    }

    pub fn persistable(&self, account_id: &str) -> Option<PersistedVault> {
        let vaults = self.vaults.lock().expect("keystore vaults");
        let vault = vaults.get(account_id)?;
        Some(PersistedVault {
            email: vault.email.clone(),
            auth_scheme: vault.auth_scheme,
            secret_key_armored: vault.key.secret_key_armored().ok()?,
            amk: b64_std().encode(vault.amk.expose()),
        })
    }

    pub fn adopt_persisted(&self, account_id: &str, persisted: PersistedVault) -> bool {
        let Ok(amk_bytes) = b64_std().decode(&persisted.amk) else {
            return false;
        };
        let Ok(amk_bytes): Result<[u8; 32], _> = amk_bytes.try_into() else {
            return false;
        };
        let Ok(key) = UnlockedKey::open_unlocked(&persisted.secret_key_armored) else {
            return false;
        };
        let Ok(public_key_armored) = key.public_key_armored() else {
            return false;
        };

        self.vaults.lock().expect("keystore vaults").insert(
            account_id.to_owned(),
            Vault {
                account_id: account_id.to_owned(),
                email: persisted.email.clone(),
                auth_scheme: persisted.auth_scheme,
                key,
                public_key_armored,
                alias_keys: HashMap::new(),
                amk: Amk::from_bytes(amk_bytes),
            },
        );
        self.emit(Broadcast::VaultChanged {
            account_id: account_id.to_owned(),
            email: persisted.email,
        });
        true
    }

    pub fn clear(&self, account_id: &str) {
        self.vaults
            .lock()
            .expect("keystore vaults")
            .remove(account_id);
        self.emit(Broadcast::Cleared {
            account_id: account_id.to_owned(),
        });
    }

    pub async fn opaque_start_registration(
        &self,
        args: OpaqueStartRegistrationArgs,
    ) -> Result<OpaqueStartRegistrationResponse, KeystoreError> {
        let start = start_registration(&args.password).map_err(|_| KeystoreError::Protocol)?;
        let operation_id = Uuid::new_v4().to_string();
        let registration_request = b64_std().encode(&start.request);

        self.pending_reg
            .lock()
            .expect("keystore pending registration")
            .insert(
                operation_id.clone(),
                PendingReg {
                    kind: RegKind::Register,
                    state: Some(start.state),
                    password: Zeroizing::new(args.password),
                    email: args.email,
                    account_id: None,
                    unlocked: None,
                    public_key_armored: None,
                    amk: None,
                    wrapped_master_key: None,
                    master_key_id: None,
                },
            );

        Ok(OpaqueStartRegistrationResponse {
            operation_id,
            registration_request,
        })
    }

    pub async fn opaque_finish_registration(
        &self,
        args: OpaqueFinishRegistrationArgs,
    ) -> OpaqueFinishRegistrationResponse {
        let op = self
            .pending_reg
            .lock()
            .expect("keystore pending registration")
            .remove(&args.operation_id);
        let Some(op) = op else {
            return OpaqueFinishRegistrationResponse::Err {
                ok: false,
                code: "no_pending_operation",
            };
        };
        let (Some(state), Ok(response)) = (op.state, b64_std().decode(&args.registration_response))
        else {
            return OpaqueFinishRegistrationResponse::Err {
                ok: false,
                code: "no_pending_operation",
            };
        };

        let identity = client_identity(&args.account_id, false);
        let _permit = self.ksf_permit.acquire().await.expect("ksf permit");
        let finished =
            finish_registration(state, &op.password, &response, &identity, SERVER_IDENTITY);
        drop(_permit);

        let Ok(finished) = finished else {
            return OpaqueFinishRegistrationResponse::Err {
                ok: false,
                code: "no_pending_operation",
            };
        };

        let amk = Amk::generate();
        let wrapped = wrap_master_key(&finished.export_key, &amk, false);
        let master_key_id = b64_std().encode(derive_master_key_id(&amk));
        let passphrase = derive_pgp_passphrase(&amk);

        let Ok(generated) = generate_account_key("", &op.email, passphrase.expose()) else {
            return OpaqueFinishRegistrationResponse::Err {
                ok: false,
                code: "no_pending_operation",
            };
        };
        let Ok(unlocked) = UnlockedKey::open(
            &generated.encrypted_private_key_armored,
            passphrase.expose(),
        ) else {
            return OpaqueFinishRegistrationResponse::Err {
                ok: false,
                code: "no_pending_operation",
            };
        };

        self.pending_reg
            .lock()
            .expect("keystore pending registration")
            .insert(
                args.operation_id,
                PendingReg {
                    kind: RegKind::Register,
                    state: None,
                    password: op.password,
                    email: op.email,
                    account_id: Some(args.account_id),
                    unlocked: Some(unlocked),
                    public_key_armored: Some(generated.public_key_armored.clone()),
                    amk: Some(amk),
                    wrapped_master_key: None,
                    master_key_id: None,
                },
            );

        OpaqueFinishRegistrationResponse::Ok {
            ok: true,
            opaque_record: b64_std().encode(&finished.record),
            wrapped_master_key: b64_std().encode(wrapped),
            master_key_id,
            opaque_params_version: 1,
            public_key: generated.public_key_armored,
            encrypted_private_key: generated.encrypted_private_key_armored,
            key_algorithm: "openpgp-curve25519-v6",
        }
    }

    pub fn opaque_finalize_register(
        &self,
        args: OpaqueFinalizeRegisterArgs,
    ) -> OpaqueFinalizeRegisterResponse {
        let op = self
            .pending_reg
            .lock()
            .expect("keystore pending registration")
            .remove(&args.operation_id);
        let Some(op) = op else {
            return OpaqueFinalizeRegisterResponse::Err {
                ok: false,
                code: "no_pending_operation",
            };
        };
        let (Some(key), Some(public_key_armored), Some(amk)) =
            (op.unlocked, op.public_key_armored, op.amk)
        else {
            return OpaqueFinalizeRegisterResponse::Err {
                ok: false,
                code: "unwrap_failed",
            };
        };
        if op.account_id.as_deref() != Some(args.account_id.as_str()) {
            return OpaqueFinalizeRegisterResponse::Err {
                ok: false,
                code: "no_pending_operation",
            };
        }

        self.vaults.lock().expect("keystore vaults").insert(
            args.account_id.clone(),
            Vault {
                account_id: args.account_id.clone(),
                email: op.email.clone(),
                auth_scheme: AuthScheme::OpaqueV1,
                key,
                public_key_armored,
                alias_keys: HashMap::new(),
                amk,
            },
        );
        self.emit(Broadcast::VaultChanged {
            account_id: args.account_id.clone(),
            email: op.email.clone(),
        });

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
        if !self
            .vaults
            .lock()
            .expect("keystore vaults")
            .contains_key(&args.account_id)
        {
            return OpaqueRecoverySetupStartResponse::Err {
                ok: false,
                code: "locked",
            };
        }

        let mut entropy = [0u8; 16];
        OsRng.fill_bytes(&mut entropy);
        let Ok(mnemonic) = bip39::Mnemonic::from_entropy(&entropy) else {
            return OpaqueRecoverySetupStartResponse::Err {
                ok: false,
                code: "locked",
            };
        };
        let phrase = mnemonic.to_string();

        let Ok(start) = start_registration(&phrase) else {
            return OpaqueRecoverySetupStartResponse::Err {
                ok: false,
                code: "locked",
            };
        };
        let operation_id = Uuid::new_v4().to_string();
        let registration_request = b64_std().encode(&start.request);

        self.pending_reg
            .lock()
            .expect("keystore pending registration")
            .insert(
                operation_id.clone(),
                PendingReg {
                    kind: RegKind::RecoverySetup,
                    state: Some(start.state),
                    password: Zeroizing::new(phrase.clone()),
                    email: String::new(),
                    account_id: Some(args.account_id),
                    unlocked: None,
                    public_key_armored: None,
                    amk: None,
                    wrapped_master_key: None,
                    master_key_id: None,
                },
            );

        OpaqueRecoverySetupStartResponse::Ok {
            ok: true,
            operation_id,
            phrase,
            registration_request,
        }
    }

    pub async fn opaque_recovery_setup_finish(
        &self,
        args: OpaqueRecoverySetupFinishArgs,
    ) -> RewrapResponse {
        let amk = {
            let vaults = self.vaults.lock().expect("keystore vaults");
            match vaults.get(&args.account_id) {
                Some(v) => v.amk.clone(),
                None => return RewrapResponse::err("locked"),
            }
        };

        let op = self
            .pending_reg
            .lock()
            .expect("keystore pending registration")
            .remove(&args.operation_id);
        let Some(op) = op else {
            return RewrapResponse::err("no_pending_operation");
        };
        if op.kind != RegKind::RecoverySetup
            || op.account_id.as_deref() != Some(args.account_id.as_str())
        {
            return RewrapResponse::err("no_pending_operation");
        }
        let (Some(state), Ok(response)) = (op.state, b64_std().decode(&args.registration_response))
        else {
            return RewrapResponse::err("no_pending_operation");
        };

        let identity = client_identity(&args.account_id, true);
        let _permit = self.ksf_permit.acquire().await.expect("ksf permit");
        let finished =
            finish_registration(state, &op.password, &response, &identity, SERVER_IDENTITY);
        drop(_permit);

        let Ok(finished) = finished else {
            return RewrapResponse::err("no_pending_operation");
        };

        RewrapResponse::Ok {
            ok: true,
            opaque_record: b64_std().encode(&finished.record),
            wrapped_master_key: b64_std().encode(wrap_master_key(&finished.export_key, &amk, true)),
            master_key_id: b64_std().encode(derive_master_key_id(&amk)),
            opaque_params_version: 1,
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
        if !op.recovery {
            return PlainOkResponse::err("no_pending_operation");
        }
        let Some(export_key) = op.export_key.as_ref() else {
            return PlainOkResponse::err("no_pending_operation");
        };

        let Ok(wrapped) = b64_std().decode(&args.wrapped_master_key) else {
            return PlainOkResponse::err("invalid_credentials");
        };
        let Ok(amk) = unwrap_master_key(export_key, &wrapped, true) else {
            return PlainOkResponse::err("invalid_credentials");
        };
        let passphrase = derive_pgp_passphrase(&amk);
        let Ok(key) = UnlockedKey::open(&args.encrypted_private_key, passphrase.expose()) else {
            return PlainOkResponse::err("invalid_credentials");
        };

        op.amk = Some(amk);
        op.unlocked = Some(key);
        PlainOkResponse::ok()
    }

    pub async fn opaque_prepare_credential_reset(
        &self,
        args: OpaquePrepareCredentialResetArgs,
    ) -> RegistrationRequestResponse {
        let Ok(start) = start_registration(&args.new_password) else {
            return RegistrationRequestResponse::Err {
                ok: false,
                code: "no_pending_reset",
            };
        };

        let mut pending = self.pending.lock().expect("keystore pending");
        let Some(op) = pending.get_mut(&args.operation_id) else {
            return RegistrationRequestResponse::Err {
                ok: false,
                code: "no_pending_reset",
            };
        };
        if op.amk.is_none() || op.unlocked.is_none() {
            return RegistrationRequestResponse::Err {
                ok: false,
                code: "no_pending_reset",
            };
        }

        let registration_request = b64_std().encode(&start.request);
        op.reset_state = Some(start.state);
        op.new_password = Some(Zeroizing::new(args.new_password));

        RegistrationRequestResponse::Ok {
            ok: true,
            registration_request,
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
        let (Some(state), Some(password), Some(amk), Some(account_id)) =
            (op.reset_state, op.new_password, op.amk, op.account_id)
        else {
            return RewrapResponse::err("no_pending_reset");
        };
        let Ok(response) = b64_std().decode(&args.registration_response) else {
            return RewrapResponse::err("no_pending_reset");
        };

        let identity = client_identity(&account_id, false);
        let _permit = self.ksf_permit.acquire().await.expect("ksf permit");
        let finished = finish_registration(state, &password, &response, &identity, SERVER_IDENTITY);
        drop(_permit);

        let Ok(finished) = finished else {
            return RewrapResponse::err("no_pending_reset");
        };

        RewrapResponse::Ok {
            ok: true,
            opaque_record: b64_std().encode(&finished.record),
            wrapped_master_key: b64_std().encode(wrap_master_key(
                &finished.export_key,
                &amk,
                false,
            )),
            master_key_id: b64_std().encode(derive_master_key_id(&amk)),
            opaque_params_version: 1,
        }
    }

    pub async fn opaque_password_change_start(
        &self,
        args: OpaquePasswordChangeStartArgs,
    ) -> OperationStartResponse {
        if !self
            .vaults
            .lock()
            .expect("keystore vaults")
            .contains_key(&args.account_id)
        {
            return OperationStartResponse::Err {
                ok: false,
                code: "locked",
            };
        }
        let Ok(start) = start_registration(&args.new_password) else {
            return OperationStartResponse::Err {
                ok: false,
                code: "locked",
            };
        };

        let operation_id = Uuid::new_v4().to_string();
        let registration_request = b64_std().encode(&start.request);
        self.pending_reg
            .lock()
            .expect("keystore pending registration")
            .insert(
                operation_id.clone(),
                PendingReg {
                    kind: RegKind::PasswordChange,
                    state: Some(start.state),
                    password: Zeroizing::new(args.new_password),
                    email: String::new(),
                    account_id: Some(args.account_id),
                    unlocked: None,
                    public_key_armored: None,
                    amk: None,
                    wrapped_master_key: None,
                    master_key_id: None,
                },
            );

        OperationStartResponse::Ok {
            ok: true,
            operation_id,
            registration_request,
        }
    }

    pub async fn opaque_password_change_finish(
        &self,
        args: OpaquePasswordChangeFinishArgs,
    ) -> RewrapResponse {
        let amk = {
            let vaults = self.vaults.lock().expect("keystore vaults");
            match vaults.get(&args.account_id) {
                Some(v) => v.amk.clone(),
                None => return RewrapResponse::err("locked"),
            }
        };

        let op = self
            .pending_reg
            .lock()
            .expect("keystore pending registration")
            .remove(&args.operation_id);
        let Some(op) = op else {
            return RewrapResponse::err("no_pending_operation");
        };
        if op.kind != RegKind::PasswordChange
            || op.account_id.as_deref() != Some(args.account_id.as_str())
        {
            return RewrapResponse::err("no_pending_operation");
        }
        let (Some(state), Ok(response)) = (op.state, b64_std().decode(&args.registration_response))
        else {
            return RewrapResponse::err("no_pending_operation");
        };

        let identity = client_identity(&args.account_id, false);
        let _permit = self.ksf_permit.acquire().await.expect("ksf permit");
        let finished =
            finish_registration(state, &op.password, &response, &identity, SERVER_IDENTITY);
        drop(_permit);

        let Ok(finished) = finished else {
            return RewrapResponse::err("no_pending_operation");
        };

        let wrapped_master_key =
            b64_std().encode(wrap_master_key(&finished.export_key, &amk, false));
        let master_key_id = b64_std().encode(derive_master_key_id(&amk));

        self.pending_reg
            .lock()
            .expect("keystore pending registration")
            .insert(
                args.operation_id,
                PendingReg {
                    kind: RegKind::PasswordChange,
                    state: None,
                    password: Zeroizing::new(String::new()),
                    email: String::new(),
                    account_id: Some(args.account_id),
                    unlocked: None,
                    public_key_armored: None,
                    amk: None,
                    wrapped_master_key: Some(wrapped_master_key.clone()),
                    master_key_id: Some(master_key_id.clone()),
                },
            );

        RewrapResponse::Ok {
            ok: true,
            opaque_record: b64_std().encode(&finished.record),
            wrapped_master_key,
            master_key_id,
            opaque_params_version: 1,
        }
    }

    pub fn opaque_password_change_commit(
        &self,
        args: OpaquePasswordChangeCommitArgs,
    ) -> OpaquePasswordChangeCommitResponse {
        let op = self
            .pending_reg
            .lock()
            .expect("keystore pending registration")
            .remove(&args.operation_id);
        let Some(op) = op else {
            return OpaquePasswordChangeCommitResponse::Err {
                ok: false,
                code: "no_pending_operation",
            };
        };
        if op.kind != RegKind::PasswordChange
            || op.wrapped_master_key.is_none()
            || op.master_key_id.is_none()
        {
            return OpaquePasswordChangeCommitResponse::Err {
                ok: false,
                code: "no_pending_operation",
            };
        }

        let vaults = self.vaults.lock().expect("keystore vaults");
        let Some(vault) = vaults.get(&args.account_id) else {
            return OpaquePasswordChangeCommitResponse::Err {
                ok: false,
                code: "locked",
            };
        };
        let email = vault.email.clone();
        drop(vaults);

        self.emit(Broadcast::VaultChanged {
            account_id: args.account_id,
            email,
        });
        OpaquePasswordChangeCommitResponse::Ok {
            ok: true,
            persisted: false,
        }
    }

    pub fn create_alias_key(&self, args: CreateAliasKeyArgs) -> CreateAliasKeyResponse {
        if args.recipients.is_empty() {
            return CreateAliasKeyResponse::err("no_recipients");
        }
        let vaults = self.vaults.lock().expect("keystore vaults");
        let Some(vault) = vaults.get(&args.account_id) else {
            return CreateAliasKeyResponse::err("locked");
        };

        let Ok(generated) = generate_alias_key(&args.display_name, &args.email) else {
            return CreateAliasKeyResponse::err("unknown");
        };

        let mut grants = Vec::with_capacity(args.recipients.len());
        for recipient in &args.recipients {
            let Ok(fingerprint) = public_key_fingerprint_hex(&recipient.public_key_armored) else {
                return CreateAliasKeyResponse::err("invalid_recipient_key");
            };
            let wrapped = vault.key.encrypt_to_armored(
                std::slice::from_ref(&recipient.public_key_armored),
                generated.encrypted_private_key_armored.as_bytes(),
                Some(&vault.key),
            );
            let Ok(wrapped) = wrapped else {
                return CreateAliasKeyResponse::err("invalid_recipient_key");
            };
            grants.push(CreatedAliasKeyGrant {
                account_id: recipient.account_id.clone(),
                wrapped_private_key_armored: wrapped,
                member_key_fingerprint_hex: fingerprint,
            });
        }

        CreateAliasKeyResponse::Ok {
            ok: true,
            public_key_armored: generated.public_key_armored,
            key_fingerprint_hex: generated.fingerprint_hex,
            grants,
        }
    }

    pub fn commit_reformatted_key(&self, args: CommitReformattedKeyArgs) -> PlainOkResponse {
        let mut vaults = self.vaults.lock().expect("keystore vaults");
        let Some(vault) = vaults.get_mut(&args.account_id) else {
            return PlainOkResponse::err("locked");
        };

        let passphrase = derive_pgp_passphrase(&vault.amk);
        let Ok(key) = UnlockedKey::open(&args.encrypted_private_key, passphrase.expose()) else {
            return PlainOkResponse::err("invalid");
        };
        if key.fingerprint_hex() != vault.key.fingerprint_hex() {
            return PlainOkResponse::err("invalid");
        }
        let Ok(public_key_armored) = key.public_key_armored() else {
            return PlainOkResponse::err("invalid");
        };

        vault.key = key;
        vault.public_key_armored = public_key_armored;
        let account_id = vault.account_id.clone();
        let email = vault.email.clone();
        drop(vaults);

        self.emit(Broadcast::VaultChanged { account_id, email });
        PlainOkResponse::ok()
    }

    pub fn abandon(&self, operation_id: &str) {
        self.pending_reg
            .lock()
            .expect("keystore pending registration")
            .remove(operation_id);

        self.pending
            .lock()
            .expect("keystore pending")
            .remove(operation_id);
    }

    pub fn discard_recovery(&self) {
        self.pending
            .lock()
            .expect("keystore pending")
            .retain(|_, op| !op.recovery);
        self.pending_reg
            .lock()
            .expect("keystore pending registration")
            .retain(|_, op| op.kind != RegKind::RecoverySetup);
    }

    pub fn abandon_password_change(&self) {
        self.pending_reg
            .lock()
            .expect("keystore pending registration")
            .retain(|_, op| op.kind != RegKind::PasswordChange);
    }

    pub fn lock(&self, account_id: &str) {
        self.vaults
            .lock()
            .expect("keystore vaults")
            .remove(account_id);
        self.emit(Broadcast::Locked {
            account_id: account_id.to_owned(),
        });
    }

    pub fn clear_all(&self) {
        self.vaults.lock().expect("keystore vaults").clear();
        self.pending.lock().expect("keystore pending").clear();
        self.emit(Broadcast::ClearedAll);
    }

    pub fn decrypt(&self, args: DecryptArgs) -> DecryptResponse {
        let vaults = self.vaults.lock().expect("keystore vaults");
        let Some(vault) = vaults.get(&args.account_id) else {
            return DecryptResponse::err("locked");
        };

        let ciphertext = match (&args.ciphertext_binary, &args.ciphertext_armored) {
            (Some(bytes), _) => bytes.clone(),
            (None, Some(armored)) => match b64_std().decode(armored) {
                Ok(b) => b,
                Err(_) => armored.as_bytes().to_vec(),
            },
            (None, None) => return DecryptResponse::err("invalid_ciphertext"),
        };

        let verification_keys = args.verification_keys_armored.unwrap_or_default();
        let plain = vault
            .key
            .decrypt_verified(&ciphertext, &verification_keys)
            .or_else(|first| {
                vault
                    .alias_keys
                    .values()
                    .find_map(|k| k.key.decrypt_verified(&ciphertext, &verification_keys).ok())
                    .ok_or(first)
            });

        match plain {
            Ok((plain, check)) if args.binary => DecryptResponse::Binary {
                ok: true,
                plaintext_binary: plain,
                signature: check.map(verdict),
            },
            Ok((plain, check)) => match String::from_utf8(plain) {
                Ok(text) => DecryptResponse::Text {
                    ok: true,
                    plaintext: text,
                    signature: check.map(verdict),
                },
                Err(_) => DecryptResponse::err("invalid_ciphertext"),
            },
            Err(_) => DecryptResponse::err("invalid_ciphertext"),
        }
    }

    pub fn load_alias_keys(&self, args: LoadAliasKeysArgs) -> LoadAliasKeysResponse {
        let mut vaults = self.vaults.lock().expect("keystore vaults");
        let Some(vault) = vaults.get_mut(&args.account_id) else {
            return LoadAliasKeysResponse::Err {
                ok: false,
                code: "locked",
            };
        };

        let mut loaded = Vec::new();
        let mut failed = Vec::new();

        for grant in args.grants {
            let want = grant.alias_key_fingerprint_hex.to_lowercase();
            if vault.alias_keys.contains_key(&want) {
                loaded.push(want);
                continue;
            }

            let unwrapped = vault
                .key
                .decrypt(grant.wrapped_private_key_armored.as_bytes())
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .and_then(|armored| UnlockedKey::open_unlocked(&armored).ok());

            match unwrapped {
                Some(key) if key.fingerprint_hex() == want => {
                    vault.alias_keys.insert(
                        want.clone(),
                        AliasKey {
                            key,
                            alias_id: grant.alias_id,
                            key_version: grant.key_version,
                        },
                    );
                    loaded.push(want);
                }
                _ => failed.push(FailedGrant {
                    alias_id: grant.alias_id,
                    key_version: grant.key_version,
                }),
            }
        }

        LoadAliasKeysResponse::Ok {
            ok: true,
            loaded,
            failed,
        }
    }

    pub fn unload_alias_keys(&self, account_id: &str) {
        if let Some(vault) = self
            .vaults
            .lock()
            .expect("keystore vaults")
            .get_mut(account_id)
        {
            vault.alias_keys.clear();
        }
    }

    pub fn decrypt_bytes(
        &self,
        account_id: &str,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, &'static str> {
        let vaults = self.vaults.lock().expect("keystore vaults");
        let Some(vault) = vaults.get(account_id) else {
            return Err("locked");
        };
        vault.key.decrypt(ciphertext).or_else(|_| {
            vault
                .alias_keys
                .values()
                .find_map(|k| k.key.decrypt(ciphertext).ok())
                .ok_or("invalid_ciphertext")
        })
    }
}

fn verdict(check: SignatureCheck) -> SignatureVerdict {
    SignatureVerdict {
        state: match check.state {
            SignatureState::Valid => "valid",
            SignatureState::Invalid => "invalid",
            SignatureState::None => "none",
            SignatureState::UnknownKey => "unknown_key",
        },
        key_fingerprint_hex: check.key_fingerprint_hex,
        signed_at_millis: check.signed_at_millis,
    }
}

fn signing_key<'a>(vault: &'a Vault, alias_id: Option<&str>) -> Option<&'a UnlockedKey> {
    let Some(alias_id) = alias_id else {
        return Some(&vault.key);
    };
    vault
        .alias_keys
        .values()
        .filter(|entry| entry.alias_id == alias_id)
        .max_by_key(|entry| entry.key_version)
        .map(|entry| &entry.key)
}

impl Keystore {
    pub fn encrypt(&self, args: EncryptArgs) -> EncryptResponse {
        let vaults = self.vaults.lock().expect("keystore vaults");
        let Some(vault) = vaults.get(&args.account_id) else {
            return EncryptResponse::Err {
                ok: false,
                code: "locked",
            };
        };
        let signer = if args.sign_with_vault_key {
            match signing_key(vault, args.alias_id.as_deref()) {
                Some(key) => Some(key),
                None => {
                    return EncryptResponse::Err {
                        ok: false,
                        code: "locked",
                    };
                }
            }
        } else {
            None
        };

        match vault.key.encrypt_to(
            std::slice::from_ref(&args.recipient_public_key_armored),
            &args.plaintext,
            signer,
        ) {
            Ok(ciphertext) => EncryptResponse::Ok {
                ok: true,
                ciphertext,
            },
            Err(_) => EncryptResponse::Err {
                ok: false,
                code: "invalid_recipient_key",
            },
        }
    }

    pub fn encrypt_to_keys(&self, args: EncryptToKeysArgs) -> EncryptToKeysResponse {
        let vaults = self.vaults.lock().expect("keystore vaults");
        let Some(vault) = vaults.get(&args.account_id) else {
            return EncryptToKeysResponse::Err {
                ok: false,
                code: "locked",
            };
        };
        if args.recipient_public_keys_armored.is_empty() {
            return EncryptToKeysResponse::Err {
                ok: false,
                code: "no_recipients",
            };
        }
        let signer = if args.sign_with_vault_key {
            match signing_key(vault, args.alias_id.as_deref()) {
                Some(key) => Some(key),
                None => {
                    return EncryptToKeysResponse::Err {
                        ok: false,
                        code: "locked",
                    };
                }
            }
        } else {
            None
        };

        match vault.key.encrypt_to_armored(
            &args.recipient_public_keys_armored,
            &args.plaintext,
            signer,
        ) {
            Ok(armored) => EncryptToKeysResponse::Ok { ok: true, armored },
            Err(_) => EncryptToKeysResponse::Err {
                ok: false,
                code: "invalid_recipient_key",
            },
        }
    }

    pub fn get_public_key(&self, account_id: &str) -> GetPublicKeyResponse {
        let vaults = self.vaults.lock().expect("keystore vaults");
        match vaults.get(account_id) {
            Some(vault) => GetPublicKeyResponse::Ok {
                ok: true,
                public_key_armored: vault.public_key_armored.clone(),
                fingerprint: vault.key.fingerprint_bytes(),
            },
            None => GetPublicKeyResponse::Err {
                ok: false,
                code: "locked",
            },
        }
    }
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

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
