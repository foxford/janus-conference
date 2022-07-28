#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int, c_long, c_short, c_uint, c_ushort};
use std::{convert::TryInto, mem::MaybeUninit};
use std::{
    ffi::CStr,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Result};
use janus::PluginRtpPacket;

////////////////////////////////////////////////////////////////////////////////

pub static JANUS_RTP_EXTMAP_AUDIO_LEVEL: &str = "urn:ietf:params:rtp-hdrext:ssrc-audio-level";

pub fn janus_rtp_extmap_audio_level() -> &'static CStr {
    c_str!("urn:ietf:params:rtp-hdrext:ssrc-audio-level")
}

#[derive(Debug)]
pub struct JanusRtpSwitchingContext {
    locked_context: Arc<Mutex<janus_rtp_switching_context>>,
}

impl JanusRtpSwitchingContext {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mut uninit_context = MaybeUninit::<janus_rtp_switching_context>::uninit();

        let context = unsafe {
            janus_rtp_switching_context_reset(uninit_context.as_mut_ptr());
            uninit_context.assume_init()
        };

        Self {
            locked_context: Arc::new(Mutex::new(context)),
        }
    }

    pub fn update_rtp_packet_header(&self, packet: &mut PluginRtpPacket) -> Result<()> {
        let mut context = self
            .locked_context
            .lock()
            .map_err(|err| anyhow!("Failed to acquire RTP switching context mutex: {}", err))?;

        let video = matches!(packet.video, 1).into();

        #[allow(unused_unsafe)]
        unsafe {
            janus_rtp_header_update(packet.buffer, &mut *context, video, 0)
        };

        Ok(())
    }
}

pub struct JanusRtpHeader(janus_rtp_header);

impl JanusRtpHeader {
    pub fn extract(packet: &PluginRtpPacket) -> Self {
        let mut uninit_header = MaybeUninit::<janus_rtp_header>::uninit();

        Self(unsafe {
            std::ptr::copy(
                packet.buffer,
                uninit_header.as_mut_ptr() as *mut i8,
                RTP_HEADER_SIZE,
            );
            uninit_header.assume_init()
        })
    }

    pub fn restore(&self, packet: &mut PluginRtpPacket) {
        unsafe { std::ptr::copy(&self.0 as *const i8, &mut *packet.buffer, RTP_HEADER_SIZE) };
    }
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub struct AudioLevel(u8);

impl AudioLevel {
    pub fn new(packet: &mut PluginRtpPacket, audio_level_ext_id: u32) -> Option<Self> {
        let mut vad = false as gboolean;
        let mut level = -1_i32;
        unsafe {
            janus_rtp_header_extension_parse_audio_level(
                packet.buffer,
                packet.length as c_int,
                audio_level_ext_id as c_int,
                &mut vad,
                &mut level,
            );
        };
        level.try_into().ok().map(Self)
    }

    pub fn as_usize(self) -> usize {
        self.0 as usize
    }

    #[cfg(test)]
    pub fn from_u8(x: u8) -> Self {
        Self(x)
    }
}

////////////////////////////////////////////////////////////////////////////////

type gboolean = c_int;
type gint16 = c_short;
type gint32 = c_int;
type gint64 = c_long;
type uint16_t = c_ushort;
type uint32_t = c_uint;

const RTP_HEADER_SIZE: usize = 12;
type janus_rtp_header = [i8; RTP_HEADER_SIZE];

pub fn replace_payload_with_zeros(packet: &mut PluginRtpPacket) {
    unsafe {
        std::ptr::write_bytes(
            &mut packet.buffer.add(RTP_HEADER_SIZE),
            0,
            packet.length as usize - RTP_HEADER_SIZE,
        )
    }
}

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

#[cfg(not(test))]
extern "C" {
    fn janus_rtp_header_extension_parse_audio_level(
        packet: *mut c_char,
        len: c_int,
        id: c_int,
        vad: *mut gboolean,
        level: *mut c_int,
    ) -> c_int;

    fn janus_rtp_switching_context_reset(context: *mut janus_rtp_switching_context);

    fn janus_rtp_header_update(
        header: *mut c_char,
        context: *mut janus_rtp_switching_context,
        video: gboolean,
        step: c_int,
    );
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[no_mangle]
unsafe extern "C" fn janus_rtp_header_extension_parse_audio_level(
    _packet: *mut c_char,
    _len: c_int,
    _id: c_int,
    _vad: *mut gboolean,
    _level: *mut c_int,
) -> c_int {
    1
}

#[cfg(test)]
#[no_mangle]
unsafe extern "C" fn janus_rtp_switching_context_reset(_context: *mut janus_rtp_switching_context) {
}

#[cfg(test)]
#[no_mangle]
unsafe extern "C" fn janus_rtp_header_update(
    _header: *mut c_char,
    _context: *mut janus_rtp_switching_context,
    _video: gboolean,
    _step: c_int,
) {
}
