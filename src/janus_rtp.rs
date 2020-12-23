#![allow(non_camel_case_types)]

use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int, c_long, c_short, c_uint, c_ushort};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use janus::PluginRtpPacket;

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct JanusRtpSwitchingContext {
    locked_context: Arc<Mutex<janus_rtp_switching_context>>,
}

impl JanusRtpSwitchingContext {
    pub fn new() -> Self {
        let mut uninit_context = MaybeUninit::<janus_rtp_switching_context>::uninit();

        let context = unsafe {
            janus_rtp_switching_context_reset(uninit_context.as_mut_ptr());
            uninit_context.assume_init()
        };

        Self { locked_context: Arc::new(Mutex::new(context)) }
    }

    pub fn update_rtp_packet_header(&self, packet: &mut PluginRtpPacket) -> Result<()> {
        let mut context = self.locked_context.lock().map_err(|err| {
            anyhow!("Failed to acquire RTP switching context mutex: {}", err)
        })?;

        let video = matches!(packet.video, 1).into();
        unsafe { janus_rtp_header_update(packet.buffer, &mut *context, video, 0) };
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////

type gboolean = c_int;
type gint16 = c_short;
type gint32 = c_int;
type gint64 = c_long;
type uint16_t = c_ushort;
type uint32_t = c_uint;

#[derive(Debug)]
#[repr(C)]
struct janus_rtp_switching_context {
    a_last_ssrc: uint32_t,
    a_last_ts: uint32_t,
    a_base_ts: uint32_t,
    a_base_ts_prev: uint32_t,
    a_prev_ts: uint32_t,
    a_target_ts: uint32_t,
    a_start_ts: uint32_t,
    v_last_ssrc: uint32_t,
    v_last_ts: uint32_t,
    v_base_ts: uint32_t,
    v_base_ts_prev: uint32_t,
    v_prev_ts: uint32_t,
    v_target_ts: uint32_t,
    v_start_ts: uint32_t,
    a_last_seq: uint16_t,
    a_prev_seq: uint16_t,
    a_base_seq: uint16_t,
    a_base_seq_prev: uint16_t,
    v_last_seq: uint16_t,
    v_prev_seq: uint16_t,
    v_base_seq: uint16_t,
    v_base_seq_prev: uint16_t,
    a_seq_reset: gboolean,
    a_new_ssrc: gboolean,
    v_seq_reset: gboolean,
    v_new_ssrc: gboolean,
    a_seq_offset: gint16,
    v_seq_offset: gint16,
    a_prev_delay: gint32,
    a_active_delay: gint32,
    a_ts_offset: gint32,
    v_prev_delay: gint32,
    v_active_delay: gint32,
    v_ts_offset: gint32,
    a_last_time: gint64,
    a_reference_time: gint64,
    a_start_time: gint64,
    a_evaluating_start_time: gint64,
    v_last_time: gint64,
    v_reference_time: gint64,
    v_start_time: gint64,
    v_evaluating_start_time: gint64,
}

extern "C" {
    fn janus_rtp_switching_context_reset(context: *mut janus_rtp_switching_context);

    fn janus_rtp_header_update(
        header: *mut c_char,
        context: *mut janus_rtp_switching_context,
        video: gboolean,
        step: c_int,
    );
}
