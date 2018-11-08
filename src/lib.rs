#[macro_use]
extern crate janus_plugin as janus;

use janus::{
    JanssonDecodingFlags, JanssonEncodingFlags, JanssonValue, JanusError, JanusResult,
    LibraryMetadata, Plugin, PluginCallbacks, PluginResult, PluginSession, RawJanssonValue,
    RawPluginResult, SessionWrapper,
};

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

// courtesy of c_string crate, which also has some other stuff we aren't interested in
// taking in as a dependency here.
macro_rules! c_str {
    ($lit:expr) => {
        unsafe { CStr::from_ptr(concat!($lit, "\0").as_ptr() as *const $crate::c_char) }
    };
}

// TODO: move CALLBACKS definition, initialization and wrappers to separate mod
static mut CALLBACKS: Option<&PluginCallbacks> = None;

fn acquire_callbacks() -> &'static PluginCallbacks {
    unsafe { CALLBACKS }.expect("Gateway is not set")
}

fn relay_rtp(handle: *mut PluginSession, video: c_int, buf: *mut c_char, len: c_int) {
    (acquire_callbacks().relay_rtp)(handle, video, buf, len);
}

fn relay_rtcp(handle: *mut PluginSession, video: c_int, buf: *mut c_char, len: c_int) {
    (acquire_callbacks().relay_rtcp)(handle, video, buf, len);
}

fn relay_data(handle: *mut PluginSession, buf: *mut c_char, len: c_int) {
    (acquire_callbacks().relay_data)(handle, buf, len);
}

#[derive(Debug)]
struct State;

type Session = SessionWrapper<State>;

extern "C" fn init(callbacks: *mut PluginCallbacks, config_path: *const c_char) -> c_int {
    unsafe {
        let callbacks = callbacks
            .as_ref()
            .expect("Invalid callbacks ptr from Janus Core");
        CALLBACKS = Some(callbacks);
    }

    janus_info!("[CONFERENCE] Janus Conference plugin initialized!");

    0
}

extern "C" fn destroy() {
    janus_info!("[CONFERENCE] Janus Conference plugin destroyed!");
}

extern "C" fn create_session(handle: *mut PluginSession, error: *mut c_int) {
    let state = State {};

    match unsafe { Session::associate(handle, state) } {
        Ok(sess) => {
            janus_info!(
                "[CONFERENCE] Initializing Conference session {:p}...",
                sess.handle
            );
        }
        Err(e) => {
            janus_err!("[CONFERENCE] {}", e);
            unsafe { *error = -1 };
        }
    }
}

extern "C" fn destroy_session(handle: *mut PluginSession, error: *mut c_int) {
    janus_info!("[CONFERENCE] Destroying Conference session...");

    match unsafe { Session::from_ptr(handle) } {
        Ok(sess) => {
            janus_info!(
                "[CONFERENCE] Destroying Conference session {:p}...",
                sess.handle
            );
        }
        Err(e) => {
            janus_err!("[CONFERENCE] {}", e);
            unsafe { *error = -1 };
        }
    }
}

extern "C" fn query_session(_handle: *mut PluginSession) -> *mut RawJanssonValue {
    let val = "{}".to_owned();
    JanssonValue::from_str(&val, JanssonDecodingFlags::empty())
        .unwrap()
        .into_raw()
}

extern "C" fn handle_message(
    handle: *mut PluginSession,
    transaction: *mut c_char,
    message: *mut RawJanssonValue,
    jsep: *mut RawJanssonValue,
) -> *mut RawPluginResult {
    let result = match unsafe { Session::from_ptr(handle) } {
        Ok(sess) => {
            janus_info!(
                "[CONFERENCE] Ignoring signalling message on {:p}.",
                sess.handle
            );
            PluginResult::ok_wait(Some(c_str!("Ignored")))
        }
        Err(_) => PluginResult::error(c_str!("No handle associated with message!")),
    };
    result.into_raw()
}

extern "C" fn setup_media(handle: *mut PluginSession) {
    let sess = unsafe { Session::from_ptr(handle).expect("Session can't be null!") };
    janus_info!(
        "[CONFERENCE] WebRTC media is now available on {:p}.",
        sess.handle
    );
}

extern "C" fn hangup_media(handle: *mut PluginSession) {
    let sess = unsafe { Session::from_ptr(handle).expect("Session can't be null!") };
    janus_info!("[CONFERENCE] Hanging up WebRTC media on {:p}.", sess.handle);
}

extern "C" fn incoming_rtp(handle: *mut PluginSession, video: c_int, buf: *mut c_char, len: c_int) {
    relay_rtp(handle, video, buf, len);
}

extern "C" fn incoming_rtcp(
    handle: *mut PluginSession,
    video: c_int,
    buf: *mut c_char,
    len: c_int,
) {
    relay_rtcp(handle, video, buf, len);
}

extern "C" fn incoming_data(handle: *mut PluginSession, buf: *mut c_char, len: c_int) {
    relay_data(handle, buf, len);
}

extern "C" fn slow_link(handle: *mut PluginSession, _uplink: c_int, _video: c_int) {}

const PLUGIN: Plugin = build_plugin!(
    LibraryMetadata {
        api_version: 10,
        version: 1,
        name: c_str!("Janus Conference plugin"),
        package: c_str!("janus.plugin.conference"),
        version_str: c_str!(env!("CARGO_PKG_VERSION")),
        description: c_str!(env!("CARGO_PKG_DESCRIPTION")),
        author: c_str!(env!("CARGO_PKG_AUTHORS")),
    },
    init,
    destroy,
    create_session,
    handle_message,
    setup_media,
    incoming_rtp,
    incoming_rtcp,
    incoming_data,
    slow_link,
    hangup_media,
    destroy_session,
    query_session
);

export_plugin!(&PLUGIN);
