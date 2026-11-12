#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, OnceLock};

use base64::Engine as _;
use serde_json::{Value, json};
use thelemail_keystore::*;
use thelemail_trust::Family;

pub const EMAIL: &str = "nora@thelemail.test";
pub const PASSWORD: &str = "a long and sturdy passphrase";

pub fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

pub fn new_account_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().expect("runtime")
}

pub fn testkit(request: Value) -> Value {
    thelemail_trust::call(Family::Testkit, &request)
        .unwrap_or_else(|err| panic!("testkit {request}: {err}"))
}

pub fn trust(family: Family, request: Value) -> Value {
    thelemail_trust::call(family, &request).unwrap_or_else(|err| panic!("{request}: {err}"))
}

#[derive(Clone, Debug)]
pub struct Key {
    pub fingerprint_hex: String,
    pub encrypted_private_key_armored: String,
    pub public_key_armored: String,
}

#[derive(Clone)]
pub struct Pass(pub String);

impl Pass {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

pub fn generate(pass: &Pass, email: &str) -> Key {
    let out = testkit(json!({"op": "generate", "email": email, "passphrase": pass.0}));
    Key {
        fingerprint_hex: out["fingerprint"].as_str().unwrap().to_owned(),
        encrypted_private_key_armored: out["encryptedPrivateKey"].as_str().unwrap().to_owned(),
        public_key_armored: out["publicKey"].as_str().unwrap().to_owned(),
    }
}

pub fn endorsement_for(
    account_id: &str,
    pass: &Pass,
    by: &Key,
    new_fp: &str,
) -> AccountKeyTransitionInput {
    let out = testkit(json!({
        "op": "signTransition",
        "accountId": account_id,
        "encryptedPrivateKey": by.encrypted_private_key_armored,
        "passphrase": pass.0,
        "newKeyFingerprint": new_fp,
    }));
    AccountKeyTransitionInput {
        previous_key_fingerprint_hex: by.fingerprint_hex.clone(),
        signature: b64().decode(out["signature"].as_str().unwrap()).unwrap(),
    }
}

pub fn open_with_passphrase(ks: &Keystore, account_id: &str, pass: &Pass, key: &Key, version: i64) {
    testkit(json!({
        "op": "openSession",
        "accountId": account_id,
        "encryptedPrivateKey": key.encrypted_private_key_armored,
        "passphrase": pass.0,
        "keyVersion": version,
    }));
    let server_half = [0x33u8; 32];
    let persisted = trust(
        Family::Session,
        json!({"op": "persist", "accountId": account_id, "serverHalf": b64().encode(server_half)}),
    );
    let wrapped: WrappedSecret = serde_json::from_value(persisted["wrapped"].clone()).unwrap();
    let keys = SavedKeys {
        encrypted_private_key: key.encrypted_private_key_armored.clone(),
        key_version: version,
        key_anchored: false,
        history_keys: Vec::new(),
    };
    assert!(ks.restore(
        account_id,
        EMAIL.to_owned(),
        &keys,
        &wrapped,
        persisted["wrapVersion"].as_i64().unwrap(),
        &server_half,
    ));
}

struct ServerProcess {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

fn server_binary() -> &'static PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("opaque-server");
        let status = Command::new(std::env::var("GO").unwrap_or_else(|_| "go".into()))
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../xtask/go"))
            .env("GOWORK", "off")
            .args(["build", "-o"])
            .arg(&out)
            .arg("./opaque-server")
            .status()
            .expect("go build opaque-server");
        assert!(status.success(), "go build opaque-server failed");
        out
    })
}

pub struct OpaqueServer(Mutex<ServerProcess>);

impl OpaqueServer {
    pub fn start() -> Self {
        let mut child = Command::new(server_binary())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn opaque-server");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self(Mutex::new(ServerProcess {
            _child: child,
            stdin,
            stdout,
        }))
    }

    fn ask(&self, request: Value) -> String {
        let mut process = self.0.lock().unwrap();
        writeln!(process.stdin, "{request}").unwrap();
        let mut line = String::new();
        process.stdout.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        if let Some(err) = reply["error"].as_str() {
            panic!("opaque-server {request}: {err}");
        }
        reply["message"].as_str().unwrap().to_owned()
    }

    pub fn register(&self, account_id: &str, request: &str, recovery: bool) -> String {
        self.ask(json!({"op": "register", "accountId": account_id, "message": request, "recovery": recovery}))
    }

    pub fn ke2(&self, account_id: &str, ke1: &str, record: &str, recovery: bool) -> String {
        self.ask(json!({"op": "login", "accountId": account_id, "message": ke1, "record": record, "recovery": recovery}))
    }
}

impl Drop for OpaqueServer {
    fn drop(&mut self) {
        if let Ok(mut process) = self.0.lock() {
            let _ = process._child.kill();
            let _ = process._child.wait();
        }
    }
}

pub struct Registered {
    pub account_id: String,
    pub record: String,
    pub wrapped_master_key: String,
    pub master_key_id: String,
    pub public_key: String,
    pub encrypted_private_key: String,
}

pub fn register(ks: &Keystore, server: &OpaqueServer, account_id: &str) -> Registered {
    let rt = runtime();
    let start = rt
        .block_on(ks.opaque_start_registration(OpaqueStartRegistrationArgs {
            email: EMAIL.to_owned(),
            password: PASSWORD.to_owned(),
        }))
        .expect("registration start");
    let response = server.register(account_id, &start.registration_request, false);
    let finished = serde_json::to_value(rt.block_on(ks.opaque_finish_registration(
        OpaqueFinishRegistrationArgs {
            operation_id: start.operation_id.clone(),
            account_id: account_id.to_owned(),
            registration_response: response,
        },
        0,
    )))
    .unwrap();
    assert_eq!(finished["ok"], true, "{finished}");
    assert_eq!(finished["keyAlgorithm"], "openpgp-curve25519-v6");
    let finalized = serde_json::to_value(ks.opaque_finalize_register(OpaqueFinalizeRegisterArgs {
        operation_id: start.operation_id,
        account_id: account_id.to_owned(),
    }))
    .unwrap();
    assert_eq!(finalized["ok"], true, "{finalized}");
    let s = |k: &str| finished[k].as_str().unwrap().to_owned();
    Registered {
        account_id: account_id.to_owned(),
        record: s("opaqueRecord"),
        wrapped_master_key: s("wrappedMasterKey"),
        master_key_id: s("masterKeyId"),
        public_key: s("publicKey"),
        encrypted_private_key: s("encryptedPrivateKey"),
    }
}

pub fn sign_in_as(
    ks: &Keystore,
    server: &OpaqueServer,
    account: &Registered,
    password: &str,
    encrypted_private_key: &str,
    stored: Vec<StoredAccountKey>,
    scheme: AuthScheme,
) -> Value {
    let rt = runtime();
    let started = rt
        .block_on(ks.opaque_start_auth(OpaqueStartAuthArgs {
            password: password.to_owned(),
            email: Some(EMAIL.to_owned()),
            recovery: false,
        }))
        .expect("auth start");
    let ke2 = server.ke2(&account.account_id, &started.ke1, &account.record, false);
    let finished = serde_json::to_value(rt.block_on(ks.opaque_finish_auth(OpaqueFinishAuthArgs {
        operation_id: started.operation_id.clone(),
        account_id: account.account_id.clone(),
        ke2,
        recovery: false,
    })))
    .unwrap();
    if finished["ok"] != true {
        return finished;
    }
    serde_json::to_value(ks.opaque_complete_login_unlock(
        OpaqueCompleteLoginUnlockArgs {
            operation_id: started.operation_id,
            account_id: account.account_id.clone(),
            encrypted_private_key: encrypted_private_key.to_owned(),
            wrapped_master_key: account.wrapped_master_key.clone(),
            master_key_id: account.master_key_id.clone(),
            opaque_params_version: 1,
            server_auth_scheme: scheme,
        },
        stored,
    ))
    .unwrap()
}

pub fn sign_in(
    ks: &Keystore,
    server: &OpaqueServer,
    account: &Registered,
    encrypted_private_key: &str,
    stored: Vec<StoredAccountKey>,
) -> Value {
    sign_in_as(
        ks,
        server,
        account,
        PASSWORD,
        encrypted_private_key,
        stored,
        AuthScheme::OpaqueV1,
    )
}

pub fn open_product_keys_account(ks: &Keystore) -> (String, Value) {
    let account_id = new_account_id();
    let account = testkit(json!({
        "op": "productKeysAccount",
        "accountId": account_id,
        "email": EMAIL,
    }));
    let server_half = [0x44u8; 32];
    let persisted = trust(
        Family::Session,
        json!({"op": "persist", "accountId": account_id, "serverHalf": b64().encode(server_half)}),
    );
    let wrapped: WrappedSecret = serde_json::from_value(persisted["wrapped"].clone()).unwrap();
    let keys = SavedKeys {
        encrypted_private_key: account["accountKeys"]["keys"][1]["encryptedPrivateKey"]
            .as_str()
            .unwrap()
            .to_owned(),
        key_version: 2,
        key_anchored: false,
        history_keys: Vec::new(),
    };
    assert!(ks.restore(
        &account_id,
        EMAIL.to_owned(),
        &keys,
        &wrapped,
        persisted["wrapVersion"].as_i64().unwrap(),
        &server_half,
    ));
    (account_id, account)
}
