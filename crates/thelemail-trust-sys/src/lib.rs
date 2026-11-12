#![no_std]
#![allow(non_snake_case)]

use core::ffi::c_char;

unsafe extern "C" {
    pub fn ThelemailOpaque(request: *const c_char) -> *mut c_char;
    pub fn ThelemailSession(request: *const c_char) -> *mut c_char;
    pub fn ThelemailMail(request: *const c_char) -> *mut c_char;
    pub fn ThelemailKeys(request: *const c_char) -> *mut c_char;
    pub fn ThelemailTrust(request: *const c_char) -> *mut c_char;
    pub fn ThelemailProof(request: *const c_char) -> *mut c_char;
    pub fn ThelemailVerifyTlogProof(request: *const c_char) -> *mut c_char;
    pub fn ThelemailFreeCString(response: *mut c_char);
}
