#![feature(c_variadic)]

#[macro_use]
extern crate janus_plugin as janus;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate lazy_static;

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::slice;

use anyhow::{bail, format_err, Context, Result};
use janus::{
    session::SessionWrapper, JanssonDecodingFlags, JanssonValue, LibraryMetadata, Plugin,
    PluginCallbacks, PluginDataPacket, PluginResult, PluginRtcpPacket, PluginRtpPacket,
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
mod switchboard;
#[macro_use]
mod utils;
#[cfg(test)]
mod test_stubs;
mod uploader;

use app::App;
use conf::Config;
use switchboard::SessionId;

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

fn init_config(config_path: *const c_char) -> Result<Config> {
    let config_path = unsafe { CStr::from_ptr(config_path) };
    let config_path = config_path.to_str()?;
    let config_path = Path::new(config_path);

    Ok(Config::from_path(config_path)?)
}

extern "C" fn create_session(handle: *mut PluginSession, error: *mut c_int) {
    if let Err(err) = create_session_impl(handle) {
        janus_err!("[CONFERENCE] {}", err);
        unsafe { *error = -1 };
    }
}

fn create_session_impl(handle: *mut PluginSession) -> Result<()> {
    app!()?.switchboard.with_write_lock(|mut switchboard| {
        let session_id = SessionId::new();
        janus_verb!("[CONFERENCE] Initializing session {}", session_id);

        // WARNING: If this variable gets droppped the memory will be freed by C.
        //          Any future calls to `SessionWrapper::from_ptr` will return an invalid result
        //          which will cause segfault on drop.
        //          To prevent this we have to store this variable as is and make sure it won't
        //          be dropped until there're no callbacks are possible to call for this handle.
        let session = unsafe { SessionWrapper::associate(handle, session_id) }
            .context("Session associate error")?;

        switchboard.connect(session)?;
        Ok(())
    })
}

extern "C" fn query_session(_handle: *mut PluginSession) -> *mut RawJanssonValue {
    janus_verb!("[CONFERENCE] Querying session");
    std::ptr::null_mut()
}

extern "C" fn handle_message(
    handle: *mut PluginSession,
    transaction: *mut c_char,
    message: *mut RawJanssonValue,
    jsep: *mut RawJanssonValue,
) -> *mut RawPluginResult {
    match handle_message_impl(handle, transaction, message, jsep) {
        Ok(()) => PluginResult::ok_wait(None).into_raw(),
        Err(err) => {
            janus_err!("[CONFERENCE] Message handling error: {}", err);
            PluginResult::error(c_str!("Failed to handle message")).into_raw()
        }
    }
}

fn handle_message_impl(
    handle: *mut PluginSession,
    transaction: *mut c_char,
    message: *mut RawJanssonValue,
    jsep: *mut RawJanssonValue,
) -> Result<()> {
    let session_id = session_id(handle)?;
    janus_verb!("[CONFERENCE] Handling message on {}.", session_id);

    let transaction = match unsafe { CString::from_raw(transaction) }.to_str() {
        Ok(transaction) => String::from(transaction),
        Err(err) => bail!("Failed to serialize transaction: {}", err),
    };

    if let Some(json) = unsafe { JanssonValue::from_raw(message) } {
        let jsep_offer = unsafe { JanssonValue::from_raw(jsep) };

        app!()?
            .message_handling_loop
            .schedule_request(session_id, &transaction, &json, jsep_offer)
            .context("Failed to schedule message handling")?;
    }

    Ok(())
}

extern "C" fn handle_admin_message(_message: *mut RawJanssonValue) -> *mut RawJanssonValue {
    JanssonValue::from_str("{}", JanssonDecodingFlags::empty())
        .expect("Failed to decode JSON")
        .into_raw()
}

extern "C" fn setup_media(handle: *mut PluginSession) {
    report_error(setup_media_impl(handle));
}

fn setup_media_impl(handle: *mut PluginSession) -> Result<()> {
    app!()?.switchboard.with_read_lock(|switchboard| {
        let session_id = session_id(handle)?;
        switchboard.publisher_to(session_id).map(send_fir);

        janus_info!(
            "[CONFERENCE] WebRTC media is now available for {}.",
            session_id
        );

        Ok(())
    })
}

extern "C" fn incoming_rtp(handle: *mut PluginSession, packet: *mut PluginRtpPacket) {
    report_error(incoming_rtp_impl(handle, packet));
}

fn incoming_rtp_impl(handle: *mut PluginSession, packet: *mut PluginRtpPacket) -> Result<()> {
    app!()?.switchboard.with_read_lock(|switchboard| {
        let session_id = session_id(handle)?;

        let state = switchboard.state(session_id)?;
        state.touch_last_rtp_packet_timestamp()?;

        let mut packet = unsafe { &mut *packet };
        let video = packet.video;

        for subscriber_id in switchboard.subscribers_to(session_id) {
            let subscriber_session =
                switchboard.session(*subscriber_id)?.lock().map_err(|err| {
                    format_err!(
                        "Failed to acquire subscriber session mutex id = {}: {}",
                        subscriber_id,
                        err
                    )
                })?;

            janus_callbacks::relay_rtp(&subscriber_session, &mut packet);
        }

        if let Some(recorder) = state.recorder() {
            let is_video = match video {
                0 => false,
                _ => true,
            };

            let buf = unsafe {
                std::slice::from_raw_parts(packet.buffer as *const u8, packet.length as usize)
            };

            recorder.record_packet(buf, is_video)?;
        }

        Ok(())
    })
}

extern "C" fn incoming_rtcp(handle: *mut PluginSession, packet: *mut PluginRtcpPacket) {
    report_error(incoming_rtcp_impl(handle, packet));
}

fn incoming_rtcp_impl(handle: *mut PluginSession, packet: *mut PluginRtcpPacket) -> Result<()> {
    app!()?.switchboard.with_read_lock(|switchboard| {
        let session_id = session_id(handle)?;
        let mut packet = unsafe { &mut *packet };
        let data = unsafe { slice::from_raw_parts_mut(packet.buffer, packet.length as usize) };

        match packet.video {
            1 if janus::rtcp::has_pli(data) => {
                switchboard.publisher_to(session_id).map(send_pli);
            }
            1 if janus::rtcp::has_fir(data) => {
                switchboard.publisher_to(session_id).map(send_fir);
            }
            _ => {
                for subscriber in switchboard.subscribers_to(session_id) {
                    let subscriber_session =
                        switchboard.session(*subscriber)?.lock().map_err(|err| {
                            format_err!(
                                "Failed to acquire subscriber session mutex for id = {}: {}",
                                subscriber,
                                err
                            )
                        })?;

                    janus_callbacks::relay_rtcp(&subscriber_session, &mut packet);
                }
            }
        }

        Ok(())
    })
}

extern "C" fn incoming_data(_handle: *mut PluginSession, _packet: *mut PluginDataPacket) {
    // Dropping incoming data.
}

extern "C" fn slow_link(_handle: *mut PluginSession, _uplink: c_int, _video: c_int) {
    janus_info!("[CONFERENCE] Slow link")
}

extern "C" fn hangup_media(_handle: *mut PluginSession) {
    janus_verb!("[CONFERENCE] Hang up")
}

extern "C" fn destroy_session(handle: *mut PluginSession, error: *mut c_int) {
    report_error(destroy_session_impl(handle, error));
}

fn destroy_session_impl(handle: *mut PluginSession, _error: *mut c_int) -> Result<()> {
    let session_id = session_id(handle)?;
    janus_verb!("[CONFERENCE] Destroying Conference session {}", session_id);

    app!()?
        .switchboard
        .with_write_lock(|mut switchboard| switchboard.handle_disconnect(session_id))
}

extern "C" fn destroy() {
    janus_info!("[CONFERENCE] Janus Conference plugin destroyed!");
}

///////////////////////////////////////////////////////////////////////////////

fn session_id(handle: *mut PluginSession) -> Result<SessionId> {
    match unsafe { SessionWrapper::from_ptr(handle) } {
        Ok(session) => Ok(**session),
        Err(err) => bail!("Failed to get session: {}", err),
    }
}

fn send_pli(publisher: SessionId) {
    report_error(send_pli_impl(publisher));
}

fn send_pli_impl(publisher: SessionId) -> Result<()> {
    app!()?.switchboard.with_read_lock(move |switchboard| {
        let session = switchboard.session(publisher)?.lock().map_err(|err| {
            format_err!("Failed to acquire mutex for session {}: {}", publisher, err)
        })?;

        let mut pli = janus::rtcp::gen_pli();

        let mut packet = PluginRtcpPacket {
            video: 1,
            buffer: pli.as_mut_ptr(),
            length: pli.len() as i16,
        };

        janus_callbacks::relay_rtcp(&session, &mut packet);
        Ok(())
    })
}

fn send_fir(publisher: SessionId) {
    report_error(send_fir_impl(publisher));
}

fn send_fir_impl(publisher: SessionId) -> Result<()> {
    app!()?.switchboard.with_read_lock(move |switchboard| {
        let session = switchboard.session(publisher)?.lock().map_err(|err| {
            format_err!("Failed to acquire mutex for session {}: {}", publisher, err)
        })?;

        let state = switchboard.state(publisher)?;
        let mut seq = state.increment_fir_seq();
        let mut fir = janus::rtcp::gen_fir(&mut seq);

        let mut packet = PluginRtcpPacket {
            video: 1,
            buffer: fir.as_mut_ptr(),
            length: fir.len() as i16,
        };

        janus_callbacks::relay_rtcp(&session, &mut packet);
        Ok(())
    })
}

fn report_error(res: Result<()>) {
    match res {
        Ok(_) => {}
        Err(err) => {
            janus_err!("[CONFERENCE] {}", err);
        }
    }
}

///////////////////////////////////////////////////////////////////////////////

const PLUGIN: Plugin = build_plugin!(
    LibraryMetadata {
        api_version: 15,
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
