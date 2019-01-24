use std::os::raw::{c_char, c_int};

use janus::{
    JanssonValue, JanusError, JanusResult, PluginCallbacks, PluginSession, RawJanssonValue,
};

use super::PLUGIN;

static mut CALLBACKS: Option<&PluginCallbacks> = None;

pub fn init(callbacks: *mut PluginCallbacks) {
    unsafe {
        let callbacks = callbacks
            .as_ref()
            .expect("Invalid callbacks ptr from Janus Core");
        CALLBACKS = Some(callbacks);
    }
}

fn acquire_callbacks() -> &'static PluginCallbacks {
    unsafe { CALLBACKS }.expect("Gateway is not set")
}

pub fn relay_rtp(handle: *mut PluginSession, video: c_int, buf: *mut c_char, len: c_int) {
    (acquire_callbacks().relay_rtp)(handle, video, buf, len);
}

pub fn relay_rtcp(handle: *mut PluginSession, video: c_int, buf: *mut c_char, len: c_int) {
    (acquire_callbacks().relay_rtcp)(handle, video, buf, len);
}

pub fn push_event(
    handle: *mut PluginSession,
    transaction: *mut c_char,
    body: Option<JanssonValue>,
    jsep: Option<JanssonValue>,
) -> JanusResult {
    let push_event_fn = acquire_callbacks().push_event;

    let body = unwrap_jansson_option_mut(body);
    let jsep = unwrap_jansson_option_mut(jsep);

    let res = push_event_fn(handle, &mut PLUGIN, transaction, body, jsep);

    JanusError::from(res)
}

fn unwrap_jansson_option_mut(val: Option<JanssonValue>) -> *mut RawJanssonValue {
    val.and_then(|val| Some(val.into_raw()))
        .unwrap_or(std::ptr::null_mut())
}
