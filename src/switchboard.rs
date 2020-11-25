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
use crate::recorder::Recorder;

///////////////////////////////////////////////////////////////////////////////

pub type StreamId = Uuid;
pub type AgentId = String;
pub type Session = Box<Arc<SessionWrapper<SessionId>>>;
pub type LockedSession = Arc<Mutex<Session>>;

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
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
    recorder: Option<Recorder>,
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

    pub fn recorder(&self) -> Option<&Recorder> {
        self.recorder.as_ref()
    }

    pub fn recorder_mut(&mut self) -> Option<&mut Recorder> {
        self.recorder.as_mut()
    }

    pub fn set_recorder(&mut self, recorder: Recorder) -> &mut Self {
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
pub struct Switchboard {
    sessions: HashMap<SessionId, LockedSession>,
    states: HashMap<SessionId, SessionState>,
    agents: BidirectionalMultimap<AgentId, SessionId>,
    writers: BidirectionalMultimap<SessionId, StreamId>,
    readers: BidirectionalMultimap<SessionId, StreamId>,
}

impl Switchboard {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            states: HashMap::new(),
            agents: BidirectionalMultimap::new(),
            writers: BidirectionalMultimap::new(),
            readers: BidirectionalMultimap::new(),
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

    pub fn disconnect(&mut self, session_id: SessionId) -> Result<()> {
        info!("Disconnecting session asynchronously"; {"handle_id": session_id});

        let session = self.session(session_id)?.lock().map_err(|err| {
            format_err!("Failed to acquire session mutex {}: {}", session_id, err)
        })?;

        janus_callbacks::end_session(&session);
        Ok(())
    }

    pub fn handle_disconnect(&mut self, session_id: SessionId) -> Result<()> {
        info!(
            "Session is about to disconnect. Removing it from the switchboard.";
            {"handle_id": session_id}
        );

        self.sessions.remove(&session_id);
        self.states.remove(&session_id);
        self.agents.remove_value(&session_id);
        self.writers.remove_key(&session_id);
        self.readers.remove_key(&session_id);
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

    pub fn writer_to(&self, reader: SessionId) -> Option<SessionId> {
        self.readers
            .get_values(&reader)
            .first()
            .and_then(|stream_id| self.writer_of(*stream_id))
    }

    pub fn writer_of(&self, stream_id: StreamId) -> Option<SessionId> {
        self.writers
            .get_keys(&stream_id)
            .first()
            .map(|id| id.to_owned())
    }

    pub fn readers_to(&self, writer: SessionId) -> &[SessionId] {
        if let Some(stream_id) = self.writers.get_values(&writer).first() {
            self.readers_of(*stream_id)
        } else {
            &[]
        }
    }

    pub fn readers_of(&self, stream_id: StreamId) -> &[SessionId] {
        self.readers.get_keys(&stream_id)
    }

    pub fn associate_agent(&mut self, session_id: SessionId, agent_id: &AgentId) -> Result<()> {
        verb!("Associating agent with the handle"; {"handle_id": session_id, "agent_id": agent_id});
        self.agents.associate(agent_id.to_owned(), session_id);
        Ok(())
    }

    pub fn create_stream(
        &mut self,
        stream_id: StreamId,
        writer: SessionId,
    ) -> Result<()> {
        info!("Creating stream"; {"rtc_id": stream_id, "handle_id": writer});
        self.writers.remove_value(&stream_id);
        self.writers.associate(writer, stream_id);
        Ok(())
    }

    pub fn join_stream(
        &mut self,
        stream_id: StreamId,
        reader: SessionId,
    ) -> Result<()> {
        verb!("Joining to stream"; {"rtc_id": stream_id, "handle_id": reader});
        self.readers.associate(reader, stream_id);
        Ok(())
    }

    pub fn remove_stream(&mut self, stream_id: StreamId) -> Result<()> {
        info!("Removing stream"; {"rtc_id": stream_id});

        if let Some(writer) = self.writers.get_keys(&stream_id).to_owned().first() {
            self.stop_recording(*writer)?;
            self.agents.remove_value(&writer);
        }

        self.writers.remove_value(&stream_id);
        self.readers.remove_value(&stream_id);
        Ok(())
    }

    fn stop_recording(&mut self, writer: SessionId) -> Result<()> {
        let state = self.state_mut(writer)?;

        if let Some(recorder) = state.recorder_mut() {
            info!("Stopping recording"; {"handle_id": writer});

            recorder
                .stop_recording()
                .map_err(|err| format_err!("Failed to stop recording {}: {}", writer, err))?;
        }

        state.unset_recorder();
        Ok(())
    }

    pub fn vacuum_writers(&mut self, timeout: &Duration) -> Result<()> {
        let writers = self
            .writers
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect::<Vec<(SessionId, StreamId)>>();

        for (writer, stream_id) in writers {
            match self.vacuum_writer(writer, timeout) {
                Ok(false) => (),
                Ok(true) => warn!(
                    "Writer timed out; No RTP packets from PeerConnection in {} seconds",
                    timeout.num_seconds();
                    {"rtc_id": stream_id, "handle_id": writer}
                ),
                Err(err) => err!(
                    "Failed to vacuum writer: {}", err;
                    {"rtc_id": stream_id, "handle_id": writer}
                ),
            }
        }

        Ok(())
    }

    fn vacuum_writer(&mut self, writer: SessionId, timeout: &Duration) -> Result<bool> {
        let state = self.state(writer)?;

        let is_timed_out = match state.since_last_rtp_packet_timestamp() {
            None => false,
            Some(duration) => duration >= *timeout,
        };

        if is_timed_out {
            self.disconnect(writer)?;
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

    pub fn vacuum_writers_loop(&self, interval: Duration) -> Result<()> {
        verb!("Vacuum thread spawned");

        let std_interval = interval
            .to_std()
            .context("Failed to convert vacuum interval")?;

        loop {
            self.with_write_lock(|mut switchboard| switchboard.vacuum_writers(&interval))
                .unwrap_or_else(|err| err!("{}", err));

            thread::sleep(std_interval);
        }
    }
}
