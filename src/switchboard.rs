use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::{fmt, usize};
use std::{
    sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU64, AtomicUsize, Ordering},
    time::{Duration, Instant},
};

use anyhow::{bail, format_err, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use fnv::FnvHashMap;
use janus::session::SessionWrapper;
use once_cell::sync::Lazy;
use uuid::Uuid;

use crate::janus_rtp::JanusRtpSwitchingContext;
use crate::recorder::RecorderHandle;
use crate::{bidirectional_multimap::BidirectionalMultimap, janus_rtp::AudioLevel};
use crate::{conf::SpeakingNotifications, janus_callbacks};

///////////////////////////////////////////////////////////////////////////////

pub type StreamId = Uuid;
pub type AgentId = String;
pub type Session = Box<Arc<SessionWrapper<SessionId>>>;

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(u64);

impl SessionId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct SessionState {
    switching_context: JanusRtpSwitchingContext,
    fir_seq: AtomicI32,
    is_speaking: AtomicBool,
    packets_count: AtomicUsize,
    audio_level_sum: AtomicUsize,
    initial_rembs_counter: AtomicU64,
    last_remb_timestamp: AtomicI64,
    last_fir_timestamp: AtomicI64,
    last_rtp_packet_timestamp: AtomicI64,
    recorder: Option<RecorderHandle>,
    audio_level_ext_id: Option<u32>,
}

impl SessionState {
    fn new() -> Self {
        Self {
            switching_context: JanusRtpSwitchingContext::new(),
            fir_seq: AtomicI32::new(0),
            initial_rembs_counter: AtomicU64::new(0),
            last_remb_timestamp: AtomicI64::new(0),
            last_rtp_packet_timestamp: AtomicI64::new(0),
            recorder: None,
            last_fir_timestamp: AtomicI64::new(0),
            is_speaking: AtomicBool::new(false),
            packets_count: AtomicUsize::new(0),
            audio_level_sum: AtomicUsize::new(0),
            audio_level_ext_id: None,
        }
    }

    pub fn is_speaking(
        &self,
        audio_level: AudioLevel,
        config: &SpeakingNotifications,
    ) -> Option<bool> {
        let packets_count = self.packets_count.fetch_add(1, Ordering::Relaxed) + 1;
        self.audio_level_sum
            .fetch_add(audio_level.as_usize(), Ordering::Relaxed);
        if packets_count == config.audio_active_packets {
            self.packets_count.store(0, Ordering::Relaxed);
            let level_avg = self.audio_level_sum.swap(0, Ordering::Relaxed) / packets_count;
            let is_speaking = self.is_speaking.load(Ordering::Relaxed);
            if !is_speaking && level_avg < config.speaking_average_level.as_usize() {
                self.is_speaking.store(true, Ordering::Relaxed);
                return Some(true);
            }
            if is_speaking && level_avg > config.not_speaking_average_level.as_usize() {
                self.is_speaking.store(false, Ordering::Relaxed);
                return Some(false);
            }
            None
        } else {
            None
        }
    }

    pub fn switching_context(&self) -> &JanusRtpSwitchingContext {
        &self.switching_context
    }

    pub fn increment_fir_seq(&self) -> i32 {
        self.fir_seq.fetch_add(1, Ordering::Relaxed)
    }

    pub fn initial_rembs_counter(&self) -> u64 {
        self.initial_rembs_counter.load(Ordering::Relaxed)
    }

    pub fn increment_initial_rembs_counter(&self) -> u64 {
        self.initial_rembs_counter.fetch_add(1, Ordering::Relaxed)
    }

    pub fn last_remb_timestamp(&self) -> Option<DateTime<Utc>> {
        match self.last_remb_timestamp.load(Ordering::Relaxed) {
            0 => None,
            timestamp => {
                let naive_dt = NaiveDateTime::from_timestamp(timestamp, 0);
                Some(DateTime::from_utc(naive_dt, Utc))
            }
        }
    }

    pub fn last_fir_timestamp(&self) -> DateTime<Utc> {
        let naive_dt =
            NaiveDateTime::from_timestamp(self.last_fir_timestamp.load(Ordering::Relaxed), 0);
        DateTime::from_utc(naive_dt, Utc)
    }

    pub fn touch_last_remb_timestamp(&self) {
        self.last_remb_timestamp
            .store(Utc::now().timestamp(), Ordering::Relaxed);
    }

    pub fn touch_last_fir_timestamp(&self) {
        self.last_fir_timestamp
            .store(Utc::now().timestamp(), Ordering::Relaxed);
    }

    fn since_last_rtp_packet_timestamp(&self) -> Option<chrono::Duration> {
        match self.last_rtp_packet_timestamp.load(Ordering::Relaxed) {
            0 => None,
            timestamp => {
                let naive_dt = NaiveDateTime::from_timestamp(timestamp, 0);
                let dt = DateTime::from_utc(naive_dt, Utc);
                Some(Utc::now() - dt)
            }
        }
    }

    pub fn touch_last_rtp_packet_timestamp(&self) {
        self.last_rtp_packet_timestamp
            .store(Utc::now().timestamp(), Ordering::Relaxed);
    }

    pub fn recorder(&self) -> Option<&RecorderHandle> {
        self.recorder.as_ref()
    }

    pub fn recorder_mut(&mut self) -> Option<&mut RecorderHandle> {
        self.recorder.as_mut()
    }

    pub fn set_recorder(&mut self, recorder: RecorderHandle) -> &mut Self {
        self.recorder = Some(recorder);
        self
    }

    fn unset_recorder(&mut self) -> &mut Self {
        self.recorder = None;
        self
    }

    /// Set the session state's audio level ext id.
    pub fn set_audio_level_ext_id(&mut self, audio_level_ext_id: Option<u32>) {
        self.audio_level_ext_id = audio_level_ext_id;
    }

    /// Get a reference to the session state's audio level ext id.
    pub fn audio_level_ext_id(&self) -> Option<u32> {
        self.audio_level_ext_id
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct ReaderConfig {
    receive_video: bool,
    receive_audio: bool,
}

impl ReaderConfig {
    pub fn new(receive_video: bool, receive_audio: bool) -> Self {
        Self {
            receive_video,
            receive_audio,
        }
    }

    pub fn receive_video(&self) -> bool {
        self.receive_video
    }

    pub fn receive_audio(&self) -> bool {
        self.receive_audio
    }
}

#[derive(Debug)]
pub struct WriterConfig {
    send_video: bool,
    send_audio: bool,
    video_remb: u32,
}

impl WriterConfig {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn send_video(&self) -> bool {
        self.send_video
    }

    pub fn set_send_video(&mut self, send_video: bool) -> &mut Self {
        self.send_video = send_video;
        self
    }

    pub fn send_audio(&self) -> bool {
        self.send_audio
    }

    pub fn set_send_audio(&mut self, send_audio: bool) -> &mut Self {
        self.send_audio = send_audio;
        self
    }

    pub fn video_remb(&self) -> u32 {
        self.video_remb
    }

    pub fn set_video_remb(&mut self, video_remb: u32) -> &mut Self {
        self.video_remb = video_remb;
        self
    }
}

impl Default for WriterConfig {
    fn default() -> Self {
        let app = app!().expect("Plugin is not initialized");

        Self {
            send_video: true,
            send_audio: true,
            video_remb: app.config.constraint.writer.default_video_bitrate,
        }
    }
}

static DEFAULT_WRITER_CONFIG: Lazy<WriterConfig> = Lazy::new(Default::default);

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct UnusedSession {
    pub created_at: Instant,
    pub session: Session,
}

impl UnusedSession {
    pub fn is_timeouted(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() > ttl
    }
}

#[derive(Debug)]
pub struct Switchboard {
    unused_sessions: FnvHashMap<SessionId, UnusedSession>,
    sessions: FnvHashMap<SessionId, Session>,
    states: FnvHashMap<SessionId, SessionState>,
    agents: BidirectionalMultimap<AgentId, SessionId>,
    publishers: FnvHashMap<StreamId, SessionId>,
    publishers_subscribers: BidirectionalMultimap<SessionId, SessionId>,
    reader_configs: FnvHashMap<AgentId, FnvHashMap<StreamId, ReaderConfig>>,
    writer_configs: FnvHashMap<StreamId, WriterConfig>,
}

impl Switchboard {
    pub fn new() -> Self {
        Self {
            sessions: FnvHashMap::default(),
            states: FnvHashMap::default(),
            agents: BidirectionalMultimap::new(),
            publishers: FnvHashMap::default(),
            publishers_subscribers: BidirectionalMultimap::new(),
            reader_configs: FnvHashMap::default(),
            writer_configs: FnvHashMap::default(),
            unused_sessions: FnvHashMap::default(),
        }
    }

    pub fn unused_sessions_count(&self) -> usize {
        self.unused_sessions.len()
    }

    pub fn sessions_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn agents_count(&self) -> usize {
        self.agents.keys_count()
    }

    pub fn publishers_count(&self) -> usize {
        self.publishers.len()
    }

    pub fn publishers_subscribers_count(&self) -> usize {
        self.publishers_subscribers.keys_count()
    }

    pub fn reader_configs_count(&self) -> usize {
        self.reader_configs.len()
    }

    pub fn writer_configs_count(&self) -> usize {
        self.writer_configs.len()
    }

    pub fn agent_id(&self, session_id: SessionId) -> Option<&AgentId> {
        self.agents.get_key(&session_id)
    }

    pub fn insert_service_session(&mut self, session: Session) {
        self.sessions.insert(***session, session);
    }

    pub fn insert_new(&mut self, session: Session) {
        let session_id = ***session;
        info!("Inserting session"; {"handle_id": session_id});
        self.unused_sessions.insert(
            session_id,
            UnusedSession {
                created_at: Instant::now(),
                session,
            },
        );
    }

    pub fn disconnect(&self, id: SessionId) -> Result<()> {
        info!("Disconnecting session asynchronously"; {"handle_id": id});

        let session = self.session(id)?;

        janus_callbacks::end_session(session);
        Ok(())
    }

    pub fn handle_disconnect(&mut self, id: SessionId) -> Result<()> {
        info!(
            "Session is about to disconnect. Removing it from the switchboard.";
            {"handle_id": id}
        );

        for subscriber in self.subscribers_to(id).to_owned() {
            self.disconnect(subscriber)?;
        }

        let stream_ids: Vec<StreamId> = self
            .publishers
            .iter()
            .filter(|(_, session_id)| **session_id == id)
            .map(|(stream_id, _)| stream_id.to_owned())
            .collect();

        for stream_id in stream_ids {
            self.remove_stream(stream_id)?;
        }
        self.unused_sessions.remove(&id);
        self.sessions.remove(&id);
        self.states.remove(&id);
        let agent = self.agents.remove_value(&id);
        if let Some(agent) = agent {
            self.reader_configs.remove(&agent);
        }
        self.publishers_subscribers.remove_value(&id);
        Ok(())
    }

    pub fn session(&self, id: SessionId) -> Result<&Session> {
        self.sessions
            .get(&id)
            .ok_or_else(|| format_err!("Session not found for id = {}", id))
    }

    pub fn state(&self, id: SessionId) -> Result<&SessionState> {
        self.states
            .get(&id)
            .ok_or_else(|| format_err!("Session state not found for id = {}", id))
    }

    pub fn state_mut(&mut self, id: SessionId) -> Result<&mut SessionState> {
        self.states
            .get_mut(&id)
            .ok_or_else(|| format_err!("Session state not found for id = {}", id))
    }

    #[allow(clippy::ptr_arg)]
    pub fn agent_sessions(&self, id: &AgentId) -> &[SessionId] {
        self.agents.get_values(id)
    }

    pub fn subscribers_to(&self, publisher: SessionId) -> &[SessionId] {
        self.publishers_subscribers.get_values(&publisher)
    }

    pub fn publisher_to(&self, subscriber: SessionId) -> Option<SessionId> {
        self.publishers_subscribers
            .get_key(&subscriber)
            .map(|id| id.to_owned())
    }

    pub fn publisher_of(&self, stream_id: StreamId) -> Option<SessionId> {
        self.publishers.get(&stream_id).map(|p| p.to_owned())
    }

    pub fn stream_id_to(&self, session_id: SessionId) -> Option<StreamId> {
        self.publishers_subscribers
            .get_values(&session_id)
            .first()
            .and_then(|publisher| self.published_by(*publisher))
            .or_else(|| self.published_by(session_id))
    }

    pub fn published_by(&self, session_id: SessionId) -> Option<StreamId> {
        self.publishers.iter().find_map(|(stream_id, publisher)| {
            if *publisher == session_id {
                Some(*stream_id)
            } else {
                None
            }
        })
    }

    pub fn reader_config(
        &self,
        stream_id: StreamId,
        reader_id: &SessionId,
    ) -> Option<&ReaderConfig> {
        let agent_id = self.agents.get_key(reader_id)?;
        self.reader_configs.get(agent_id)?.get(&stream_id)
    }

    #[allow(clippy::ptr_arg)]
    pub fn update_reader_config(
        &mut self,
        stream_id: StreamId,
        reader_id: &AgentId,
        config: ReaderConfig,
    ) -> Result<()> {
        if !self.agents.contains_key(reader_id) {
            return Err(anyhow!("Agent {} not registered", reader_id));
        }

        self.reader_configs
            .entry(reader_id.to_owned())
            .or_default()
            .insert(stream_id, config);
        Ok(())
    }

    pub fn writer_config(&self, stream_id: StreamId) -> &WriterConfig {
        self.writer_configs
            .get(&stream_id)
            .unwrap_or(&DEFAULT_WRITER_CONFIG)
    }

    pub fn set_writer_config(
        &mut self,
        stream_id: StreamId,
        writer_config: WriterConfig,
    ) -> Option<WriterConfig> {
        info!("SET WRITER CONFIG: {:?}", writer_config; {"rtc_id": stream_id});
        self.writer_configs.insert(stream_id, writer_config)
    }

    pub fn create_stream(
        &mut self,
        id: StreamId,
        publisher: SessionId,
        agent_id: AgentId,
    ) -> Result<()> {
        info!("Creating stream"; {"rtc_id": id, "handle_id": publisher, "agent_id": agent_id});
        let session = self.unused_sessions.remove(&publisher).ok_or_else(|| {
            anyhow!(
                "Publisher's session id: {} not present in the new_sessions set",
                publisher
            )
        })?;
        self.sessions.insert(publisher, session.session);
        self.states.insert(publisher, SessionState::new());
        let maybe_old_publisher = self.publishers.remove(&id);
        self.publishers.insert(id, publisher);

        if let Some(old_publisher) = maybe_old_publisher {
            if let Some(subscribers) = self.publishers_subscribers.remove_key(&old_publisher) {
                for subscriber in subscribers {
                    self.publishers_subscribers.associate(publisher, subscriber);
                }
            }
        }

        self.agents.associate(agent_id, publisher);
        Ok(())
    }

    pub fn join_stream(
        &mut self,
        id: StreamId,
        subscriber: SessionId,
        agent_id: AgentId,
    ) -> Result<()> {
        let session = self.unused_sessions.remove(&subscriber).ok_or_else(|| {
            anyhow!(
                "Subscriber's session id: {} not present in the new_sessions set",
                subscriber
            )
        })?;
        self.sessions.insert(subscriber, session.session);
        self.states.insert(subscriber, SessionState::new());

        let maybe_publisher = self.publishers.get(&id).map(|p| p.to_owned());

        match maybe_publisher {
            None => bail!("Stream {} does not exist", id),
            Some(publisher) => {
                verb!(
                    "Joining to stream";
                    {"rtc_id": id, "handle_id": subscriber, "agent_id": agent_id}
                );

                self.publishers_subscribers.associate(publisher, subscriber);
                self.agents.associate(agent_id, subscriber);
                Ok(())
            }
        }
    }

    pub fn remove_stream(&mut self, id: StreamId) -> Result<()> {
        info!("Removing stream"; {"rtc_id": id});
        let maybe_publisher = self.publishers.get(&id).map(|p| p.to_owned());

        if let Some(publisher) = maybe_publisher {
            self.stop_recording(publisher)?;
            self.publishers.remove(&id);
            self.writer_configs.remove(&id);
            self.publishers_subscribers.remove_key(&publisher);
            self.agents.remove_value(&publisher);
        }

        Ok(())
    }

    fn stop_recording(&mut self, publisher: SessionId) -> Result<()> {
        let state = self.state_mut(publisher)?;

        if let Some(recorder) = state.recorder_mut() {
            info!("Stopping recording"; {"handle_id": publisher});

            recorder
                .stop_recording()
                .map_err(|err| format_err!("Failed to stop recording {}: {}", publisher, err))?;
        }

        state.unset_recorder();
        Ok(())
    }

    pub fn vacuum_sessions(&self, ttl: Duration) -> Result<()> {
        for (_, session) in self.unused_sessions.iter() {
            if session.is_timeouted(ttl) {
                janus_callbacks::end_session(&session.session);
            }
        }
        Ok(())
    }

    pub fn vacuum_publishers(&self, timeout: &chrono::Duration) -> Result<()> {
        for (stream_id, publisher) in self.publishers.iter() {
            match self.vacuum_publisher(*publisher, timeout) {
                Ok(false) => (),
                Ok(true) => warn!(
                    "Publisher timed out; No RTP packets from PeerConnection in {} seconds",
                    timeout.num_seconds();
                    {"rtc_id": stream_id, "handle_id": publisher}
                ),
                Err(err) => err!(
                    "Failed to vacuum publisher: {}", err;
                    {"rtc_id": stream_id, "handle_id": publisher}
                ),
            }
        }

        Ok(())
    }

    fn vacuum_publisher(&self, publisher: SessionId, timeout: &chrono::Duration) -> Result<bool> {
        let state = self.state(publisher)?;

        let is_timed_out = match state.since_last_rtp_packet_timestamp() {
            None => false,
            Some(duration) => duration >= *timeout,
        };

        if is_timed_out {
            self.disconnect(publisher)?;
        }

        Ok(is_timed_out)
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct LockedSwitchboard(RwLock<Switchboard>);

impl LockedSwitchboard {
    pub fn new() -> Self {
        Self(RwLock::new(Switchboard::new()))
    }

    pub fn with_read_lock<F, R>(&self, callback: F) -> Result<R>
    where
        F: FnOnce(RwLockReadGuard<Switchboard>) -> Result<R>,
    {
        match self.0.read() {
            Ok(switchboard) => callback(switchboard),
            Err(_) => bail!("Failed to acquire switchboard read lock"),
        }
    }

    pub fn with_write_lock<F, R>(&self, callback: F) -> Result<R>
    where
        F: FnOnce(RwLockWriteGuard<Switchboard>) -> Result<R>,
    {
        match self.0.write() {
            Ok(switchboard) => callback(switchboard),
            Err(_) => bail!("Failed to acquire switchboard write lock"),
        }
    }

    pub fn vacuum_publishers_loop(&self, interval: Duration, sessions_ttl: Duration) -> Result<()> {
        info!("Vacuum thread spawned");
        loop {
            self.with_read_lock(|switchboard| {
                switchboard.vacuum_publishers(&chrono::Duration::from_std(interval)?)?;
                switchboard.vacuum_sessions(sessions_ttl)?;
                Ok(())
            })
            .unwrap_or_else(|err| err!("{}", err));

            thread::sleep(interval);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use crate::{conf::SpeakingNotifications, janus_rtp::AudioLevel};

    use super::SessionState;

    #[test]
    fn test_speaking_notification() {
        let state = SessionState::new();
        // none when not enought packets
        assert_eq!(
            state.is_speaking(
                AudioLevel::from_u8(10),
                &SpeakingNotifications {
                    audio_active_packets: 5,
                    speaking_average_level: AudioLevel::from_u8(10),
                    not_speaking_average_level: AudioLevel::from_u8(10),
                }
            ),
            None
        );
        // none when not enought packets
        assert_eq!(
            state.is_speaking(
                AudioLevel::from_u8(10),
                &SpeakingNotifications {
                    audio_active_packets: 3,
                    speaking_average_level: AudioLevel::from_u8(10),
                    not_speaking_average_level: AudioLevel::from_u8(10),
                }
            ),
            None
        );
        // none when state didn't change
        assert_eq!(
            state.is_speaking(
                AudioLevel::from_u8(10),
                &SpeakingNotifications {
                    audio_active_packets: 3,
                    speaking_average_level: AudioLevel::from_u8(5),
                    not_speaking_average_level: AudioLevel::from_u8(5),
                }
            ),
            None
        );
        assert_eq!(state.packets_count.load(Ordering::Relaxed), 0);
        // none when not enought packets
        assert_eq!(
            state.is_speaking(
                AudioLevel::from_u8(10),
                &SpeakingNotifications {
                    audio_active_packets: 2,
                    speaking_average_level: AudioLevel::from_u8(10),
                    not_speaking_average_level: AudioLevel::from_u8(10),
                }
            ),
            None
        );
        // true when state changed
        assert_eq!(
            state.is_speaking(
                AudioLevel::from_u8(10),
                &SpeakingNotifications {
                    audio_active_packets: 2,
                    speaking_average_level: AudioLevel::from_u8(15),
                    not_speaking_average_level: AudioLevel::from_u8(15),
                }
            ),
            Some(true)
        );
        assert_eq!(state.packets_count.load(Ordering::Relaxed), 0);
        // none when not enough packets
        assert_eq!(
            state.is_speaking(
                AudioLevel::from_u8(10),
                &SpeakingNotifications {
                    audio_active_packets: 2,
                    speaking_average_level: AudioLevel::from_u8(5),
                    not_speaking_average_level: AudioLevel::from_u8(5),
                }
            ),
            None
        );
        //none when state didn't change
        assert_eq!(
            state.is_speaking(
                AudioLevel::from_u8(10),
                &SpeakingNotifications {
                    audio_active_packets: 2,
                    speaking_average_level: AudioLevel::from_u8(15),
                    not_speaking_average_level: AudioLevel::from_u8(15),
                }
            ),
            None
        );
    }
}
