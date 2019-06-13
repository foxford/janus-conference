#[macro_use]
extern crate janus_plugin as janus;

extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate config;
extern crate http;

#[macro_use]
extern crate lazy_static;
extern crate atom;
extern crate multimap;

extern crate glib;
extern crate gstreamer;
extern crate gstreamer_app;
extern crate gstreamer_base;
extern crate gstreamer_pbutils;

extern crate rusoto_core;
extern crate rusoto_s3;
extern crate s4;

#[macro_use]
extern crate failure;
extern crate futures;
extern crate tokio_threadpool;

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::slice;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, SystemTime};

use atom::AtomSetOnce;
use failure::{err_msg, Error};
use janus::{
    JanssonDecodingFlags, JanssonEncodingFlags, JanssonValue, LibraryMetadata, Plugin,
    PluginCallbacks, PluginResult, PluginSession, RawJanssonValue, RawPluginResult,
};

mod bidirectional_multimap;
mod conf;
mod janus_callbacks;
mod message_handler;
mod messages;
mod recorder;
mod session;
mod switchboard;
#[macro_use]
mod utils;
mod codecs;
mod gst_elements;
mod uploader;

use conf::Config;
use message_handler::MessageHandler;
use messages::{APIError, ErrorStatus, StreamOperation};
use recorder::Recorder;
use session::{Session, SessionState};

#[derive(Clone, Debug)]
pub struct Message {
    session: Arc<Session>,
    transaction: String,
    operation: Option<StreamOperation>,
    jsep: Option<String>,
}

unsafe impl Send for Message {}

pub enum Event {
    Request(Message),
    Response {
        msg: Message,
        response: Option<JanssonValue>,
        jsep: Option<JanssonValue>,
    },
}

pub type ConcreteRecorder = recorder::RecorderImpl<codecs::H264, codecs::OPUS>;

lazy_static! {
    static ref MESSAGE_HANDLER: AtomSetOnce<Box<MessageHandler>> = AtomSetOnce::empty();
}

fn send_pli<T: IntoIterator<Item = U>, U: AsRef<Session>>(publishers: T) {
    for publisher in publishers {
        let mut pli = janus::rtcp::gen_pli();
        janus_callbacks::relay_rtcp(publisher.as_ref(), 1, &mut pli);
    }
}

fn send_fir<T: IntoIterator<Item = U>, U: AsRef<Session>>(publishers: T) {
    for publisher in publishers {
        let mut seq = publisher.as_ref().incr_fir_seq() as i32;
        let mut fir = janus::rtcp::gen_fir(&mut seq);
        janus_callbacks::relay_rtcp(publisher.as_ref(), 1, &mut fir);
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

fn init_config(config_path: *const c_char) -> Result<Config, Error> {
    let config_path = unsafe { CStr::from_ptr(config_path) };
    let config_path = config_path.to_str()?;
    let config_path = Path::new(config_path);

    Ok(Config::from_path(config_path)?)
}

extern "C" fn init(callbacks: *mut PluginCallbacks, config_path: *const c_char) -> c_int {
    let config = match init_config(config_path) {
        Ok(config) => {
            janus_info!("{:?}", config);
            config
        }
        Err(err) => {
            janus_fatal!("[CONFERENCE] Failed to read config: {}", err);
            return -1;
        }
    };

    let (tx, rx) = mpsc::sync_channel(10);

    match MessageHandler::new(config.clone(), tx) {
        Ok(message_handler) => {
            MESSAGE_HANDLER.set_if_none(Box::new(message_handler));
        }
        Err(err) => {
            janus_fatal!("[CONFERENCE] Message handler init failed: {}", err);
            return -1;
        }
    };

    janus_callbacks::init(callbacks);

    thread::spawn(move || {
        janus_info!("[CONFERENCE] Message processing thread is alive.");

        let message_handler = match MESSAGE_HANDLER.get() {
            Some(message_handler) => message_handler,
            None => {
                janus_fatal!("[CONFERENCE] Message handler not initialized");
                return;
            }
        };

        for item in rx.iter() {
            match item {
                Event::Request(msg) => {
                    janus_info!("[CONFERENCE] Handling request ({})", msg.transaction);
                    message_handler.handle(&msg)
                }
                Event::Response {
                    msg,
                    response,
                    jsep,
                } => {
                    janus_info!("[CONFERENCE] Sending response ({})", msg.transaction);

                    let push_result = CString::new(msg.transaction.clone()).map(|transaction| {
                        janus_callbacks::push_event(
                            &msg.session,
                            transaction.into_raw(),
                            response,
                            jsep,
                        )
                    });

                    if let Err(err) = push_result {
                        janus_err!("[CONFERENCE] Error pushing event: {}", err);
                    }
                }
            }
        }
    });

    let res = gstreamer::init();
    if let Err(err) = res {
        janus_fatal!("[CONFERENCE] Failed to init GStreamer: {}", err);
        return -1;
    }

    let interval = Duration::new(config.general.vacuum_interval, 0);

    thread::spawn(move || loop {
        thread::sleep(interval);

        report_error(
            MESSAGE_HANDLER
                .get()
                .ok_or_else(|| err_msg("Message handler not initialized"))
                .and_then(|message_handler| {
                    message_handler
                        .switchboard
                        .read()
                        .map_err(|_| err_msg("Failed to acquire switchboard read lock"))
                })
                .map(|switchboard| switchboard.vacuum_publishers(&interval)),
        );
    });

    janus_info!("[CONFERENCE] Janus Conference plugin initialized!");
    0
}

extern "C" fn destroy() {
    janus_info!("[CONFERENCE] Janus Conference plugin destroyed!");
}

extern "C" fn create_session(handle: *mut PluginSession, error: *mut c_int) {
    let initial_state = SessionState::new();

    let message_handler = match MESSAGE_HANDLER.get() {
        Some(message_handler) => message_handler,
        None => {
            janus_err!("[CONFERENCE] Message handler is not initialized");
            return;
        }
    };

    match unsafe { Session::associate(handle, initial_state) } {
        Ok(sess) => {
            janus_info!("[CONFERENCE] Initializing session {:p}...", sess.handle);
            let mut switchboard = message_handler.switchboard.write();

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

extern "C" fn destroy_session(handle: *mut PluginSession, _error: *mut c_int) {
    janus_info!("[CONFERENCE] Destroying Conference session {:p}...", handle);
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

    let message_handler = match MESSAGE_HANDLER.get() {
        Some(message_handler) => message_handler,
        None => {
            janus_err!("[CONFERENCE] Message handler not initialized");
            return PluginResult::error(c_str!("Message handler not initialized")).into_raw();
        }
    };

    let session = match unsafe { Session::from_ptr(handle) } {
        Ok(session) => session,
        Err(err) => {
            janus_err!("[CONFERENCE] Failed to restore session state: {}", err);
            return PluginResult::error(c_str!("Failed to restore session state")).into_raw();
        }
    };

    let transaction = match unsafe { CString::from_raw(transaction) }.to_str() {
        Ok(transaction) => String::from(transaction),
        Err(err) => {
            janus_err!("[CONFERENCE] Failed to serialize transaction: {}", err);
            return PluginResult::error(c_str!("Failed serialize transaction")).into_raw();
        }
    };

    // TODO: Suboptimal serialization to String for making Message thread safe.
    let jsep = match unsafe { JanssonValue::from_raw(jsep) } {
        None => None,
        Some(jsep) => match jsep.to_libcstring(JanssonEncodingFlags::empty()).to_str() {
            Ok(jsep) => Some(String::from(jsep)),
            Err(err) => {
                janus_err!("[CONFERENCE] Failed to serialize JSEP: {}", err);
                return PluginResult::error(c_str!("Failed serialize JSEP")).into_raw();
            }
        },
    };

    unsafe { JanssonValue::from_raw(message) }.map(|message| {
        match utils::jansson_to_serde(&message) {
            Ok(operation) => {
                let msg = Message {
                    session,
                    transaction,
                    operation: Some(operation),
                    jsep,
                };

                message_handler.tx.send(Event::Request(msg)).ok();
            }
            Err(err) => {
                let msg = Message {
                    session,
                    transaction,
                    operation: None,
                    jsep: None,
                };

                let err = APIError::new(ErrorStatus::BAD_REQUEST, err, &None);
                message_handler.respond(&msg, Err(err), None);
            }
        };
    });

    PluginResult::ok_wait(None).into_raw()
}

extern "C" fn handle_admin_message(_message: *mut RawJanssonValue) -> *mut RawJanssonValue {
    JanssonValue::from_str("{}", JanssonDecodingFlags::empty())
        .expect("Failed to decode JSON")
        .into_raw()
}

fn setup_media_impl(handle: *mut PluginSession) -> Result<(), Error> {
    let sess = unsafe { Session::from_ptr(handle)? };

    let switchboard = MESSAGE_HANDLER
        .get()
        .ok_or_else(|| err_msg("Message handler not initialized"))
        .and_then(|message_handler| {
            message_handler
                .switchboard
                .read()
                .map_err(|_| err_msg("Failed to acquire switchboard read lock"))
        })?;

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

fn hangup_media_impl(handle: *mut PluginSession) -> Result<(), Error> {
    janus_info!("[CONFERENCE] Hanging up WebRTC media on {:p}.", handle);

    let message_handler = MESSAGE_HANDLER
        .get()
        .ok_or_else(|| err_msg("Message handler is not initialized"))?;

    let mut switchboard = message_handler
        .switchboard
        .write()
        .map_err(|_| err_msg("Failed to acquire switchboard write lock"))?;

    let session = unsafe { Session::from_ptr(handle) }?;
    session.set_last_rtp_packet_timestamp(None)?;
    switchboard.disconnect(&session)
}

extern "C" fn hangup_media(handle: *mut PluginSession) {
    report_error(hangup_media_impl(handle));
}

fn incoming_rtp_impl(
    handle: *mut PluginSession,
    video: c_int,
    buf: *mut c_char,
    len: c_int,
) -> Result<(), Error> {
    let sess = unsafe { Session::from_ptr(handle)? };
    sess.set_last_rtp_packet_timestamp(Some(SystemTime::now()))?;

    let switchboard = MESSAGE_HANDLER
        .get()
        .ok_or_else(|| err_msg("Message handler not initialized"))
        .and_then(|message_handler| {
            message_handler
                .switchboard
                .read()
                .map_err(|_| err_msg("Failed to acquire message handler read lock"))
        })?;

    let buf_slice = unsafe { std::slice::from_raw_parts_mut(buf, len as usize) };

    for subscriber in switchboard.subscribers_to(&sess) {
        janus_callbacks::relay_rtp(subscriber, video, buf_slice);
    }

    if let Some(recorder) = switchboard.recorder_for(&sess) {
        let is_video = match video {
            0 => false,
            _ => true,
        };

        let buf = unsafe { std::slice::from_raw_parts(buf as *const u8, len as usize) };

        recorder.record_packet(buf, is_video)?;
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

    let switchboard = MESSAGE_HANDLER
        .get()
        .ok_or_else(|| err_msg("Message handler not initialized"))
        .and_then(|message_handler| {
            message_handler
                .switchboard
                .read()
                .map_err(|_| err_msg("Failed to acquire message handler read lock"))
        })?;

    let packet = unsafe { slice::from_raw_parts_mut(buf, len as usize) };

    match video {
        1 if janus::rtcp::has_pli(packet) => {
            send_pli(switchboard.publisher_to(&sess));
        }
        1 if janus::rtcp::has_fir(packet) => {
            send_fir(switchboard.publisher_to(&sess));
        }
        _ => {
            for subscriber in switchboard.subscribers_to(&sess) {
                janus_callbacks::relay_rtcp(subscriber, video, packet);
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

extern "C" fn incoming_data(
    _handle: *mut PluginSession,
    _label: *mut c_char,
    _buf: *mut c_char,
    _len: c_int,
) {
    // Dropping incoming data.
}

extern "C" fn slow_link(_handle: *mut PluginSession, _uplink: c_int, _video: c_int) {
    janus_info!("[CONFERENCE] slow link callback")
}

const PLUGIN: Plugin = build_plugin!(
    LibraryMetadata {
        api_version: 13,
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
    handle_admin_message,
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
