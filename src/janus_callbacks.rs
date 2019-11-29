use std::os::raw::{c_char, c_int};

use failure::Error;
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
    // A parallel thread may have deleted the raw handle pointer while we want to send in a packet.
    // Skip this packet safely since the client has already gone away.
    match session.closing().read() {
        Ok(value) => {
            if *value {
                janus_warn!("[CONFERENCE] Skipping relaying an RTP packet to a closing session");
            } else {
                let callback = acquire_callbacks().relay_rtp;
                janus_info!("[CONFERENCE] About to dereference session (relay_rtp)");
                callback(session.as_ptr(), video, buf.as_mut_ptr(), buf.len() as i32);
            }
        }
        Err(err) => janus_err!(
            "[CONFERENCE] Failed to acquire closing flag mutex (relay_rtp): {}",
            err
        ),
    }
}

pub fn relay_rtcp(session: &Session, video: c_int, buf: &mut [i8]) {
    // A parallel thread may have deleted the raw handle pointer while we want to send in a packet.
    // Skip this packet safely since the client has already gone away.
    match session.closing().read() {
        Ok(value) => {
            if *value {
                janus_warn!("[CONFERENCE] Skipping relaying an RTCP packet to a closing session");
            } else {
                let callback = acquire_callbacks().relay_rtcp;
                janus_info!("[CONFERENCE] About to dereference session (relay_rtcp)");
                callback(session.as_ptr(), video, buf.as_mut_ptr(), buf.len() as i32);
            }
        }
        Err(err) => janus_err!(
            "[CONFERENCE] Failed to acquire closing flag mutex (relay_rtcp): {}",
            err
        ),
    }
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

    // There may be some responses in the queue while the handle pointer may be already deleted.
    // Skip this messages safely since the client has already gone away.
    match session.closing().read() {
        Ok(value) => {
            if *value {
                janus_warn!("[CONFERENCE] Skipping pushing an event to a closing session");
                return Ok(());
            } else {
                janus_info!("[CONFERENCE] About to dereference session (push_event)");
                let res = push_event_fn(session.as_ptr(), &mut PLUGIN, transaction, body, jsep);
                JanusError::from(res)
            }
        }
        Err(err) => {
            janus_err!(
                "[CONFERENCE] Failed to acquire closing flag mutex (push_event): {}",
                err
            );
            return Ok(());
        }
    }
}

fn unwrap_jansson_option_mut(val: Option<JanssonValue>) -> *mut RawJanssonValue {
    val.and_then(|val| Some(val.into_raw()))
        .unwrap_or(std::ptr::null_mut())
}

pub fn end_session(session: &Session) -> Result<(), Error> {
    // Mark the session closing to ensure we won't dereference raw handle pointer when
    // Janus might have deleted it and cause segfault.
    match session.closing().write() {
        Ok(mut closing) => *closing = true,
        Err(err) => bail!(
            "Failed to acquire closing flag mutex (end_session): {}",
            err
        ),
    }

    janus_info!("[CONFERENCE] About to dereference session (end_session)");
    (acquire_callbacks().end_session)(session.as_ptr());
    Ok(())
}
