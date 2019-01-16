#[macro_use]
extern crate janus_plugin as janus;

extern crate serde;
#[macro_use]
extern crate serde_json;
#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate lazy_static;
extern crate atom;
extern crate multimap;

#[macro_use]
extern crate failure;

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::slice;
use std::sync::{atomic::Ordering, mpsc, Arc, RwLock};
use std::thread;

use atom::AtomSetOnce;
use failure::Error;
use janus::{
    sdp, JanssonValue, LibraryMetadata, Plugin, PluginCallbacks, PluginResult, PluginSession,
    RawJanssonValue, RawPluginResult,
};

mod bidirectional_multimap;
mod janus_callbacks;
mod messages;
mod session;
mod switchboard;
#[macro_use]
mod utils;

use messages::{APIError, ErrorStatus, JsepKind, StreamOperation, ToAPIError};
use session::{Session, SessionState};
use switchboard::Switchboard;

#[derive(Debug)]
struct Message {
    session: Arc<Session>,
    transaction: *mut c_char,
    message: Option<JanssonValue>,
    jsep: Option<JanssonValue>,
}

unsafe impl Send for Message {}

#[derive(Debug)]
struct State {
    pub message_channel: AtomSetOnce<Box<mpsc::SyncSender<Message>>>,
    pub switchboard: RwLock<Switchboard>,
}

lazy_static! {
    static ref STATE: State = State {
        message_channel: AtomSetOnce::empty(),
        switchboard: RwLock::new(Switchboard::new()),
    };
}

fn send_pli<T: IntoIterator<Item = U>, U: AsRef<Session>>(publishers: T) {
    for publisher in publishers {
        let mut pli = janus::rtcp::gen_pli();
        janus_callbacks::relay_rtcp(
            publisher.as_ref().as_ptr(),
            1,
            pli.as_mut_ptr(),
            pli.len() as i32,
        );
    }
}

fn send_fir<T: IntoIterator<Item = U>, U: AsRef<Session>>(publishers: T) {
    for publisher in publishers {
        let mut seq = publisher.as_ref().fir_seq.fetch_add(1, Ordering::Relaxed) as i32;
        let mut fir = janus::rtcp::gen_fir(&mut seq);
        janus_callbacks::relay_rtcp(
            publisher.as_ref().as_ptr(),
            1,
            fir.as_mut_ptr(),
            fir.len() as i32,
        );
    }
}

fn report_error(res: Result<(), Error>) {
    match res {
        Ok(_) => {}
        Err(err) => {
            janus_err!("[CONFERENCE] {}", err);
        }
    }
}

extern "C" fn init(callbacks: *mut PluginCallbacks, _config_path: *const c_char) -> c_int {
    janus_callbacks::init(callbacks);

    let (messages_tx, messages_rx) = mpsc::sync_channel(10);

    STATE.message_channel.set_if_none(Box::new(messages_tx));

    thread::spawn(move || {
        janus_info!("[CONFERENCE] Message processing thread is alive.");
        for msg in messages_rx.iter() {
            let push_result = match handle_message_async(&msg) {
                Ok((event, jsep)) => {
                    janus_callbacks::push_event(msg.session.handle, msg.transaction, event, jsep)
                        .map_err(Error::from)
                }
                Err(err) => {
                    janus_err!("Error processing message: {}", err);

                    let event = match err.status {
                        ErrorStatus::Internal => json!({
                            "success": false,
                            "error": {
                                "status": err.status,
                                "title": "Internal error - please, contact developers"
                            }
                        }),
                        _ => json!({
                            "success": false,
                            "error": err
                        }),
                    };

                    utils::serde_to_jansson(&event).and_then(|event| {
                        janus_callbacks::push_event(
                            msg.session.handle,
                            msg.transaction,
                            Some(event),
                            None,
                        )
                        .map_err(Error::from)
                    })
                }
            };

            if let Err(err) = push_result {
                janus_err!("Error pushing event: {}", err);
            }
        }
    });

    janus_info!("[CONFERENCE] Janus Conference plugin initialized!");

    0
}

extern "C" fn destroy() {
    janus_info!("[CONFERENCE] Janus Conference plugin destroyed!");
}

extern "C" fn create_session(handle: *mut PluginSession, error: *mut c_int) {
    let initial_state = SessionState::new();

    match unsafe { Session::associate(handle, initial_state) } {
        Ok(sess) => {
            janus_info!("[CONFERENCE] Initializing session {:p}...", sess.handle);
            let mut switchboard = STATE.switchboard.write();

            match switchboard {
                Ok(mut switchboard) => {
                    switchboard.connect(sess);
                }
                Err(err) => {
                    janus_err!("[CONFERENCE] {}", err);
                    unsafe {
                        *error = -1;
                    }
                }
            }
        }
        Err(e) => {
            janus_err!("[CONFERENCE] {}", e);
            unsafe {
                *error = -1;
            }
        }
    }
}

extern "C" fn destroy_session(handle: *mut PluginSession, error: *mut c_int) {
    match unsafe { Session::from_ptr(handle) } {
        Ok(sess) => {
            janus_info!(
                "[CONFERENCE] Destroying Conference session {:p}...",
                sess.handle
            );

            let mut switchboard = STATE.switchboard.write();

            match switchboard {
                Ok(mut switchboard) => {
                    switchboard.disconnect(&sess);
                }
                Err(err) => {
                    janus_err!("[CONFERENCE] {}", err);
                    unsafe {
                        *error = -1;
                    }
                }
            }
        }
        Err(e) => {
            janus_err!("{}", e);
            unsafe {
                *error = -1;
            }
        }
    }
}

extern "C" fn query_session(_handle: *mut PluginSession) -> *mut RawJanssonValue {
    janus_info!("[CONFERENCE] Querying session...");
    std::ptr::null_mut()
}

extern "C" fn handle_message(
    handle: *mut PluginSession,
    transaction: *mut c_char,
    message: *mut RawJanssonValue,
    jsep: *mut RawJanssonValue,
) -> *mut RawPluginResult {
    janus_info!("[CONFERENCE] Queueing message on {:p}.", handle);

    match unsafe { Session::from_ptr(handle) } {
        Ok(sess) => {
            let msg = Message {
                session: sess,
                transaction,
                message: unsafe { JanssonValue::from_raw(message) },
                jsep: unsafe { JanssonValue::from_raw(jsep) },
            };

            STATE.message_channel.get().and_then(|ch| ch.send(msg).ok());

            PluginResult::ok_wait(Some(c_str!("Processing..."))).into_raw()
        }
        Err(e) => {
            janus_err!("[CONFERENCE] Failed to restore session state: {}", e);

            PluginResult::error(c_str!("Failed to restore session state")).into_raw()
        }
    }
}

fn setup_media_impl(handle: *mut PluginSession) -> Result<(), Error> {
    let sess = unsafe { Session::from_ptr(handle)? };
    let switchboard = STATE
        .switchboard
        .read()
        .map_err(|err| format_err!("{}", err))?;

    send_fir(switchboard.publisher_to(&sess));

    janus_info!(
        "[CONFERENCE] WebRTC media is now available on {:p}.",
        handle
    );

    Ok(())
}

extern "C" fn setup_media(handle: *mut PluginSession) {
    report_error(setup_media_impl(handle));
}

extern "C" fn hangup_media(handle: *mut PluginSession) {
    janus_info!("[CONFERENCE] Hanging up WebRTC media on {:p}.", handle);
}

fn incoming_rtp_impl(
    handle: *mut PluginSession,
    video: c_int,
    buf: *mut c_char,
    len: c_int,
) -> Result<(), Error> {
    let sess = unsafe { Session::from_ptr(handle)? };
    let switchboard = STATE
        .switchboard
        .read()
        .map_err(|err| format_err!("{}", err))?;

    for subscriber in switchboard.subscribers_to(&sess) {
        janus_callbacks::relay_rtp(subscriber.as_ptr(), video, buf, len);
    }

    Ok(())
}

extern "C" fn incoming_rtp(handle: *mut PluginSession, video: c_int, buf: *mut c_char, len: c_int) {
    report_error(incoming_rtp_impl(handle, video, buf, len));
}

fn incoming_rtcp_impl(
    handle: *mut PluginSession,
    video: c_int,
    buf: *mut c_char,
    len: c_int,
) -> Result<(), Error> {
    let sess = unsafe { Session::from_ptr(handle)? };

    let switchboard = STATE
        .switchboard
        .read()
        .map_err(|err| format_err!("{}", err))?;

    let packet = unsafe { slice::from_raw_parts(buf, len as usize) };

    match video {
        1 if janus::rtcp::has_pli(packet) => {
            send_pli(switchboard.publisher_to(&sess));
        }
        1 if janus::rtcp::has_fir(packet) => {
            send_fir(switchboard.publisher_to(&sess));
        }
        _ => {
            for subscriber in switchboard.subscribers_to(&sess) {
                janus_callbacks::relay_rtcp(subscriber.as_ptr(), video, buf, len);
            }
        }
    }

    Ok(())
}

extern "C" fn incoming_rtcp(
    handle: *mut PluginSession,
    video: c_int,
    buf: *mut c_char,
    len: c_int,
) {
    report_error(incoming_rtcp_impl(handle, video, buf, len));
}

extern "C" fn incoming_data(_handle: *mut PluginSession, _buf: *mut c_char, _len: c_int) {
    // Dropping incoming data.
}

extern "C" fn slow_link(_handle: *mut PluginSession, _uplink: c_int, _video: c_int) {
    janus_info!("[CONFERENCE] slow link callback")
}

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

fn handle_message_async(
    received: &Message,
) -> Result<(Option<JanssonValue>, Option<JanssonValue>), APIError> {
    let success_event = json!({ "success": true });

    match received.message {
        Some(ref message) => {
            let message: StreamOperation = utils::jansson_to_serde(&message).map_err(|err| {
                err.to_bad_request("Unknown method name or invalid method parameters")
            })?;

            let mut switchboard = STATE.switchboard.write().map_err(|err| {
                let err = format_err!("{}", err);
                err.to_internal()
            })?;

            let jsep = match message {
                StreamOperation::Create { .. } | StreamOperation::Read { .. } => {
                    let jsep = handle_jsep(&received.jsep)
                        .map_err(|err| err.to_bad_request("Invalid SDP"))?;

                    Some(jsep)
                }
            };

            let event = match message {
                StreamOperation::Create { id } => {
                    switchboard.create_room(id, received.session.clone());
                    success_event
                }
                StreamOperation::Read { id } => {
                    switchboard
                        .join_room(&id, received.session.clone())
                        .map_err(|err| err.to_non_existent_room(id))?;
                    success_event
                }
            };

            let event = utils::serde_to_jansson(&event).map_err(|err| err.to_internal())?;

            Ok((Some(event), jsep))
        }
        None => Ok((None, None)),
    }
}

fn handle_jsep(jsep: &Option<JanssonValue>) -> Result<JanssonValue, Error> {
    match jsep {
        Some(jsep) => {
            let jsep_json: JsepKind = utils::jansson_to_serde(jsep)?;

            let response = match jsep_json {
                JsepKind::Offer { sdp } => {
                    let offer = sdp::Sdp::parse(&CString::new(sdp)?)?;
                    janus_verb!("[CONFERENCE] offer: {:?}", offer);

                    let mut answer = answer_sdp!(
                        offer,
                        sdp::OfferAnswerParameters::AudioCodec,
                        sdp::AudioCodec::Opus.to_cstr().as_ptr(),
                        sdp::OfferAnswerParameters::VideoCodec,
                        sdp::VideoCodec::H264.to_cstr().as_ptr()
                    );
                    janus_verb!("[CONFERENCE] answer: {:?}", answer);

                    let answer = answer.to_glibstring().to_string_lossy().to_string();
                    JsepKind::Answer { sdp: answer }
                }
                JsepKind::Answer { .. } => unreachable!(),
            };

            let response = serde_json::to_value(response)?;
            let response = utils::serde_to_jansson(&response)?;

            Ok(response)
        }
        None => Err(failure::err_msg("JSEP is empty")),
    }
}
