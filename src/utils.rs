#![allow(unused_macros)]

use std::os::raw::{c_ulong, c_void};

use anyhow::{format_err, Context, Result};
use janus_plugin::{JanssonDecodingFlags, JanssonEncodingFlags, JanssonValue};
use serde::de::DeserializeOwned;
use serde_json::Value;

// Based on https://github.com/slog-rs/slog/blob/master/src/lib.rs#L750.
macro_rules! log(
    // `2` means that `;` was already found
    // -- print out the entry finally
    (2 @ { $($fmt:tt)* }, { }, $lvl:expr, $msg_fmt:expr) => {
        janus_plugin::janus_log!($lvl, "[CONFERENCE {{}}] {}", format_args!($msg_fmt, $($fmt)*))
    };
    (2 @ { $($fmt:tt)* }, { $($tags:tt)+ }, $lvl:expr, $msg_fmt:expr) => {
        janus_plugin::janus_log!(
            $lvl,
            "[CONFERENCE {}] {}",
            serde_json::json!($($tags)+),
            format_args!($msg_fmt, $($fmt)*)
        )
    };
    // -- collect tags
    (2 @ { $($fmt:tt)* }, { $($tags:tt)* }, $lvl:expr, $msg_fmt:expr,) => {
        log!(2 @ { $($fmt)* }, { $($tags)* }, $lvl, $msg_fmt)
    };
    (2 @ { $($fmt:tt)* }, { $($tags:tt)* }, $lvl:expr, $msg_fmt:expr;) => {
        log!(2 @ { $($fmt)* }, { $($tags)* }, $lvl, $msg_fmt)
    };
    (2 @ { $($fmt:tt)* }, { $($tags:tt)* }, $lvl:expr, $msg_fmt:expr, $($args:tt)*) => {
        log!(2 @ { $($fmt)* }, { $($tags)* $($args)* }, $lvl, $msg_fmt)
    };
    // `1` means that we are still looking for `;`
    // -- look for `;` termination
    (1 @ { $($fmt:tt)* }, { $($tags:tt)* }, $lvl:expr, $msg_fmt:expr,) => {
        log!(2 @ { $($fmt)* }, { $($tags)* }, $lvl, $msg_fmt)
    };
    (1 @ { $($fmt:tt)* }, { $($tags:tt)* }, $lvl:expr, $msg_fmt:expr) => {
        log!(2 @ { $($fmt)* }, { $($tags)* }, $lvl, $msg_fmt)
    };
    (1 @ { $($fmt:tt)* }, { $($tags:tt)* }, $lvl:expr, $msg_fmt:expr, ; $($args:tt)*) => {
        log!(1 @ { $($fmt)* }, { $($tags)* }, $lvl, $msg_fmt; $($args)*)
    };
    (1 @ { $($fmt:tt)* }, { $($tags:tt)* }, $lvl:expr, $msg_fmt:expr; $($args:tt)*) => {
        log!(2 @ { $($fmt)* }, { $($tags)* }, $lvl, $msg_fmt, $($args)*)
    };
    // -- must be normal argument to format string
    (1 @ { $($fmt:tt)* }, { $($tags:tt)* }, $lvl:expr, $msg_fmt:expr, $f:tt $($args:tt)*) => {
        log!(1 @ { $($fmt)* $f }, { $($tags)* }, $lvl, $msg_fmt, $($args)*)
    };
    ($lvl:expr, $($args:tt)*) => {
        log!(1 @ { }, { }, $lvl, $($args)*)
    };
);

macro_rules! fatal(($($args:tt)*) => { log!(janus_plugin::debug::LogLevel::Fatal, $($args)*) };);
macro_rules! err(($($args:tt)*) => { log!(janus_plugin::debug::LogLevel::Err, $($args)*) };);
macro_rules! warn(($($args:tt)*) => { log!(janus_plugin::debug::LogLevel::Warn, $($args)*) };);
macro_rules! info(($($args:tt)*) => { log!(janus_plugin::debug::LogLevel::Info, $($args)*) };);
macro_rules! verb(($($args:tt)*) => { log!(janus_plugin::debug::LogLevel::Verb, $($args)*) };);
macro_rules! huge(($($args:tt)*) => { log!(janus_plugin::debug::LogLevel::Huge, $($args)*) };);
macro_rules! dbg(($($args:tt)*) => { log!(janus_plugin::debug::LogLevel::Dbg, $($args)*) };);

////////////////////////////////////////////////////////////////////////////////

// Courtesy of c_string crate, which also has some other stuff we aren't interested in
// taking in as a dependency here.
macro_rules! c_str {
    ($lit:expr) => {
        unsafe { CStr::from_ptr(concat!($lit, "\0").as_ptr() as *const $crate::c_char) }
    };
}

pub fn serde_to_jansson(json: &Value) -> Result<JanssonValue> {
    JanssonValue::from_str(&json.to_string(), JanssonDecodingFlags::empty())
        .map_err(|err| format_err!("{}", err))
}

pub fn jansson_to_serde<T: DeserializeOwned>(json: &JanssonValue) -> Result<T> {
    let json = json.to_libcstring(JanssonEncodingFlags::empty());
    let json = json.to_string_lossy();
    serde_json::from_str(&json).context("Failed to parse JSON")
}

pub fn retry_failed<T, E: std::fmt::Debug>(r: Option<Result<&T, &E>>) -> bool {
    if let Some(Err(e)) = r {
        err!("Request failed: {:?}", e);
        true
    } else {
        false
    }
}

////////////////////////////////////////////////////////////////////////////////

#[allow(non_camel_case_types)]
#[repr(C)]
pub struct janus_ice_handle {
    _session: *mut c_void,
    pub handle_id: c_ulong,
}
