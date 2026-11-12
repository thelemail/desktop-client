use std::collections::HashMap;

use base64::Engine as _;
use serde::Deserialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};
use thelemail_api::{ApiRequest, Net};
use thelemail_keystore::Keystore;

use crate::keystore::{PersistentHalves, clear_account};

const PRODUCT: &str = "app";

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    NoHead,
    Verified,
    Unreachable(String),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Refused(pub String);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundleRef {
    bundle_id: String,
    revision: i64,
}

#[derive(Deserialize)]
struct IdentityManifest {
    bundles: Vec<BundleRef>,
}

enum Fetched {
    Body(Value),
    Refused(u16),
    Unreachable(String),
}

async fn get(net: &Net, account_id: &str, token: &str, path: &str) -> Fetched {
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_owned(), format!("Bearer {token}"));
    headers.insert("X-Account-Id".to_owned(), account_id.to_owned());
    headers.insert("X-Client".to_owned(), "desktop".to_owned());
    let resp = match net
        .request(ApiRequest {
            url: format!("{}{path}", net.config().api_base),
            method: "GET".to_owned(),
            headers,
            body: None,
        })
        .await
    {
        Ok(resp) => resp,
        Err(err) => return Fetched::Unreachable(err.to_string()),
    };
    match resp.status {
        200 => match serde_json::from_slice(&resp.body.unwrap_or_default()) {
            Ok(v) => Fetched::Body(v),
            Err(err) => Fetched::Unreachable(err.to_string()),
        },
        401 | 408 | 429 | 500..=599 => {
            Fetched::Unreachable(format!("{path} returned {}", resp.status))
        }
        status => Fetched::Refused(status),
    }
}

fn bundle_refs(product_keys: &Value) -> Result<Vec<BundleRef>, Refused> {
    let record = product_keys
        .pointer("/identityManifest/record")
        .and_then(Value::as_str)
        .ok_or_else(|| Refused("account head v2 without an identity manifest".to_owned()))?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(record)
        .map_err(|_| Refused("identity manifest is not base64".to_owned()))?;
    let manifest: IdentityManifest = serde_json::from_slice(&raw)
        .map_err(|_| Refused("identity manifest is malformed".to_owned()))?;
    Ok(manifest.bundles)
}

pub async fn verify(
    net: &Net,
    ks: &Keystore,
    account_id: &str,
    token: &str,
) -> Result<Outcome, Refused> {
    let product_keys = match get(
        net,
        account_id,
        token,
        &format!("v1/me/product-keys/{PRODUCT}"),
    )
    .await
    {
        Fetched::Body(v) => v,
        Fetched::Refused(status) => {
            return Ok(Outcome::Unreachable(format!(
                "product keys returned {status}"
            )));
        }
        Fetched::Unreachable(err) => return Ok(Outcome::Unreachable(err)),
    };
    if product_keys
        .pointer("/head/version")
        .and_then(Value::as_i64)
        != Some(2)
    {
        return Ok(Outcome::NoHead);
    }
    let refs = bundle_refs(&product_keys)?;
    let root = match get(
        net,
        account_id,
        token,
        &format!("v1/me/product-keys/{PRODUCT}/root-descriptor"),
    )
    .await
    {
        Fetched::Body(v) => v,
        Fetched::Refused(status) => return Err(Refused(format!("product root returned {status}"))),
        Fetched::Unreachable(err) => return Ok(Outcome::Unreachable(err)),
    };
    let mut bundles = Vec::with_capacity(refs.len());
    for r in refs {
        let path = format!(
            "v1/me/product-keys/{PRODUCT}/bundles/{}/revisions/{}",
            r.bundle_id, r.revision
        );
        match get(net, account_id, token, &path).await {
            Fetched::Body(v) => bundles.push(v),
            Fetched::Refused(status) => {
                return Err(Refused(format!("product bundle returned {status}")));
            }
            Fetched::Unreachable(err) => return Ok(Outcome::Unreachable(err)),
        }
    }
    ks.read_product_keys(account_id, &product_keys, &root, &bundles)
        .map_err(|err| Refused(err.message().to_owned()))?;
    Ok(Outcome::Verified)
}

pub async fn verify_or_clear(app: &AppHandle, account_id: &str, token: &str) -> bool {
    let net = app.state::<Net>();
    let ks = app.state::<Keystore>();
    match verify(&net, &ks, account_id, token).await {
        Ok(Outcome::Unreachable(err)) => {
            eprintln!("productkeys: could not check {account_id}: {err}");
            true
        }
        Ok(_) => true,
        Err(Refused(reason)) => {
            eprintln!("productkeys: {account_id} refused: {reason}");
            clear_account(&ks, &app.state::<PersistentHalves>(), account_id);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    use serde_json::json;
    use thelemail_api::ApiConfig;
    use thelemail_trust::Family;
    use url::Url;

    use super::*;

    type Routes = HashMap<String, (u16, Value)>;

    fn serve(routes: Routes) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut request_line = String::new();
                let _ = reader.read_line(&mut request_line);
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let path = request_line.split(' ').nth(1).unwrap_or("/").to_owned();
                let (status, body) = routes
                    .get(&path)
                    .cloned()
                    .unwrap_or((404, json!({"error": "not found"})));
                let body = body.to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        format!("http://{addr}/")
    }

    fn net(api: &str) -> Net {
        Net::new(ApiConfig {
            api_base: Url::parse(api).expect("url"),
            submission_base: Url::parse("http://127.0.0.1:9").expect("url"),
            web_origin: "http://localhost:5175".to_owned(),
        })
        .expect("net")
    }

    fn testkit(request: Value) -> Value {
        thelemail_trust::call(Family::Testkit, &request).expect("testkit")
    }

    struct Fixture {
        account_id: String,
        routes: Routes,
    }

    fn fixture() -> Fixture {
        let account_id = uuid::Uuid::new_v4().to_string();
        let opened = testkit(json!({
            "op": "openSession",
            "accountId": account_id,
            "masterKey": true,
            "email": "nora@thelemail.test",
        }));
        let account = testkit(json!({
            "op": "productKeysAccount",
            "accountId": account_id,
            "encryptedPrivateKey": opened["encryptedPrivateKey"],
        }));
        let mut routes = Routes::new();
        routes.insert(
            "/v1/me/product-keys/app".to_owned(),
            (200, account["productKeys"].clone()),
        );
        routes.insert(
            "/v1/me/product-keys/app/root-descriptor".to_owned(),
            (200, account["rootDescriptor"].clone()),
        );
        for b in account["productBundles"].as_array().unwrap() {
            routes.insert(
                format!(
                    "/v1/me/product-keys/app/bundles/{}/revisions/{}",
                    b["bundleId"].as_str().unwrap(),
                    b["bundleRevision"]
                ),
                (200, b.clone()),
            );
        }
        Fixture { account_id, routes }
    }

    fn run(f: &Fixture) -> Result<Outcome, Refused> {
        let net = net(&serve(f.routes.clone()));
        let ks = Keystore::new();
        tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(verify(&net, &ks, &f.account_id, "token"))
    }

    fn route<'a>(f: &'a mut Fixture, suffix: &str) -> &'a mut (u16, Value) {
        f.routes
            .iter_mut()
            .find(|(path, _)| path.ends_with(suffix))
            .map(|(_, v)| v)
            .expect("route")
    }

    #[test]
    fn a_v2_account_verifies() {
        assert_eq!(run(&fixture()), Ok(Outcome::Verified));
    }

    #[test]
    fn an_account_without_a_head_is_left_alone() {
        let mut f = fixture();
        route(&mut f, "/product-keys/app").1["head"] = Value::Null;
        assert_eq!(run(&f), Ok(Outcome::NoHead));
    }

    #[test]
    fn a_server_that_cannot_answer_is_not_a_refusal() {
        let mut f = fixture();
        route(&mut f, "/product-keys/app").0 = 503;
        assert!(matches!(run(&f), Ok(Outcome::Unreachable(_))));

        let mut f = fixture();
        route(&mut f, "/product-keys/app").0 = 404;
        assert!(matches!(run(&f), Ok(Outcome::Unreachable(_))));

        let mut f = fixture();
        route(&mut f, "/revisions/1").0 = 502;
        assert!(matches!(run(&f), Ok(Outcome::Unreachable(_))));
    }

    #[test]
    fn a_forged_head_is_refused() {
        let mut f = fixture();
        route(&mut f, "/product-keys/app").1["head"]["mac"] =
            json!(base64::engine::general_purpose::STANDARD.encode([0u8; 32]));
        assert!(run(&f).is_err());
    }

    #[test]
    fn a_v2_head_whose_root_is_withheld_is_refused() {
        let mut f = fixture();
        route(&mut f, "/root-descriptor").0 = 403;
        assert!(run(&f).is_err());

        let mut f = fixture();
        route(&mut f, "/revisions/1").0 = 404;
        assert!(run(&f).is_err());
    }
}
