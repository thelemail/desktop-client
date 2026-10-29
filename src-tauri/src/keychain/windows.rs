use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
use windows_sys::Win32::Security::Credentials::{
    CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW,
    CredDeleteW, CredFree, CredReadW, CredWriteW,
};

use super::Read;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn target(service: &str, account: &str) -> Vec<u16> {
    wide(&format!("{service}/{account}"))
}

fn last_error(op: &str) -> (u32, String) {
    let code = unsafe { GetLastError() };
    (code, format!("{op} failed with windows error {code}"))
}

pub(super) fn hardening_available() -> bool {
    true
}

pub(super) fn read(service: &str, account: &str) -> Read {
    let target = target(service, account);
    let mut credential: *mut CREDENTIALW = ptr::null_mut();
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
        let (code, message) = last_error("CredReadW");
        return if code == ERROR_NOT_FOUND {
            Read::NotPresent
        } else {
            Read::Failed(message)
        };
    }
    let bytes = unsafe {
        let blob = &*credential;
        let bytes = if blob.CredentialBlob.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(blob.CredentialBlob, blob.CredentialBlobSize as usize)
                .to_vec()
        };
        CredFree(credential.cast());
        bytes
    };
    match String::from_utf8(bytes) {
        Ok(value) => Read::Found(value),
        Err(_) => Read::Failed("stored value is not valid utf-8".to_owned()),
    }
}

pub(super) fn write(service: &str, account: &str, value: &str) -> Result<(), String> {
    if value.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
        return Err("value is too large for the credential store".to_owned());
    }
    let mut target = target(service, account);
    let mut user = wide(account);
    let mut blob = value.as_bytes().to_vec();
    let credential = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        Comment: ptr::null_mut(),
        LastWritten: unsafe { std::mem::zeroed() },
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: ptr::null_mut(),
        TargetAlias: ptr::null_mut(),
        UserName: user.as_mut_ptr(),
    };
    if unsafe { CredWriteW(&credential, 0) } == 0 {
        return Err(last_error("CredWriteW").1);
    }
    Ok(())
}

pub(super) fn remove(service: &str, account: &str) -> Result<(), String> {
    let target = target(service, account);
    if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
        let (code, message) = last_error("CredDeleteW");
        if code != ERROR_NOT_FOUND {
            return Err(message);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SERVICE: &str = "com.thelemail.desktop.test.credentials";
    const TEST_ACCOUNT: &str = "11111111-2222-3333-4444-555555555555";

    #[test]
    fn a_credential_round_trips_and_can_be_removed() {
        let _ = remove(TEST_SERVICE, TEST_ACCOUNT);
        assert!(matches!(read(TEST_SERVICE, TEST_ACCOUNT), Read::NotPresent));

        write(TEST_SERVICE, TEST_ACCOUNT, "first").expect("write");
        write(TEST_SERVICE, TEST_ACCOUNT, "second").expect("overwrite");
        match read(TEST_SERVICE, TEST_ACCOUNT) {
            Read::Found(value) => assert_eq!(value, "second"),
            other => panic!("expected the stored credential, got {other:?}"),
        }

        remove(TEST_SERVICE, TEST_ACCOUNT).expect("remove");
        assert!(matches!(read(TEST_SERVICE, TEST_ACCOUNT), Read::NotPresent));
        remove(TEST_SERVICE, TEST_ACCOUNT).expect("removing twice is not an error");
    }

    #[test]
    fn services_and_accounts_do_not_collide() {
        let mine = "33333333-4444-5555-6666-777777777777";
        let other = "22222222-3333-4444-5555-666666666666";
        write(TEST_SERVICE, mine, "mine").expect("write");
        assert!(matches!(read(TEST_SERVICE, other), Read::NotPresent));
        assert!(matches!(
            read("com.thelemail.desktop.test.other", mine),
            Read::NotPresent
        ));
        remove(TEST_SERVICE, mine).expect("remove");
    }

    #[test]
    fn an_oversized_value_is_refused_before_it_reaches_the_store() {
        let huge = "x".repeat(CRED_MAX_CREDENTIAL_BLOB_SIZE as usize + 1);
        assert!(write(TEST_SERVICE, TEST_ACCOUNT, &huge).is_err());
    }
}
