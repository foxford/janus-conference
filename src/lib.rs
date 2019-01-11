#[macro_use]
extern crate janus_plugin as janus;

extern crate serde;
#[macro_use]
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate toml;

#[macro_use]
extern crate lazy_static;
extern crate atom;
extern crate multimap;

extern crate glib;
extern crate gstreamer;
extern crate gstreamer_app;
extern crate gstreamer_base;

extern crate futures;
extern crate futures_fs;
extern crate rusoto_core;
extern crate rusoto_s3;

#[macro_use]
extern crate failure;

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::slice;
use std::sync::{atomic::Ordering, mpsc, Arc, RwLock};
use std::thread;

use atom::AtomSetOnce;
use failure::Error;
use janus::{
    sdp, JanssonDecodingFlags, JanssonEncodingFlags, JanssonValue, LibraryMetadata, Plugin,
    PluginCallbacks, PluginResult, PluginSession, RawJanssonValue, RawPluginResult,
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
use messages::{JsepKind, StreamOperation};
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
            STATE.uploader.set_if_none(Box::new(uploader));
        }
        Err(err) => {
            janus_fatal!("[CONFERENCE] Failed to init uploader: {:?}", err);
            return -1;
        }
    }

    janus_callbacks::init(callbacks);

    let (messages_tx, messages_rx) = mpsc::sync_channel(0);

    STATE.message_channel.set_if_none(Box::new(messages_tx));

    thread::spawn(move || {
        janus_info!("[CONFERENCE] Message processing thread is alive.");
        for msg in messages_rx.iter() {
            if let Some(err) = handle_message_async(msg).err() {
                janus_err!("Error processing message: {}", err);
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
                    switchboard
                        .disconnect(&sess)
                        .map(|mut recorder| report_error(recorder.finish_record()));
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

    let buf = unsafe { std::slice::from_raw_parts(buf as *const u8, len as usize) };
    if let Some(recorder) = switchboard.recorder_for(sess) {
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

fn handle_message_async(received: Message) -> Result<(), Error> {
    match received.jsep {
        Some(jsep) => {
            handle_jsep(&received.session, received.transaction, &jsep)?;
        }
        None => {
            janus_info!("[CONFERENCE] JSEP is empty, skipping");
        }
    };

    if let Some(message) = received.message {
        let message = message.to_libcstring(JanssonEncodingFlags::empty());
        let message = message.to_string_lossy();
        let message: StreamOperation = serde_json::from_str(&message)?;

        let mut switchboard = STATE
            .switchboard
            .write()
            .map_err(|err| format_err!("{}", err))?;

        match message {
            StreamOperation::Create { id } => {
                {
                    let config = STATE.config.get().expect("Empty config?!");
                    let recorder = Recorder::new(
                        &config.recording.root_save_directory,
                        &id,
                        VideoCodec::H264,
                        AudioCodec::OPUS,
                    );

                    switchboard.attach_recorder(received.session.clone(), recorder);
                }

                switchboard.create_room(id, received.session.clone());
            }
            StreamOperation::Read { id } => switchboard.join_room(id, received.session.clone()),
        }
    }

    Ok(())
}

fn handle_jsep(
    session: &Session,
    transaction: *mut c_char,
    jsep: &JanssonValue,
) -> Result<(), Error> {
    let jsep = jsep.to_libcstring(JanssonEncodingFlags::empty());
    let jsep = jsep.to_string_lossy();
    let jsep_json: JsepKind = serde_json::from_str(&jsep)?;

    let answer: serde_json::Value = match jsep_json {
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

            serde_json::to_value(JsepKind::Answer { sdp: answer })?
        }
        JsepKind::Answer { .. } => unreachable!(),
    };

    let event_json = json!({ "result": "ok" });
    let mut event_serde: JanssonValue =
        JanssonValue::from_str(&event_json.to_string(), JanssonDecodingFlags::empty())
            .map_err(|err| format_err!("{}", err))?;
    let event = event_serde.as_mut_ref();

    let mut jsep_serde: JanssonValue =
        JanssonValue::from_str(&answer.to_string(), JanssonDecodingFlags::empty())
            .map_err(|err| format_err!("{}", err))?;
    let jsep = jsep_serde.as_mut_ref();

    janus_callbacks::push_event(session.handle, transaction, event, jsep)?;

    Ok(())
}
