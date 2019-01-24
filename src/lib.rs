#[macro_use]
extern crate janus_plugin as janus;

extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate http;
extern crate toml;

#[macro_use]
extern crate lazy_static;
extern crate atom;
extern crate multimap;

extern crate glib;
extern crate gstreamer;
extern crate gstreamer_app;
extern crate gstreamer_base;

extern crate rusoto_core;
extern crate rusoto_s3;
extern crate s4;

#[macro_use]
extern crate failure;

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::slice;
use std::sync::{mpsc, Arc, RwLock};
use std::thread;

use atom::AtomSetOnce;
use failure::Error;
use janus::{
    sdp, JanssonValue, LibraryMetadata, Plugin, PluginCallbacks, PluginResult, PluginSession,
    RawJanssonValue, RawPluginResult,
};

mod bidirectional_multimap;
mod config;
mod janus_callbacks;
mod messages;
mod recorder;
mod session;
mod switchboard;
#[macro_use]
mod utils;
mod uploader;

use config::Config;
use messages::{APIError, ErrorStatus, JsepKind, Response, StreamOperation, StreamResponse};
use recorder::{AudioCodec, Recorder, VideoCodec};
use session::{Session, SessionState};
use switchboard::Switchboard;
use uploader::Uploader;

#[derive(Debug)]
struct Message {
    session: Arc<Session>,
    transaction: *mut c_char,
    message: Option<JanssonValue>,
    jsep: Option<JanssonValue>,
}

unsafe impl Send for Message {}

const CONFIG_FILE_NAME: &str = "janus.plugin.conference.toml";
// TODO: merge this with codecs for recorder
const AUDIO_CODEC: sdp::AudioCodec = sdp::AudioCodec::Opus;
const VIDEO_CODEC: sdp::VideoCodec = sdp::VideoCodec::H264;

#[derive(Debug)]
struct State {
    pub message_channel: AtomSetOnce<Box<mpsc::SyncSender<Message>>>,
    pub switchboard: RwLock<Switchboard>,
    pub config: AtomSetOnce<Box<Config>>,
    pub uploader: AtomSetOnce<Box<Uploader>>,
}

lazy_static! {
    static ref STATE: State = State {
        message_channel: AtomSetOnce::empty(),
        switchboard: RwLock::new(Switchboard::new()),
        config: AtomSetOnce::empty(),
        uploader: AtomSetOnce::empty(),
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

fn init_config(config_path: *const c_char) -> Result<config::Config, Error> {
    let config_path = unsafe { CStr::from_ptr(config_path) };
    let config_path = config_path.to_str()?;
    let config_path = Path::new(config_path);
    let mut config_path = config_path.to_path_buf();
    config_path.push(CONFIG_FILE_NAME);
    janus_info!(
        "[CONFERENCE] Reading config located at {}",
        config_path.to_string_lossy()
    );

    Ok(config::Config::from_path(&config_path)?)
}

extern "C" fn init(callbacks: *mut PluginCallbacks, config_path: *const c_char) -> c_int {
    match init_config(config_path) {
        Ok(config) => {
            STATE.config.set_if_none(Box::new(config));
        }
        Err(err) => {
            janus_fatal!("[CONFERENCE] Failed to read config: {}", err);
            return -1;
        }
    }

    let config = STATE.config.get().expect("Empty config?!");
    match Uploader::new(config.uploading.clone()) {
        Ok(uploader) => {
            // let res = uploader.upload_file(
            //     Path::new("/build/test_bd"),
            //     "origin.webinar.beta.foxford.ru",
            //     "test-test",
            // );

            // janus_info!("{:?}", res);

            STATE.uploader.set_if_none(Box::new(uploader));
        }
        Err(err) => {
            janus_fatal!("[CONFERENCE] Failed to init uploader: {:?}", err);
            return -1;
        }
    }

    janus_callbacks::init(callbacks);

    let (messages_tx, messages_rx) = mpsc::sync_channel(10);

    STATE.message_channel.set_if_none(Box::new(messages_tx));

    thread::spawn(move || {
        janus_info!("[CONFERENCE] Message processing thread is alive.");
        for msg in messages_rx.iter() {
            let push_result = match handle_message_async(&msg) {
                Ok((response, jsep)) => {
                    janus_callbacks::push_event(msg.session.handle, msg.transaction, response, jsep)
                        .map_err(Error::from)
                }
                Err(err) => {
                    janus_err!("Error processing message: {}", err);

                    let response = Response::new(None, Some(err));

                    serde_json::to_value(response)
                        .map_err(Error::from)
                        .and_then(|response| {
                            utils::serde_to_jansson(&response).and_then(|response| {
                                janus_callbacks::push_event(
                                    msg.session.handle,
                                    msg.transaction,
                                    Some(response),
                                    None,
                                )
                                .map_err(Error::from)
                            })
                        })
                }
            };

            if let Err(err) = push_result {
                janus_err!("Error pushing event: {}", err);
            }
        }
    });

    let res = gstreamer::init();
    if let Err(err) = res {
        janus_fatal!("[CONFERENCE] Failed to init GStreamer: {}", err);
        return -1;
    }

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
                    if let Some(recorder) = switchboard.recorder_for_mut(&sess) {
                        report_error(recorder.finish_record());
                    }
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

    let buf = unsafe { std::slice::from_raw_parts(buf as *const u8, len as usize) };
    if let Some(recorder) = switchboard.recorder_for(&sess) {
        let is_video = match video {
            0 => false,
            _ => true,
        };

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
    match received.message {
        Some(ref message) => {
            let operation: StreamOperation = utils::jansson_to_serde(&message)
                .map_err(|err| APIError::new(ErrorStatus::BAD_REQUEST, err, None))?;

            let mut switchboard = STATE.switchboard.write().map_err(|err| {
                let err = format_err!("{}", err);
                APIError::new(ErrorStatus::INTERNAL_SERVER_ERROR, err, Some(&operation))
            })?;

            let maybe_jsep = parse_jsep(&received.jsep)
                .map_err(|err| APIError::new(ErrorStatus::BAD_REQUEST, err, Some(&operation)));

            let jsep = match operation {
                StreamOperation::Create { .. } | StreamOperation::Read { .. } => {
                    let offer = maybe_jsep?;
                    let answer = offer.negotatiate(VIDEO_CODEC, AUDIO_CODEC);

                    received.session.set_offer(offer).map_err(|err| {
                        APIError::new(ErrorStatus::INTERNAL_SERVER_ERROR, err, Some(&operation))
                    })?;

                    let jsep = serde_json::to_value(answer).map_err(|err| {
                        APIError::new(
                            ErrorStatus::INTERNAL_SERVER_ERROR,
                            Error::from(err),
                            Some(&operation),
                        )
                    })?;
                    let jsep = utils::serde_to_jansson(&jsep).map_err(|err| {
                        APIError::new(ErrorStatus::INTERNAL_SERVER_ERROR, err, Some(&operation))
                    })?;

                    Some(jsep)
                }
                StreamOperation::Upload { .. } => None,
            };

            let response = match &operation {
                StreamOperation::Create { ref id } => {
                    switchboard.create_stream(id.to_owned(), received.session.clone());

                    let config = STATE.config.get().expect("Empty config?!");
                    let recorder = Recorder::new(
                        &config.recording.root_save_directory,
                        &id,
                        VideoCodec::H264,
                        AudioCodec::OPUS,
                    );
                    switchboard.attach_recorder(received.session.clone(), recorder);

                    StreamResponse::Create {}
                }
                StreamOperation::Read { ref id } => {
                    switchboard
                        .join_stream(&id, received.session.clone())
                        .map_err(|err| {
                            APIError::new(ErrorStatus::NOT_FOUND, err, Some(&operation))
                        })?;

                    StreamResponse::Read {}
                }
                StreamOperation::Upload { id, bucket, object } => {
                    if let Some(publisher) = switchboard.publisher_by_stream(&id) {
                        if let Some(recorder) = switchboard.recorder_for(publisher) {
                            let path = recorder.get_full_record_path();
                            STATE
                                .uploader
                                .get()
                                .expect("Empty uploader?!")
                                .upload_file(&path, &bucket, &object)
                                .map_err(|err| {
                                    APIError::new(
                                        ErrorStatus::INTERNAL_SERVER_ERROR,
                                        err,
                                        Some(&operation),
                                    )
                                })?;

                            StreamResponse::Upload {}
                        } else {
                            return Err(APIError::new(
                                ErrorStatus::NOT_FOUND,
                                format_err!("There's no recording for Stream with Id = {}", id),
                                Some(&operation),
                            ));
                        }
                    } else {
                        return Err(APIError::new(
                            ErrorStatus::NOT_FOUND,
                            format_err!("Stream with Id = {} is not found", id),
                            Some(&operation),
                        ));
                    }
                }
            };

            let response = Response::new(Some(response), None);

            let response = serde_json::to_value(response).map_err(|err| {
                APIError::new(
                    ErrorStatus::INTERNAL_SERVER_ERROR,
                    Error::from(err),
                    Some(&operation),
                )
            })?;
            let response = utils::serde_to_jansson(&response).map_err(|err| {
                APIError::new(ErrorStatus::INTERNAL_SERVER_ERROR, err, Some(&operation))
            })?;

            Ok((Some(response), jsep))
        }
        None => Ok((None, None)),
    }
}

fn parse_jsep(jsep: &Option<JanssonValue>) -> Result<JsepKind, Error> {
    match jsep {
        Some(jsep) => {
            let jsep_json: JsepKind = utils::jansson_to_serde(jsep)?;
            Ok(jsep_json)
        }
        None => Err(failure::err_msg("JSEP is empty")),
    }
}
