#![feature(c_variadic)]

#[macro_use]
extern crate janus_plugin as janus;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate failure;

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::slice;
use std::time::SystemTime;

use failure::Error;
use janus::{
    JanssonDecodingFlags, JanssonValue, LibraryMetadata, Plugin, PluginCallbacks, PluginResult,
    PluginSession, RawJanssonValue, RawPluginResult,
};

#[macro_use]
mod app;
mod bidirectional_multimap;
mod conf;
mod janus_callbacks;
mod jsep;
mod message_handler;
mod recorder;
mod serde;
mod session;
mod switchboard;
#[macro_use]
mod utils;
#[cfg(test)]
mod test_stubs;
mod uploader;

use app::App;
use conf::Config;
use session::{Session, SessionState};

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
        Ok(config) => config,
        Err(err) => {
            janus_fatal!("[CONFERENCE] Failed to read config: {}", err);
            return -1;
        }
    };

    janus_info!("{:?}", config);

    if let Err(err) = App::init(config) {
        janus_fatal!("[CONFERENCE] Janus Conference plugin init failed: {}", err);
        return -1;
    };

    if let Err(err) = gstreamer::init() {
        janus_fatal!("[CONFERENCE] Failed to init GStreamer: {}", err);
        return -1;
    }

    janus_callbacks::init(callbacks);

    janus_info!("[CONFERENCE] Janus Conference plugin initialized!");
    0
}

extern "C" fn destroy() {
    janus_info!("[CONFERENCE] Janus Conference plugin destroyed!");
}

extern "C" fn create_session(handle: *mut PluginSession, error: *mut c_int) {
    if let Err(err) = create_session_impl(handle) {
        janus_err!("[CONFERENCE] {}", err);
        unsafe { *error = -1 };
    }
}

fn create_session_impl(handle: *mut PluginSession) -> Result<(), Error> {
    app!()?.switchboard.with_write_lock(|mut switchboard| {
        let initial_state = SessionState::new();

        let session = unsafe { Session::associate(handle, initial_state) }
            .map_err(|err| format_err!("Session associate error: {}", err))?;

        janus_info!(
            "[CONFERENCE] Initializing session for handle {:p}",
            session.handle
        );

        switchboard.connect(session);
        Ok(())
    })
}

extern "C" fn destroy_session(handle: *mut PluginSession, _error: *mut c_int) {
    janus_info!("[CONFERENCE] Destroying session for handle {:p}", handle);
}

extern "C" fn query_session(handle: *mut PluginSession) -> *mut RawJanssonValue {
    janus_info!("[CONFERENCE] Querying session for handle {:p}", handle);
    std::ptr::null_mut()
}

extern "C" fn handle_message(
    handle: *mut PluginSession,
    transaction: *mut c_char,
    message: *mut RawJanssonValue,
    jsep: *mut RawJanssonValue,
) -> *mut RawPluginResult {
    janus_info!("[CONFERENCE] Handling message on {:p}", handle);

    let session = match unsafe { Session::from_ptr(handle) } {
        Ok(session) => session,
        Err(err) => {
            janus_err!("[CONFERENCE] Failed to restore session state: {}", err);
            return PluginResult::error(c_str!("Failed to restore session state")).into_raw();
        }
    };

    match session.closing().read() {
        Ok(value) => {
            if *value {
                janus_warn!("[CONFERENCE] Skipping handling a message sent to a closing session");
                let err = c_str!("Skipping handling a message sent to a closing session");
                return PluginResult::error(err).into_raw();
            }
        }
        Err(err) => {
            janus_err!(
                "[CONFERENCE] Failed to acquire closing flag mutex (handle_message): {}",
                err
            );
            let err = c_str!("Failed acquire closing flag mutex");
            return PluginResult::error(err).into_raw();
        }
    }

    let transaction = match unsafe { CString::from_raw(transaction) }.to_str() {
        Ok(transaction) => String::from(transaction),
        Err(err) => {
            janus_err!("[CONFERENCE] Failed to serialize transaction: {}", err);
            return PluginResult::error(c_str!("Failed serialize transaction")).into_raw();
        }
    };

    if let Some(json) = unsafe { JanssonValue::from_raw(message) } {
        let jsep_offer = unsafe { JanssonValue::from_raw(jsep) };

        let result = app!().map(|app| {
            app.message_handling_loop
                .schedule_request(session, &transaction, &json, jsep_offer)
        });

        if let Err(err) = result {
            janus_err!("[CONFERENCE] Failed to schedule message handling: {}", err);
            return PluginResult::error(c_str!("Failed to schedule message handling")).into_raw();
        }
    }

    PluginResult::ok_wait(None).into_raw()
}

extern "C" fn handle_admin_message(_message: *mut RawJanssonValue) -> *mut RawJanssonValue {
    JanssonValue::from_str("{}", JanssonDecodingFlags::empty())
        .expect("Failed to decode JSON")
        .into_raw()
}

fn setup_media_impl(handle: *mut PluginSession) -> Result<(), Error> {
    let app = app!()?;
    let session = unsafe { Session::from_ptr(handle)? };

    app.switchboard.with_read_lock(|switchboard| {
        send_fir(switchboard.publisher_to(&session));
        Ok(())
    })?;

    janus_info!("[CONFERENCE] WebRTC media is now available on {:p}", handle);
    Ok(())
}

extern "C" fn setup_media(handle: *mut PluginSession) {
    report_error(setup_media_impl(handle));
}

fn hangup_media_impl(handle: *mut PluginSession) -> Result<(), Error> {
    janus_info!("[CONFERENCE] Hanging up WebRTC media on {:p}", handle);

    let app = app!()?;
    let session = unsafe { Session::from_ptr(handle) }?;
    session.set_last_rtp_packet_timestamp(None)?;

    app.switchboard
        .with_write_lock(|mut switchboard| switchboard.disconnect(&session))
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
    let app = app!()?;
    let session = unsafe { Session::from_ptr(handle)? };
    session.set_last_rtp_packet_timestamp(Some(SystemTime::now()))?;

    app.switchboard.with_read_lock(|switchboard| {
        let subscribers = switchboard.subscribers_to(&session);
        let buf_slice = unsafe { std::slice::from_raw_parts_mut(buf, len as usize) };

        for subscriber in subscribers {
            janus_callbacks::relay_rtp(subscriber, video, buf_slice);
        }

        let recorder = switchboard.recorder_for(&session);

        if let Some(recorder) = recorder {
            let is_video = match video {
                0 => false,
                _ => true,
            };

            let buf = unsafe { std::slice::from_raw_parts(buf as *const u8, len as usize) };
            recorder.record_packet(buf, is_video)?;
        }

        Ok(())
    })
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
    app!()?.switchboard.with_read_lock(|switchboard| {
        let session = unsafe { Session::from_ptr(handle)? };
        let packet = unsafe { slice::from_raw_parts_mut(buf, len as usize) };

        match video {
            1 if janus::rtcp::has_pli(packet) => {
                send_pli(switchboard.publisher_to(&session));
            }
            1 if janus::rtcp::has_fir(packet) => {
                send_fir(switchboard.publisher_to(&session));
            }
            _ => {
                for subscriber in switchboard.subscribers_to(&session) {
                    janus_callbacks::relay_rtcp(subscriber, video, packet);
                }
            }
        }

        Ok(())
    })
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
