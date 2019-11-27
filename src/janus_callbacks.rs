use std::os::raw::{c_char, c_int};

use janus::{JanssonValue, JanusError, JanusResult, PluginCallbacks, RawJanssonValue};

use super::PLUGIN;
use crate::session::Session;

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

pub fn relay_rtp(session: &Session, video: c_int, buf: &mut [i8]) {
    (acquire_callbacks().relay_rtp)(session.as_ptr(), video, buf.as_mut_ptr(), buf.len() as i32);
}

pub fn relay_rtcp(session: &Session, video: c_int, buf: &mut [i8]) {
    (acquire_callbacks().relay_rtcp)(session.as_ptr(), video, buf.as_mut_ptr(), buf.len() as i32);
}

pub fn push_event(
    session: &Session,
    transaction: *mut c_char,
    body: Option<JanssonValue>,
    jsep: Option<JanssonValue>,
) -> JanusResult {
    let push_event_fn = acquire_callbacks().push_event;

    let body = unwrap_jansson_option_mut(body);
    let jsep = unwrap_jansson_option_mut(jsep);

    let res = push_event_fn(session.as_ptr(), &mut PLUGIN, transaction, body, jsep);

    JanusError::from(res)
}

fn unwrap_jansson_option_mut(val: Option<JanssonValue>) -> *mut RawJanssonValue {
    val.and_then(|val| Some(val.into_raw()))
        .unwrap_or(std::ptr::null_mut())
}

// pub fn close_pc(session: &Session) {
//     (acquire_callbacks().close_pc)(session.as_ptr());
// }

pub fn end_session(session: &Session) {
    (acquire_callbacks().end_session)(session.as_ptr());
}
