use std::ffi::{CStr, CString, c_char};

use serde::Serialize;
use serde::de::DeserializeOwned;
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Opaque,
    Session,
    Mail,
    Keys,
    Trust,
    Proof,
    Tlog,
    #[cfg(feature = "testkit")]
    Testkit,
}

#[derive(Debug, thiserror::Error)]
pub enum TrustError {
    #[error("{0}")]
    Call(String),
    #[error("trust: request could not be encoded")]
    Request,
    #[error("trust: response could not be decoded")]
    Response,
}

impl TrustError {
    pub fn message(&self) -> &str {
        match self {
            Self::Call(message) => message,
            Self::Request => "trust: request could not be encoded",
            Self::Response => "trust: response could not be decoded",
        }
    }
}

#[derive(serde::Deserialize)]
struct Envelope<T> {
    error: Option<String>,
    result: Option<T>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct Empty {}

pub fn call<T: DeserializeOwned>(
    family: Family,
    request: &impl Serialize,
) -> Result<T, TrustError> {
    let response = exchange(family, request)?;
    let envelope: Envelope<T> =
        serde_json::from_slice(&response).map_err(|_| TrustError::Response)?;
    match (envelope.error, envelope.result) {
        (Some(error), _) => Err(TrustError::Call(error)),
        (None, Some(result)) => Ok(result),
        (None, None) => Err(TrustError::Response),
    }
}

pub fn call_bare<T: DeserializeOwned>(
    family: Family,
    request: &impl Serialize,
) -> Result<T, TrustError> {
    let response = exchange(family, request)?;
    serde_json::from_slice(&response).map_err(|_| TrustError::Response)
}

fn exchange(family: Family, request: &impl Serialize) -> Result<Zeroizing<Vec<u8>>, TrustError> {
    let encoded = Zeroizing::new(serde_json::to_vec(request).map_err(|_| TrustError::Request)?);
    let request = match CString::new(encoded.to_vec()) {
        Ok(request) => request,
        Err(err) => {
            err.into_vec().zeroize();
            return Err(TrustError::Request);
        }
    };
    let response = raw(family, &request);
    let mut spent = request.into_bytes_with_nul();
    spent.zeroize();
    response
}

#[allow(unsafe_code)]
fn raw(family: Family, request: &CStr) -> Result<Zeroizing<Vec<u8>>, TrustError> {
    use thelemail_trust_sys as sys;

    let entry: unsafe extern "C" fn(*const c_char) -> *mut c_char = match family {
        Family::Opaque => sys::ThelemailOpaque,
        Family::Session => sys::ThelemailSession,
        Family::Mail => sys::ThelemailMail,
        Family::Keys => sys::ThelemailKeys,
        Family::Trust => sys::ThelemailTrust,
        Family::Proof => sys::ThelemailProof,
        Family::Tlog => sys::ThelemailVerifyTlogProof,
        #[cfg(feature = "testkit")]
        Family::Testkit => sys::ThelemailTestkit,
    };
    let response = unsafe { entry(request.as_ptr()) };
    if response.is_null() {
        return Err(TrustError::Response);
    }
    let bytes = unsafe { CStr::from_ptr(response) }.to_bytes();
    let copied = Zeroizing::new(bytes.to_vec());
    let len = bytes.len();
    unsafe {
        std::ptr::write_bytes(response, 0, len);
        sys::ThelemailFreeCString(response);
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_c_abi() {
        let words: serde_json::Value =
            call(Family::Keys, &serde_json::json!({"op": "wordlist"})).expect("wordlist");
        assert_eq!(words["words"].as_array().map(Vec::len), Some(2048));
    }

    #[test]
    fn surfaces_bridge_errors() {
        let err = call::<Empty>(Family::Session, &serde_json::json!({"op": "nope"}))
            .expect_err("unknown op");
        assert!(err.message().contains("unknown operation"), "{err}");
    }

    #[test]
    fn tlog_answers_without_an_envelope() {
        let out: serde_json::Value = call_bare(Family::Tlog, &serde_json::json!({})).expect("tlog");
        assert!(out["code"].is_string(), "{out}");
    }
}
