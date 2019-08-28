use std::os::raw::{c_char, c_int};
/// This modules defines stubs for functions from janus-plugin-sys crate to enable linking when
/// compiling for running unit tests.
use std::ptr;

use janus_plugin_sys::sdp::janus_sdp;

// lib.rs

#[no_mangle]
pub static janus_log_timestamps: c_int = 0;

#[no_mangle]
pub static janus_log_colors: c_int = 0;

#[no_mangle]
pub static janus_log_level: c_int = 3;

#[no_mangle]
pub static refcount_debug: c_int = 0;

#[no_mangle]
pub extern "C" fn janus_get_api_error(_error: c_int) -> *const c_char {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn janus_vprintf(_format: *const c_char, _args: ...) {}

// sdp.rs

#[no_mangle]
pub extern "C" fn janus_sdp_parse(
    _sdp: *const c_char,
    _error: *mut c_char,
    _errlen: usize,
) -> *mut janus_sdp {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn janus_sdp_generate_answer(
    _offer: *mut janus_sdp,
    _args: ...
) -> *mut janus_sdp {
    ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn janus_sdp_write(_sdp: *mut janus_sdp) -> *mut c_char {
    ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn janus_sdp_destroy(_sdp: *mut janus_sdp) {}
