#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate janus_plugin as janus;
#[macro_use]
extern crate serde_derive;

use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::slice;
use std::{
    ffi::{CStr, CString},
    time::Instant,
};

use anyhow::{bail, Context, Result};
use chrono::Utc;
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
pub mod janus_rtp;
mod jsep;
mod message_handler;
mod metrics;
mod recorder;
mod serde;
mod switchboard;
#[cfg(test)]
mod test_stubs;

use app::App;
use conf::Config;
use janus_rtp::JanusRtpHeader;
use switchboard::{SessionId, Switchboard};

use crate::{
    janus_rtp::AudioLevel,
    message_handler::{handle_request, prepare_request, send_response, send_speaking_notification},
    metrics::Metrics,
};

const INITIAL_REMBS: u64 = 4;

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

    Config::from_path(config_path)
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

    app!()?.switchboard.with_write_lock(|mut switchboard| {
        if switchboard.sessions_count() == 0 {
            switchboard.insert_service_session(session)
        } else {
            switchboard.insert_new(session);
        }
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
        Ok(()) => {
            Metrics::observe_success_request();
            PluginResult::ok_wait(None).into_raw()
        }
        Err(err) => {
            Metrics::observe_failed_request();
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
    let now = Instant::now();
    let session_id = session_id(handle)?;
    verb!("Incoming message"; {"handle_id": session_id});

    let transaction = match unsafe { CString::from_raw(transaction) }.to_str() {
        Ok(transaction) => String::from(transaction),
        Err(err) => bail!("Failed to serialize transaction: {}", err),
    };

    if let Some(json) = unsafe { JanssonValue::from_raw(message) } {
        let janus_sender = app!()?.janus_sender.clone();
        let jsep_offer = unsafe { JanssonValue::from_raw(jsep) };
        let request = prepare_request(session_id, &transaction, &json, jsep_offer)?;
        async_std::task::spawn(async move {
            let method_kind = request.method_kind();
            let response = handle_request(request).await;
            send_response(janus_sender, response);
            if let Some(method) = method_kind {
                Metrics::observe_request(now, method)
            }
        });
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
    app!()?.switchboard.with_read_lock(|switchboard| {
        if let Some(publisher) = switchboard.publisher_to(session_id) {
            send_fir(publisher, &switchboard);
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
    let mut packet = unsafe { &mut *packet };
    let is_video = matches!(packet.video, 1);
    let header = JanusRtpHeader::extract(packet);
    // Touch last packet timestamp  to drop timeout.
    let session_id = session_id(handle)?;
    app.switchboard.with_read_lock(|switchboard| {
        let state = switchboard.state(session_id)?;
        let is_speaking =
        app.config.speaking_notifications
            .as_ref()
            .filter(|_| !is_video)
            .and_then(|config| {
                let agent_id = switchboard.agent_id(session_id)?;
                let is_speaking = state.is_speaking(AudioLevel::new(packet, state.audio_level_ext_id()?)?,  config)?;
                Some((agent_id, is_speaking))
            });

        if let Some((agent_id, is_speaking)) = is_speaking {
            verb!("Sending speaking notification: is_speaking: {}, agent_id: {}", is_speaking, agent_id);
            if let Err(err) = send_speaking_notification(&app.janus_sender, session_id, agent_id, is_speaking) {
                err!("Sending spaking notification errored: {:?}", err; { "session_id": session_id, "agent_id": agent_id });
            }
        }
        state.touch_last_rtp_packet_timestamp();

        // Check whether publisher media is muted and drop the packet if it is.
        let stream_id = switchboard
            .published_by(session_id)
            .ok_or_else(|| anyhow!("Failed to identify the stream id of the packet"))?;

        let writer_config = switchboard.writer_config(stream_id);

        // Send incremental initial or regular REMB to the publisher if needed to control bitrate.
        // Do it only for video because Windows and Linux don't make a difference for media types
        // and apply audio limitation to video while only MacOS does.
        let remb_interval = chrono::Duration::seconds(5);
        if is_video {
            let now = Utc::now();
            if now - state.last_fir_timestamp() >= app.fir_interval {
                send_fir(session_id, &switchboard);
            }
            let target_bitrate = writer_config.video_remb();
            let initial_rembs_left = INITIAL_REMBS - state.initial_rembs_counter();

            if initial_rembs_left > 0 {
                let bitrate = target_bitrate / initial_rembs_left as u32;
                send_remb(session_id, bitrate);
                state.touch_last_remb_timestamp();
                state.increment_initial_rembs_counter();
            } else if let Some(last_remb_timestamp) = state.last_remb_timestamp() {
                if now - last_remb_timestamp >= remb_interval {
                    send_remb(session_id, target_bitrate);
                    state.touch_last_remb_timestamp();
                }
            }
        }

        // Retransmit packet to publishers as is.
        for subscriber_id in switchboard.subscribers_to(session_id) {
            // Check whether media is muted by the agent.
            let is_relay_packet = switchboard
                .reader_config(stream_id, subscriber_id)
                .map(|reader_config| match is_video {
                    true => reader_config.receive_video(),
                    false => reader_config.receive_audio(),
                })
                .unwrap_or(true);

            if is_relay_packet {
                match relay_rtp_packet(&switchboard, *subscriber_id, &mut packet, &header) {
                    Ok(()) => (),
                    Err(err) => huge!(
                        "Failed to relay an RTP packet: {}", err;
                        {"handle_id": subscriber_id, "rtc_id": stream_id}
                    ),
                }
            }
        }

        // Push packet to the recorder.
        if let Some(recorder) = state.recorder() {
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
    let session_id = session_id(handle)?;
    let mut packet = unsafe { &mut *packet };
    let data = unsafe { slice::from_raw_parts_mut(packet.buffer, packet.length as usize) };

    app!()?.switchboard.with_read_lock(|switchboard| {
        match packet.video {
            1 if janus::rtcp::has_pli(data) => {
                if let Some(publisher) = switchboard.publisher_to(session_id) {
                    send_pli(publisher, &switchboard);
                }
            }
            1 if janus::rtcp::has_fir(data) => {
                if let Some(publisher) = switchboard.publisher_to(session_id) {
                    send_fir(publisher, &switchboard);
                }
            }
            _ => {
                for subscriber in switchboard.subscribers_to(session_id) {
                    let subscriber_session = switchboard.session(*subscriber)?;

                    janus_callbacks::relay_rtcp(subscriber_session, &mut packet);
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

    app!()?.switchboard.with_read_lock(|switchboard| {
        let rtc_id = switchboard.stream_id_to(session_id);
        info!("Hang up"; {"handle_id": session_id, "rtc_id": rtc_id});
        switchboard.disconnect(session_id)
    })
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

    let reader_session = switchboard.session(reader)?;

    janus_callbacks::relay_rtp(reader_session, packet);

    // Restore original header rewritten by `janus_rtp_header_update`
    // for the next iteration of the loop.
    original_header.restore(packet);
    Ok(())
}

fn send_pli(publisher: SessionId, switchboard: &Switchboard) {
    report_error(send_pli_impl(publisher, switchboard));
}

fn send_pli_impl(publisher: SessionId, switchboard: &Switchboard) -> Result<()> {
    let session = switchboard.session(publisher)?;

    let mut pli = janus::rtcp::gen_pli();

    let mut packet = PluginRtcpPacket {
        video: 1,
        buffer: pli.as_mut_ptr(),
        length: pli.len() as i16,
    };

    janus_callbacks::relay_rtcp(session, &mut packet);
    Ok(())
}

fn send_fir(publisher: SessionId, switchboard: &Switchboard) {
    report_error(send_fir_impl(publisher, switchboard));
}

fn send_fir_impl(publisher: SessionId, switchboard: &Switchboard) -> Result<()> {
    let session = switchboard.session(publisher)?;

    let state = switchboard.state(publisher)?;
    state.touch_last_fir_timestamp();
    let mut seq = state.increment_fir_seq();
    let mut fir = janus::rtcp::gen_fir(&mut seq);

    let mut packet = PluginRtcpPacket {
        video: 1,
        buffer: fir.as_mut_ptr(),
        length: fir.len() as i16,
    };

    janus_callbacks::relay_rtcp(session, &mut packet);
    Ok(())
}

fn send_remb(publisher: SessionId, bitrate: u32) {
    verb!("Sending REMB bitrate = {}", bitrate; {"handle_id": publisher});
    report_error(send_remb_impl(publisher, bitrate));
}

fn send_remb_impl(publisher: SessionId, bitrate: u32) -> Result<()> {
    app!()?.switchboard.with_read_lock(move |switchboard| {
        let session = switchboard.session(publisher)?;

        let mut remb = janus::rtcp::gen_remb(bitrate);

        let mut packet = PluginRtcpPacket {
            video: 1,
            buffer: remb.as_mut_ptr(),
            length: remb.len() as i16,
        };

        janus_callbacks::relay_rtcp(session, &mut packet);
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

static PLUGIN: Plugin = build_plugin!(
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
