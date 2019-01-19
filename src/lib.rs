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
use std::sync::{mpsc, Arc, RwLock};
use std::thread;

use atom::AtomSetOnce;
use failure::Error;
use janus::{
    sdp::{self, OfferAnswerParameters},
    JanssonValue, LibraryMetadata, Plugin, PluginCallbacks, PluginResult, PluginSession,
    RawJanssonValue, RawPluginResult,
};

mod bidirectional_multimap;
mod janus_callbacks;
mod messages;
mod session;
mod switchboard;
#[macro_use]
mod utils;

use messages::{APIError, ErrorKind, JsepKind, StreamOperation, ToAPIError};
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

const AUDIO_CODEC: sdp::AudioCodec = sdp::AudioCodec::Opus;
const VIDEO_CODEC: sdp::VideoCodec = sdp::VideoCodec::H264;

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
        let mut seq = publisher.as_ref().incr_fir_seq() as i32;
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

                    let event = match err.kind {
                        ErrorKind::Internal => json!({
                            "success": false,
                            "error": {
                                "kind": err.kind,
                                "detail": "Contact developers"
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

            PluginResult::ok_wait(None).into_raw()
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

            let (event, jsep) = match message {
                StreamOperation::Create { id } => {
                    switchboard.create_stream(id, received.session.clone());

                    let answer = handle_jsep(&received.jsep)
                        .map_err(|err| err.to_bad_request("Invalid SDP"))?;

                    let offer = generate_subsciber_offer(&answer);

                    let event = json!({
                        "success": true,
                        "offer": offer.to_glibstring().to_string_lossy().to_string()
                    });

                    received
                        .session
                        .set_subscriber_offer(offer)
                        .map_err(|err| err.to_internal())?;

                    let answer = answer.to_glibstring().to_string_lossy().to_string();
                    let jsep = serde_json::to_value(JsepKind::Answer { sdp: answer })
                        .map_err(|err| Error::from(err).to_internal())?;
                    let jsep = utils::serde_to_jansson(&jsep).map_err(|err| err.to_internal())?;

                    (event, Some(jsep))
                }
                StreamOperation::Read { id } => {
                    switchboard
                        .join_stream(&id, received.session.clone())
                        .map_err(|err| err.to_non_existent_stream(id))?;

                    (success_event, None)
                }
            };

            let event = utils::serde_to_jansson(&event).map_err(|err| err.to_internal())?;

            Ok((Some(event), jsep))
        }
        None => Ok((None, None)),
    }
}

fn generate_subsciber_offer(answer_to_publisher: &sdp::Sdp) -> sdp::Sdp {
    let audio_payload_type = answer_to_publisher.get_payload_type(AUDIO_CODEC.to_cstr());
    let video_payload_type = answer_to_publisher.get_payload_type(VIDEO_CODEC.to_cstr());

    offer_sdp!(
        std::ptr::null(),
        answer_to_publisher.c_addr as *const _,
        OfferAnswerParameters::Audio,
        1,
        OfferAnswerParameters::AudioCodec,
        AUDIO_CODEC.to_cstr().as_ptr(),
        OfferAnswerParameters::AudioPayloadType,
        audio_payload_type.unwrap_or(111),
        OfferAnswerParameters::AudioDirection,
        sdp::MediaDirection::JANUS_SDP_SENDONLY,
        OfferAnswerParameters::Video,
        1,
        OfferAnswerParameters::VideoCodec,
        VIDEO_CODEC.to_cstr().as_ptr(),
        OfferAnswerParameters::VideoPayloadType,
        video_payload_type.unwrap_or(96),
        OfferAnswerParameters::VideoDirection,
        sdp::MediaDirection::JANUS_SDP_SENDONLY
    )
}

fn handle_jsep(jsep: &Option<JanssonValue>) -> Result<sdp::Sdp, Error> {
    match jsep {
        Some(jsep) => {
            let jsep_json: JsepKind = utils::jansson_to_serde(jsep)?;

            let response = match jsep_json {
                JsepKind::Offer { sdp } => {
                    let offer = sdp::Sdp::parse(&CString::new(sdp)?)?;
                    janus_verb!("[CONFERENCE] offer: {:?}", offer);

                    let mut answer = answer_sdp!(
                        offer,
                        OfferAnswerParameters::AudioCodec,
                        AUDIO_CODEC.to_cstr().as_ptr(),
                        OfferAnswerParameters::VideoCodec,
                        VIDEO_CODEC.to_cstr().as_ptr()
                    );
                    janus_verb!("[CONFERENCE] answer: {:?}", answer);

                    answer
                }
                JsepKind::Answer { .. } => unreachable!(),
            };

            Ok(response)
        }
        None => Err(failure::err_msg("JSEP is empty")),
    }
}
