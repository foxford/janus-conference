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

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::slice;
use std::sync::{atomic::Ordering, mpsc, Arc, RwLock};
use std::thread;

use atom::AtomSetOnce;
use janus::{
    sdp, JanssonDecodingFlags, JanssonEncodingFlags, JanssonValue, JanusResult, LibraryMetadata,
    Plugin, PluginCallbacks, PluginResult, PluginSession, RawJanssonValue, RawPluginResult,
};

mod bidirectional_multimap;
mod janus_callbacks;
mod messages;
mod session;
mod switchboard;
#[macro_use]
mod utils;

use messages::{JsepKind, RTCOperation};
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

extern "C" fn init(callbacks: *mut PluginCallbacks, _config_path: *const c_char) -> c_int {
    janus_callbacks::init(callbacks);

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

extern "C" fn setup_media(handle: *mut PluginSession) {
    let sess = unsafe { Session::from_ptr(handle).expect("Session can't be null!") };
    let switchboard = STATE.switchboard.read().expect("Switchboard is poisoned");
    send_fir(switchboard.publisher_to(&sess));

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
    for subscriber in switchboard.subscribers_to(&sess) {
        janus_callbacks::relay_rtp(subscriber.as_ptr(), video, buf, len);
    }
}

extern "C" fn incoming_rtcp(
    handle: *mut PluginSession,
    video: c_int,
    buf: *mut c_char,
    len: c_int,
) {
    let sess = unsafe { Session::from_ptr(handle).expect("Session can't be null") };

    let switchboard = STATE
        .switchboard
        .read()
        .expect("Switchboard lock poisoned; can't continue");

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
    match received.jsep {
        Some(jsep) => {
            handle_jsep(received.session.clone(), received.transaction, jsep)?;
        }
        None => {
            janus_info!("[CONFERENCE] JSEP is empty, skipping");
        }
    };

    if let Some(message) = received.message {
        let message = message.to_libcstring(JanssonEncodingFlags::empty());
        let message = message.to_string_lossy();
        let message: RTCOperation =
            serde_json::from_str(&message).expect("Failed to parse message");

        let mut switchboard = STATE
            .switchboard
            .write()
            .expect("Switchboard lock poisoned; can't continue");

        match message {
            RTCOperation::Create { room_id } => {
                switchboard.create_room(room_id, received.session.clone())
            }
            RTCOperation::Read { room_id } => {
                switchboard.join_room(room_id, received.session.clone())
            }
        }
    }

    Ok(())
}

fn handle_jsep(session: Arc<Session>, transaction: *mut c_char, jsep: JanssonValue) -> JanusResult {
    let jsep = jsep.to_libcstring(JanssonEncodingFlags::empty());
    let jsep = jsep.to_string_lossy();
    let jsep_json: JsepKind = serde_json::from_str(&jsep).expect("Failed to parse JSEP kind");

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

    janus_callbacks::push_event(session.handle, transaction, event, jsep)
        .expect("Pushing event has failed");

    Ok(())
}
