use std::os::raw::{c_char, c_short};
use std::sync::mpsc;
use std::thread;

use anyhow::{format_err, Result};
use janus_plugin::{PluginRtcpPacket, PluginRtpExtensions, PluginRtpPacket};

use crate::janus_callbacks;
use crate::switchboard::{Session, SessionId};

///////////////////////////////////////////////////////////////////////////////

enum Job {
    RelayRtp(SessionId, OwnedRtpPacket),
    RelayRtcp(SessionId, OwnedRtcpPacket),
    Stop,
}

///////////////////////////////////////////////////////////////////////////////

trait Relayable {
    fn relay(&mut self, session_id: &Session);
}

///////////////////////////////////////////////////////////////////////////////

struct OwnedRtpPacket {
    video: c_char,
    buffer: Vec<c_char>,
    length: c_short,
    audio_level: c_char,
    audio_level_vad: c_char,
    video_rotation: c_short,
    video_back_camera: c_char,
    video_flipped: c_char,
}

impl OwnedRtpPacket {
    fn new(packet: &PluginRtpPacket) -> Self {
        let buffer_slice = unsafe {
            std::slice::from_raw_parts_mut(packet.buffer as *mut c_char, packet.length as usize)
        };

        Self {
            video: packet.video,
            buffer: buffer_slice.to_vec(),
            length: packet.length,
            audio_level: packet.extensions.audio_level,
            audio_level_vad: packet.extensions.audio_level_vad,
            video_rotation: packet.extensions.video_rotation,
            video_back_camera: packet.extensions.video_back_camera,
            video_flipped: packet.extensions.video_flipped,
        }
    }
}

impl Relayable for OwnedRtpPacket {
    fn relay(&mut self, session_id: &Session) {
        let mut plugin_rtp_packet = PluginRtpPacket {
            video: self.video,
            buffer: self.buffer.as_mut_ptr(),
            length: self.length,
            extensions: PluginRtpExtensions {
                audio_level: self.audio_level,
                audio_level_vad: self.audio_level_vad,
                video_rotation: self.video_rotation,
                video_back_camera: self.video_back_camera,
                video_flipped: self.video_flipped,
            },
        };

        janus_callbacks::relay_rtp(session_id, &mut plugin_rtp_packet);
    }
}

///////////////////////////////////////////////////////////////////////////////

struct OwnedRtcpPacket {
    video: c_char,
    buffer: Vec<c_char>,
    length: c_short,
}

impl OwnedRtcpPacket {
    fn new(packet: &PluginRtcpPacket) -> Self {
        let buffer_slice = unsafe {
            std::slice::from_raw_parts_mut(packet.buffer as *mut c_char, packet.length as usize)
        };

        Self {
            video: packet.video,
            buffer: buffer_slice.to_vec(),
            length: packet.length,
        }
    }
}

impl Relayable for OwnedRtcpPacket {
    fn relay(&mut self, session_id: &Session) {
        let mut plugin_rtcp_packet = PluginRtcpPacket {
            video: self.video,
            buffer: self.buffer.as_mut_ptr(),
            length: self.length,
        };

        janus_callbacks::relay_rtcp(session_id, &mut plugin_rtcp_packet);
    }
}

///////////////////////////////////////////////////////////////////////////////

pub struct Relay {
    senders: Vec<mpsc::Sender<Job>>,
    join_handles: Option<Vec<thread::JoinHandle<()>>>,
}

impl Relay {
    pub fn new(size: usize) -> Self {
        let mut senders = Vec::with_capacity(size);
        let mut join_handles = Vec::with_capacity(size);

        for _ in 0..size {
            let (tx, rx) = mpsc::channel();

            let join_handle = thread::spawn(move || {
                for job in rx {
                    match job {
                        Job::Stop => break,
                        Job::RelayRtp(session_id, packet) => {
                            if let Err(err) = relay(session_id, packet) {
                                janus_err!("[CONFERENCE] Failed to relay RTP packet: {}", err);
                            }
                        }
                        Job::RelayRtcp(session_id, packet) => {
                            if let Err(err) = relay(session_id, packet) {
                                janus_err!("[CONFERENCE] Failed to relay RTCP packet: {}", err);
                            }
                        }
                    }
                }
            });

            senders.push(tx);
            join_handles.push(join_handle);
        }

        Self {
            senders,
            join_handles: Some(join_handles),
        }
    }

    pub fn relay_rtp(&self, session_id: SessionId, packet: &PluginRtpPacket) -> Result<()> {
        let job = Job::RelayRtp(session_id, OwnedRtpPacket::new(packet));
        self.schedule_job(session_id, job)
    }

    pub fn relay_rtcp(&self, session_id: SessionId, packet: &PluginRtcpPacket) -> Result<()> {
        let job = Job::RelayRtcp(session_id, OwnedRtcpPacket::new(packet));
        self.schedule_job(session_id, job)
    }

    fn schedule_job(&self, session_id: SessionId, job: Job) -> Result<()> {
        let idx = (session_id.as_u128() % self.senders.len() as u128) as usize;

        self.senders[idx]
            .send(job)
            .map_err(|err| format_err!("Failed to send relay job to sender {}: {}", idx, err))
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        for tx in &self.senders {
            if let Err(err) = tx.send(Job::Stop) {
                janus_err!(
                    "[CONFERENCE] Failed to send stop job to a relay thread: {}",
                    err
                );
            }
        }

        match self.join_handles.take() {
            None => janus_err!("[CONFERENCE] Failed to get join handles"),
            Some(join_handles) => {
                for join_handle in join_handles.into_iter() {
                    if let Err(err) = join_handle.join() {
                        janus_err!("[CONFERENCE] Failed to join relay thread: {:?}", err);
                    }
                }
            }
        }
    }
}

///////////////////////////////////////////////////////////////////////////////

fn relay<P: Relayable>(session_id: SessionId, mut packet: P) -> Result<()> {
    app!()?.switchboard.with_read_lock(|switchboard| {
        let session = switchboard.session(session_id)?.lock().map_err(|err| {
            format_err!(
                "Failed to acquire session mutex id = {}: {}",
                session_id,
                err,
            )
        })?;

        packet.relay(&session);
        Ok(())
    })
}
