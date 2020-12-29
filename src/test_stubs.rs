#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int, c_uint};
/// This modules defines stubs for functions from janus-plugin-sys crate to enable linking when
/// compiling for running unit tests.
use std::ptr;

use jansson_sys::json_t;
use janus_plugin_sys::plugin::{janus_plugin_result, janus_plugin_result_type};
use janus_plugin_sys::sdp::janus_sdp;

type gboolean = c_int;
type uint32_t = c_uint;

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

#[no_mangle]
pub extern "C" fn janus_rtcp_has_fir(_packet: *mut c_char, _len: c_int) -> gboolean {
    0
}

#[no_mangle]
pub extern "C" fn janus_rtcp_has_pli(_packet: *mut c_char, _len: c_int) -> gboolean {
    0
}

#[no_mangle]
pub extern "C" fn janus_rtcp_fir(_packet: *mut c_char, _len: c_int, _seqnr: *mut c_int) -> c_int {
    0
}

#[no_mangle]
pub extern "C" fn janus_rtcp_pli(_packet: *mut c_char, _len: c_int) -> c_int {
    0
}

#[no_mangle]
pub extern "C" fn janus_rtcp_remb(_packet: *mut c_char, _len: c_int, _bitrate: uint32_t) -> c_int {
    0
}

#[no_mangle]
pub extern "C" fn janus_plugin_result_new(
    _type: janus_plugin_result_type,
    _text: *const c_char,
    _content: *mut json_t,
) -> *mut janus_plugin_result {
    ptr::null_mut()
}

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
