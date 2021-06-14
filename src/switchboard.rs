use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;

use anyhow::{bail, format_err, Context, Result};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use janus::session::SessionWrapper;
use uuid::Uuid;

use crate::bidirectional_multimap::BidirectionalMultimap;
use crate::janus_callbacks;
use crate::recorder::RecorderHandle;

///////////////////////////////////////////////////////////////////////////////

pub type StreamId = Uuid;
pub type AgentId = String;
pub type Session = Box<Arc<SessionWrapper<SessionId>>>;
pub type LockedSession = Arc<Mutex<Session>>;

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
    fir_seq: AtomicI32,
    initial_rembs_counter: AtomicU64,
    last_remb_timestamp: AtomicI64,
    last_rtp_packet_timestamp: AtomicI64,
    recorder: Option<RecorderHandle>,
}

impl SessionState {
    fn new() -> Self {
        Self {
            fir_seq: AtomicI32::new(0),
            initial_rembs_counter: AtomicU64::new(0),
            last_remb_timestamp: AtomicI64::new(0),
            last_rtp_packet_timestamp: AtomicI64::new(0),
            recorder: None,
        }
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

    pub fn touch_last_remb_timestamp(&self) {
        self.last_remb_timestamp
            .store(Utc::now().timestamp(), Ordering::Relaxed);
    }

    fn since_last_rtp_packet_timestamp(&self) -> Option<Duration> {
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

lazy_static! {
    static ref DEFAULT_WRITER_CONFIG: WriterConfig = Default::default();
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct Switchboard {
    sessions: HashMap<SessionId, LockedSession>,
    states: HashMap<SessionId, SessionState>,
    agents: BidirectionalMultimap<AgentId, SessionId>,
    publishers: HashMap<StreamId, SessionId>,
    publishers_subscribers: BidirectionalMultimap<SessionId, SessionId>,
    reader_configs: HashMap<(StreamId, AgentId), ReaderConfig>,
    writer_configs: HashMap<StreamId, WriterConfig>,
}

impl Switchboard {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            states: HashMap::new(),
            agents: BidirectionalMultimap::new(),
            publishers: HashMap::new(),
            publishers_subscribers: BidirectionalMultimap::new(),
            reader_configs: HashMap::new(),
            writer_configs: HashMap::new(),
        }
    }

    pub fn connect(&mut self, session: Session) -> Result<()> {
        let session_id = ***session;
        info!("Connecting session"; {"handle_id": session_id});
        let locked_session = Arc::new(Mutex::new(session));
        self.sessions.insert(session_id, locked_session);
        self.states.insert(session_id, SessionState::new());
        Ok(())
    }

    pub fn disconnect(&mut self, id: SessionId) -> Result<()> {
        info!("Disconnecting session asynchronously"; {"handle_id": id});

        let session = self
            .session(id)?
            .lock()
            .map_err(|err| format_err!("Failed to acquire session mutex {}: {}", id, err))?;

        janus_callbacks::end_session(&session);
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

        self.sessions.remove(&id);
        self.states.remove(&id);
        self.agents.remove_value(&id);
        self.publishers_subscribers.remove_value(&id);
        Ok(())
    }

    pub fn session(&self, id: SessionId) -> Result<&LockedSession> {
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
        self.agents
            .get_key(reader_id)
            .and_then(|agent_id| self.reader_configs.get(&(stream_id, agent_id.to_owned())))
    }

    pub fn update_reader_config(
        &mut self,
        stream_id: StreamId,
        reader_id: SessionId,
        config: ReaderConfig,
    ) -> Result<()> {
        let agent_id = self
            .agents
            .get_key(&reader_id)
            .ok_or_else(|| anyhow!("Agent not registered for handle {}", reader_id))?;

        self.reader_configs
            .insert((stream_id, agent_id.to_owned()), config);
        Ok(())
    }

    pub fn writer_config(&self, stream_id: StreamId) -> &WriterConfig {
        self.writer_configs
            .get(&stream_id)
            .unwrap_or(&DEFAULT_WRITER_CONFIG)
    }

    pub fn set_writer_config(&mut self, stream_id: StreamId, writer_config: WriterConfig) {
        info!("SET WRITER CONFIG: {:?}", writer_config; {"rtc_id": stream_id});
        self.writer_configs.insert(stream_id, writer_config);
    }

    pub fn create_stream(
        &mut self,
        id: StreamId,
        publisher: SessionId,
        agent_id: AgentId,
    ) -> Result<()> {
        info!("Creating stream"; {"rtc_id": id, "handle_id": publisher, "agent_id": agent_id});

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

    pub fn vacuum_publishers(&mut self, timeout: &Duration) -> Result<()> {
        for (stream_id, publisher) in self.publishers.clone().into_iter() {
            match self.vacuum_publisher(publisher, timeout) {
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

    fn vacuum_publisher(&mut self, publisher: SessionId, timeout: &Duration) -> Result<bool> {
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

    pub fn vacuum_publishers_loop(&self, interval: Duration) -> Result<()> {
        info!("Vacuum thread spawned");

        let std_interval = interval
            .to_std()
            .context("Failed to convert vacuum interval")?;

        loop {
            self.with_write_lock(|mut switchboard| switchboard.vacuum_publishers(&interval))
                .unwrap_or_else(|err| err!("{}", err));

            thread::sleep(std_interval);
        }
    }
}
