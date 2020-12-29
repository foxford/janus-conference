#![feature(c_variadic)]

#[macro_use]
extern crate anyhow;
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
use chrono::{Duration, Utc};
use janus::{
    session::SessionWrapper, JanssonDecodingFlags, JanssonValue, LibraryMetadata, Plugin,
    PluginCallbacks, PluginDataPacket, PluginResult, PluginRtcpPacket, PluginRtpPacket,
    PluginSession, RawJanssonValue, RawPluginResult,
};

#[macro_use]
mod utils;
#[macro_use]
mod app;
mod bidirectional_multimap;
mod conf;
mod janus_callbacks;
mod janus_recorder;
mod janus_rtp;
mod jsep;
mod message_handler;
mod recorder;
mod serde;
mod switchboard;
#[cfg(test)]
mod test_stubs;

use app::App;
use conf::Config;
use switchboard::SessionId;

const INITIAL_REMBS: u64 = 4;

lazy_static! {
    static ref REMB_INTERVAL: Duration = Duration::seconds(5);
}

extern "C" fn init(callbacks: *mut PluginCallbacks, config_path: *const c_char) -> c_int {
    let config = match init_config(config_path) {
        Ok(config) => config,
        Err(err) => {
            fatal!("Failed to read config: {}", err);
            return -1;
        }
    };

    info!("Config: {:#?}", config);

    if let Err(err) = App::init(config) {
        fatal!("Init failed: {}", err);
        return -1;
    };

    janus_callbacks::init(callbacks);
    info!("Initialized");
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
        err!("Failed to create session: {}", err);
        unsafe { *error = -1 };
    }
}

fn create_session_impl(handle: *mut PluginSession) -> Result<()> {
    app!()?.switchboard.with_write_lock(|mut switchboard| {
        let ice_handle = unsafe { &*((*handle).gateway_handle as *mut utils::janus_ice_handle) };
        let session_id = SessionId::new(ice_handle.handle_id);
        verb!("Initializing session"; {"handle_id": session_id});

        // WARNING: If this variable gets dropped the memory would be freed by C.
        //          Any future calls to `SessionWrapper::from_ptr` would return an invalid result
        //          which would cause segfault on drop.
        //          To prevent this we have to store this variable as is and make sure it won't
        //          be dropped until there're no callbacks possible to call for this handle.
        let session = unsafe { SessionWrapper::associate(handle, session_id) }
            .context("Session associate error")?;

        switchboard.connect(session)?;
        Ok(())
    })
}

extern "C" fn query_session(_handle: *mut PluginSession) -> *mut RawJanssonValue {
    verb!("Querying session");
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
            err!("Message handling error: {}", err);
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
    verb!("Incoming message"; {"handle_id": session_id});

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

        if let Some(writer) = switchboard.writer_to(session_id) {
            send_fir(writer);
        }

        let rtc_id = switchboard.stream_id_to(session_id);
        info!("WebRTC media is now available"; {"handle_id": session_id, "rtc_id": rtc_id});
        Ok(())
    })
}

extern "C" fn incoming_rtp(handle: *mut PluginSession, packet: *mut PluginRtpPacket) {
    report_error(incoming_rtp_impl(handle, packet));
}

fn incoming_rtp_impl(handle: *mut PluginSession, packet: *mut PluginRtpPacket) -> Result<()> {
    let app = app!()?;

    app.switchboard.with_read_lock(|switchboard| {
        let session_id = session_id(handle)?;
        let state = switchboard.state(session_id)?;

        // Touch last packet timestamp to drop timeout.
        state.touch_last_rtp_packet_timestamp();

        // Send incremental initial or regular REMB to the writer if needed to control bitrate.
        if let Some(target_bitrate) = app.config.constraint.writer.bitrate {
            let initial_rembs_left = INITIAL_REMBS - state.initial_rembs_counter();

            if initial_rembs_left > 0 {
                let bitrate = target_bitrate / initial_rembs_left as u32;
                send_remb(session_id, bitrate);
                state.touch_last_remb_timestamp();
                state.increment_initial_rembs_counter();
            } else if let Some(last_remb_timestamp) = state.last_remb_timestamp() {
                if Utc::now() - last_remb_timestamp >= *REMB_INTERVAL {
                    send_remb(session_id, target_bitrate);
                    state.touch_last_remb_timestamp();
                }
            }
        }

        let mut packet = unsafe { &mut *packet };
        let video = packet.video;

        // Find stream id for the writer.
        let stream_id = match switchboard.written_by(session_id) {
            Some(stream_id) => stream_id,
            None => {
                verb!(
                    "Incoming RTP packet from non-writer. Dropping.";
                    {"handle_id": session_id}
                );

                return Ok(());
            }
        };

        // Update packet header for proper timestamp and seq number.
        match switchboard.switching_context(stream_id) {
            Some(switching_context) => {
                switching_context.update_rtp_packet_header(&mut packet)?;
            }
            None => {
                warn!(
                    "Failed to get switching context. Skipping RTP packet header update";
                    {"rtc_id": stream_id, "handle_id": session_id}
                );
            }
        }

        // Retransmit packet to writers.
        for reader in switchboard.readers_of(stream_id) {
            let reader_session = switchboard.session(*reader)?.lock().map_err(|err| {
                format_err!(
                    "Failed to acquire reader session mutex id = {}: {}",
                    reader,
                    err
                )
            })?;

            janus_callbacks::relay_rtp(&reader_session, &mut packet);
        }

        // Push packet to the recorder.
        if let Some(recorder) = state.recorder() {
            let is_video = matches!(video, 1);

            let buf = unsafe {
                std::slice::from_raw_parts(packet.buffer as *const i8, packet.length as usize)
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
                if let Some(writer) = switchboard.writer_to(session_id) {
                    send_pli(writer);
                }
            }
            1 if janus::rtcp::has_fir(data) => {
                if let Some(writer) = switchboard.writer_to(session_id) {
                    send_fir(writer);
                }
            }
            _ => {
                let stream_id = match switchboard.written_by(session_id) {
                    Some(stream_id) => stream_id,
                    None => {
                        verb!(
                            "Incoming RTCP packet from non-writer. Dropping.";
                            {"handle_id": session_id}
                        );

                        return Ok(());
                    }
                };

                for reader in switchboard.readers_of(stream_id) {
                    let reader_session = switchboard.session(*reader)?.lock().map_err(|err| {
                        format_err!(
                            "Failed to acquire reader session mutex for id = {}: {}",
                            reader,
                            err
                        )
                    })?;

                    janus_callbacks::relay_rtcp(&reader_session, &mut packet);
                }
            }
        }

        Ok(())
    })
}

extern "C" fn incoming_data(_handle: *mut PluginSession, _packet: *mut PluginDataPacket) {
    // Dropping incoming data.
}

extern "C" fn data_ready(_handle: *mut PluginSession) {
    // Skip data channels.
}

extern "C" fn slow_link(handle: *mut PluginSession, uplink: c_int, video: c_int) {
    report_error(slow_link_impl(handle, uplink, video));
}

fn slow_link_impl(handle: *mut PluginSession, uplink: c_int, video: c_int) -> Result<()> {
    let session_id = session_id(handle)?;

    let rtc_id = app!()?
        .switchboard
        .with_read_lock(|switchboard| Ok(switchboard.stream_id_to(session_id)))?;

    info!(
        "Slow link: uplink = {}; is_video = {}", uplink, video;
        {"handle_id": session_id, "rtc_id": rtc_id}
    );

    Ok(())
}

extern "C" fn hangup_media(handle: *mut PluginSession) {
    report_error(hangup_media_impl(handle));
}

fn hangup_media_impl(handle: *mut PluginSession) -> Result<()> {
    let session_id = session_id(handle)?;

    let rtc_id = app!()?
        .switchboard
        .with_read_lock(|switchboard| Ok(switchboard.stream_id_to(session_id)))?;

    info!("Hang up"; {"handle_id": session_id, "rtc_id": rtc_id});

    app!()?
        .switchboard
        .with_write_lock(|mut switchboard| switchboard.disconnect(session_id))
}

extern "C" fn destroy_session(handle: *mut PluginSession, error: *mut c_int) {
    report_error(destroy_session_impl(handle, error));
}

fn destroy_session_impl(handle: *mut PluginSession, _error: *mut c_int) -> Result<()> {
    let session_id = session_id(handle)?;

    let rtc_id = app!()?
        .switchboard
        .with_read_lock(|switchboard| Ok(switchboard.stream_id_to(session_id)))?;

    info!("Handle destroyed"; {"handle_id": session_id, "rtc_id": rtc_id});

    app!()?
        .switchboard
        .with_write_lock(|mut switchboard| switchboard.handle_disconnect(session_id))
}

extern "C" fn destroy() {
    info!("Janus Conference plugin destroyed");
}

///////////////////////////////////////////////////////////////////////////////

fn session_id(handle: *mut PluginSession) -> Result<SessionId> {
    match unsafe { SessionWrapper::from_ptr(handle) } {
        Ok(session) => Ok(**session),
        Err(err) => bail!("Failed to get session: {}", err),
    }
}

fn send_pli(writer: SessionId) {
    report_error(send_pli_impl(writer));
}

fn send_pli_impl(writer: SessionId) -> Result<()> {
    app!()?.switchboard.with_read_lock(move |switchboard| {
        let session = switchboard.session(writer)?.lock().map_err(|err| {
            format_err!("Failed to acquire mutex for session {}: {}", writer, err)
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

fn send_fir(writer: SessionId) {
    report_error(send_fir_impl(writer));
}

fn send_fir_impl(writer: SessionId) -> Result<()> {
    app!()?.switchboard.with_read_lock(move |switchboard| {
        let session = switchboard.session(writer)?.lock().map_err(|err| {
            format_err!("Failed to acquire mutex for session {}: {}", writer, err)
        })?;

        let state = switchboard.state(writer)?;
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

fn send_remb(writer: SessionId, bitrate: u32) {
    report_error(send_remb_impl(writer, bitrate));
}

fn send_remb_impl(writer: SessionId, bitrate: u32) -> Result<()> {
    app!()?.switchboard.with_read_lock(move |switchboard| {
        let session = switchboard.session(writer)?.lock().map_err(|err| {
            format_err!("Failed to acquire mutex for session {}: {}", writer, err)
        })?;

        let mut remb = janus::rtcp::gen_remb(bitrate);

        let mut packet = PluginRtcpPacket {
            video: 1,
            buffer: remb.as_mut_ptr(),
            length: remb.len() as i16,
        };

        janus_callbacks::relay_rtcp(&session, &mut packet);
        Ok(())
    })
}

fn report_error(res: Result<()>) {
    match res {
        Ok(_) => {}
        Err(err) => {
            err!("{}", err);
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
    data_ready,
    slow_link,
    hangup_media,
    destroy_session,
    query_session
);

export_plugin!(&PLUGIN);
