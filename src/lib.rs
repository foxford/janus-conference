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
use janus_rtp::JanusRtpHeader;
use switchboard::{SessionId, Switchboard};

const INITIAL_REMBS: u64 = 4;

lazy_static! {
    static ref REMB_INTERVAL: Duration = Duration::seconds(5);
}

// These type unsafely allow passing C pointers to RTP/RTCP packets to another thread.
// However we must ensure blocking the thread that called `incoming_rtp/rtcp` callback until the
// asynchronous processing is over to prevent C from freeing the memory too early.
struct RtpPacket(*mut PluginRtpPacket);
unsafe impl Send for RtpPacket {}
unsafe impl Sync for RtpPacket {}

struct RtcpPacket(*mut PluginRtcpPacket);
unsafe impl Send for RtcpPacket {}
unsafe impl Sync for RtcpPacket {}

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

    app!()?
        .switchboard_dispatcher
        .dispatch_sync(move |switchboard| switchboard.connect(session))?
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
    let session_id = session_id(handle)?;

    app!()?
        .switchboard_dispatcher
        .dispatch_sync(move |switchboard| {
            if let Some(writer) = switchboard.writer_to(session_id) {
                send_fir(&switchboard, writer);
            }

            info!(
                "WebRTC media is now available";
                {"handle_id": session_id, "rtc_id": switchboard.stream_id_to(session_id)}
            );
        })?;

    Ok(())
}

extern "C" fn incoming_rtp(handle: *mut PluginSession, packet: *mut PluginRtpPacket) {
    report_error(incoming_rtp_impl(handle, packet));
}

fn incoming_rtp_impl(handle: *mut PluginSession, packet: *mut PluginRtpPacket) -> Result<()> {
    let app = app!()?;
    let session_id = session_id(handle)?;
    let packet = RtpPacket(packet);

    app.switchboard_dispatcher
        .dispatch_sync(move |switchboard| {
            let mut packet = unsafe { &mut *packet.0 };
            let state = switchboard.state(session_id)?;

            // Touch last packet timestamp to drop timeout.
            state.touch_last_rtp_packet_timestamp();

            // Send incremental initial or regular REMB to the writer if needed to control bitrate.
            if let Some(target_bitrate) = app.config.constraint.writer.bitrate {
                let initial_rembs_left = INITIAL_REMBS - state.initial_rembs_counter();

                if initial_rembs_left > 0 {
                    let bitrate = target_bitrate / initial_rembs_left as u32;
                    send_remb(&switchboard, session_id, bitrate);
                    state.touch_last_remb_timestamp();
                    state.increment_initial_rembs_counter();
                } else if let Some(last_remb_timestamp) = state.last_remb_timestamp() {
                    if Utc::now() - last_remb_timestamp >= *REMB_INTERVAL {
                        send_remb(&switchboard, session_id, target_bitrate);
                        state.touch_last_remb_timestamp();
                    }
                }
            }

            let video = packet.video;
            let header = JanusRtpHeader::extract(&packet);

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

            // Relay packet to readers.
            // For each reader we clone the packet and change the header according to his own
            // switching context to avoid sending identical packets and to maintain SSRC switching.
            for reader in switchboard.readers_of(stream_id) {
                let result = relay_rtp_packet(&switchboard, *reader, &mut packet, &header);

                if let Err(err) = result {
                    huge!(
                        "Failed to relay an RTP packet: {}", err;
                        {"handle_id": reader, "rtc_id": stream_id}
                    );
                }
            }

            // Push packet to the recorder.
            if let Some(recorder) = state.recorder() {
                let buf = unsafe {
                    std::slice::from_raw_parts(packet.buffer as *const i8, packet.length as usize)
                };

                recorder.record_packet(buf, video == 1)?;
            }

            Ok(())
        })?
}

fn relay_rtp_packet(
    switchboard: &Switchboard,
    reader: SessionId,
    packet: &mut PluginRtpPacket,
    original_header: &JanusRtpHeader,
) -> Result<()> {
    let reader_state = switchboard.state(reader)?;

    reader_state
        .switching_context()
        .update_rtp_packet_header(packet)?;

    let reader_session = switchboard.session(reader)?.lock().map_err(|err| {
        format_err!(
            "Failed to acquire reader session mutex id = {}: {}",
            reader,
            err
        )
    })?;

    janus_callbacks::relay_rtp(&reader_session, packet);

    // Restore original header rewritten by `janus_rtp_header_update`
    // for the next iteration of the loop.
    original_header.restore(packet);
    Ok(())
}

extern "C" fn incoming_rtcp(handle: *mut PluginSession, packet: *mut PluginRtcpPacket) {
    report_error(incoming_rtcp_impl(handle, packet));
}

fn incoming_rtcp_impl(handle: *mut PluginSession, packet: *mut PluginRtcpPacket) -> Result<()> {
    let session_id = session_id(handle)?;
    let packet = RtcpPacket(packet);

    app!()?
        .switchboard_dispatcher
        .dispatch_sync(move |switchboard| {
            let mut packet = unsafe { &mut *packet.0 };
            let data = unsafe { slice::from_raw_parts_mut(packet.buffer, packet.length as usize) };

            match packet.video {
                1 if janus::rtcp::has_pli(data) => {
                    if let Some(writer) = switchboard.writer_to(session_id) {
                        send_pli(&switchboard, writer);
                    }
                }
                1 if janus::rtcp::has_fir(data) => {
                    if let Some(writer) = switchboard.writer_to(session_id) {
                        send_fir(&switchboard, writer);
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
                        let reader_session =
                            switchboard.session(*reader)?.lock().map_err(|err| {
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

            if let Some(recorder) = switchboard.state(session_id)?.recorder() {
                let buf = unsafe {
                    std::slice::from_raw_parts(packet.buffer as *const i8, packet.length as usize)
                };

                recorder.record_packet(buf, packet.video == 1)?;
            }

            Ok(())
        })?
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
    let session_id_clone = session_id.clone();

    let rtc_id = app!()?
        .switchboard_dispatcher
        .dispatch_sync(move |switchboard| switchboard.stream_id_to(session_id_clone))?;

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

    app!()?
        .switchboard_dispatcher
        .dispatch_sync(move |switchboard| {
            let stream_id = switchboard.stream_id_to(session_id);
            info!("Hang up"; {"handle_id": session_id, "rtc_id": stream_id});
            switchboard.disconnect(session_id)
        })?
}

extern "C" fn destroy_session(handle: *mut PluginSession, error: *mut c_int) {
    report_error(destroy_session_impl(handle, error));
}

fn destroy_session_impl(handle: *mut PluginSession, _error: *mut c_int) -> Result<()> {
    let session_id = session_id(handle)?;

    app!()?
        .switchboard_dispatcher
        .dispatch_sync(move |switchboard| {
            let stream_id = switchboard.stream_id_to(session_id);
            info!("Handle destroyed"; {"handle_id": session_id, "rtc_id": stream_id});
            switchboard.handle_disconnect(session_id)
        })?
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

fn send_pli(switchboard: &Switchboard, writer: SessionId) {
    report_error(send_pli_impl(switchboard, writer));
}

fn send_pli_impl(switchboard: &Switchboard, writer: SessionId) -> Result<()> {
    let session = switchboard
        .session(writer)?
        .lock()
        .map_err(|err| format_err!("Failed to acquire mutex for session {}: {}", writer, err))?;

    let mut pli = janus::rtcp::gen_pli();

    let mut packet = PluginRtcpPacket {
        video: 1,
        buffer: pli.as_mut_ptr(),
        length: pli.len() as i16,
    };

    janus_callbacks::relay_rtcp(&session, &mut packet);
    Ok(())
}

fn send_fir(switchboard: &Switchboard, writer: SessionId) {
    report_error(send_fir_impl(switchboard, writer));
}

fn send_fir_impl(switchboard: &Switchboard, writer: SessionId) -> Result<()> {
    let session = switchboard
        .session(writer)?
        .lock()
        .map_err(|err| format_err!("Failed to acquire mutex for session {}: {}", writer, err))?;

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
}

fn send_remb(switchboard: &Switchboard, writer: SessionId, bitrate: u32) {
    report_error(send_remb_impl(switchboard, writer, bitrate));
}

fn send_remb_impl(switchboard: &Switchboard, writer: SessionId, bitrate: u32) -> Result<()> {
    let session = switchboard
        .session(writer)?
        .lock()
        .map_err(|err| format_err!("Failed to acquire mutex for session {}: {}", writer, err))?;

    let mut remb = janus::rtcp::gen_remb(bitrate);

    let mut packet = PluginRtcpPacket {
        video: 1,
        buffer: remb.as_mut_ptr(),
        length: remb.len() as i16,
    };

    janus_callbacks::relay_rtcp(&session, &mut packet);
    Ok(())
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
