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

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::sync::{mpsc, RwLock};
use std::thread;

use atom::AtomSetOnce;
use janus::{
    sdp, JanssonDecodingFlags, JanssonEncodingFlags, JanssonValue, JanusError, JanusResult,
    LibraryMetadata, Plugin, PluginCallbacks, PluginResult, PluginSession, RawJanssonValue,
    RawPluginResult,
};

mod messages;
mod session;
mod switchboard;

use messages::JsepKind;
use session::{Session, SessionState};
use switchboard::Switchboard;

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

fn push_event(
    handle: *mut PluginSession,
    transaction: *mut c_char,
    body: *mut RawJanssonValue,
    jsep: *mut RawJanssonValue,
) -> JanusResult {
    let push_event_fn = acquire_callbacks().push_event;

    let res = push_event_fn(handle, &mut PLUGIN, transaction, body, jsep);

    JanusError::from(res)
}

#[derive(Debug)]
struct Message {
    handle: *mut PluginSession,
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

extern "C" fn init(callbacks: *mut PluginCallbacks, _config_path: *const c_char) -> c_int {
    unsafe {
        let callbacks = callbacks
            .as_ref()
            .expect("Invalid callbacks ptr from Janus Core");
        CALLBACKS = Some(callbacks);
    }

    let (messages_tx, messages_rx) = mpsc::sync_channel(0);

    STATE.message_channel.set_if_none(Box::new(messages_tx));

    thread::spawn(move || {
        janus_info!("[CONFERENCE] Message processing thread is alive.");
        for msg in messages_rx.iter() {
            handle_message_async(msg).err().map(|e| {
                janus_err!("Error processing message: {}", e);
            });
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
            STATE
                .switchboard
                .write()
                .expect("Switchboard is poisoned")
                .connect(sess)
        }
        Err(e) => {
            janus_err!("{}", e);
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

            let mut destroyed = sess
                .destroyed
                .lock()
                .expect("Session destruction mutex is poisoned");
            let mut switchboard = STATE.switchboard.write().expect("Switchboard is poisoned");
            switchboard.disconnect(&sess);

            *destroyed = true;
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
    janus_info!("[CONFERENCE] Queueing signalling message on {:p}.", handle);

    let msg = Message {
        handle,
        transaction,
        message: unsafe { JanssonValue::from_raw(message) },
        jsep: unsafe { JanssonValue::from_raw(jsep) },
    };

    STATE.message_channel.get().and_then(|ch| ch.send(msg).ok());

    PluginResult::ok_wait(Some(c_str!("Processing..."))).into_raw()
}

extern "C" fn setup_media(handle: *mut PluginSession) {
    janus_info!(
        "[CONFERENCE] WebRTC media is now available on {:p}.",
        handle
    );
}

extern "C" fn hangup_media(handle: *mut PluginSession) {
    janus_info!("[CONFERENCE] Hanging up WebRTC media on {:p}.", handle);
}

extern "C" fn incoming_rtp(handle: *mut PluginSession, video: c_int, buf: *mut c_char, len: c_int) {
    let sess = unsafe { Session::from_ptr(handle).expect("Session can't be null") };
    let switchboard = STATE
        .switchboard
        .read()
        .expect("Switchboard lock poisoned; can't continue");
    for other in switchboard.subscribers_for(&sess) {
        relay_rtp(other.as_ptr(), video, buf, len);
    }
}

extern "C" fn incoming_rtcp(
    handle: *mut PluginSession,
    video: c_int,
    buf: *mut c_char,
    len: c_int,
) {
    // Dropping incoming rtcp.
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

fn handle_message_async(received: Message) -> JanusResult {
    if received.jsep.is_none() {
        janus_info!("[CONFERENCE] JSEP is empty, skipping");
        return Ok(());
    }

    let jsep = received
        .jsep
        .expect("JSEP is None")
        .to_libcstring(JanssonEncodingFlags::empty());
    let jsep_string = jsep.to_string_lossy();
    let jsep_json: JsepKind =
        serde_json::from_str(&jsep_string).expect("Failed to parse JSEP kind");
    janus_verb!("[CONFERENCE] jsep: {:?}", jsep_json);

    let answer: serde_json::Value = match jsep_json {
        JsepKind::Offer { sdp } => {
            let offer = sdp::Sdp::parse(
                &CString::new(sdp).expect("Failed to create string from SDP offer"),
            ).expect("Failed to parse SDP offer");
            janus_verb!("[CONFERENCE] offer: {:?}", offer);

            let answer = answer_sdp!(offer);
            janus_verb!("[CONFERENCE] answer: {:?}", answer);

            let answer = answer.to_glibstring().to_string_lossy().to_string();

            serde_json::to_value(JsepKind::Answer { sdp: answer })
                .expect("Failed to create JSON string from SDP answer")
        }
        JsepKind::Answer { .. } => unreachable!(),
    };

    let event_json = json!({ "result": "ok" });
    let mut event_serde: JanssonValue =
        JanssonValue::from_str(&event_json.to_string(), JanssonDecodingFlags::empty())
            .expect("Failed to create Jansson value with event");
    let event = event_serde.as_mut_ref();

    let mut jsep_serde: JanssonValue =
        JanssonValue::from_str(&answer.to_string(), JanssonDecodingFlags::empty())
            .expect("Failed to create Jansson value with JSEP");
    let jsep = jsep_serde.as_mut_ref();

    push_event(received.handle, received.transaction, event, jsep)
        .expect("Pushing event has failed");

    Ok(())
}
