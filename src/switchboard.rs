use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use failure::{err_msg, Error};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
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
    last_rtp_packet_timestamp: AtomicU64,
    recorder: Option<Recorder>,
}

impl SessionState {
    fn new() -> Self {
        Self {
            fir_seq: AtomicI32::new(0),
            last_rtp_packet_timestamp: AtomicU64::new(0),
            recorder: None,
        }
    }

    pub fn increment_fir_seq(&self) -> i32 {
        self.fir_seq.fetch_add(1, Ordering::Relaxed)
    }

    fn since_last_rtp_packet_timestamp(&self) -> Result<Option<Duration>, Error> {
        match self.last_rtp_packet_timestamp.load(Ordering::Relaxed) {
            0 => Ok(None),
            secs => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|err| {
                        format_err!("Failed to set last RTP packet timestamp: {}", err)
                    })?;

                Ok(Some(now - Duration::from_secs(secs)))
            }
        }
    }

    pub fn touch_last_rtp_packet_timestamp(&self) -> Result<(), Error> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| format_err!("Failed to set last RTP packet timestamp: {}", err))?
            .as_secs();

        self.last_rtp_packet_timestamp.store(now, Ordering::Relaxed);
        Ok(())
    }

    pub fn recorder(&self) -> Option<&Recorder> {
        self.recorder.as_ref()
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
    publishers: HashMap<StreamId, SessionId>,
    publishers_subscribers: BidirectionalMultimap<SessionId, SessionId>,
}

impl Switchboard {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            states: HashMap::new(),
            agents: BidirectionalMultimap::new(),
            publishers: HashMap::new(),
            publishers_subscribers: BidirectionalMultimap::new(),
        }
    }

    pub fn connect(&mut self, session: Session) -> Result<(), Error> {
        let session_id = ***session;
        janus_verb!("[CONFERENCE] Connecting session {}", session_id);
        let locked_session = Arc::new(Mutex::new(session));
        self.sessions.insert(session_id, locked_session);
        self.states.insert(session_id, SessionState::new());
        Ok(())
    }

    pub fn disconnect(&mut self, id: SessionId) -> Result<(), Error> {
        janus_verb!("[CONFERENCE] Disconnecting session {}", id);

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

    pub fn session(&self, id: SessionId) -> Result<&LockedSession, Error> {
        self.sessions
            .get(&id)
            .ok_or_else(|| format_err!("Session not found for id = {}", id))
    }

    pub fn state(&self, id: SessionId) -> Result<&SessionState, Error> {
        self.states
            .get(&id)
            .ok_or_else(|| format_err!("Session state not found for id = {}", id))
    }

    pub fn state_mut(&mut self, id: SessionId) -> Result<&mut SessionState, Error> {
        self.states
            .get_mut(&id)
            .ok_or_else(|| format_err!("Session state not found for id = {}", id))
    }

    pub fn agent(&self, id: AgentId) -> Result<SessionId, Error> {
        self.agents
            .get_values(&id)
            .first()
            .map(|id| id.to_owned())
            .ok_or_else(|| format_err!("Agent not found for id = {}", id))
    }

    pub fn subscribers_to(&self, publisher: SessionId) -> &[SessionId] {
        self.publishers_subscribers.get_values(&publisher)
    }

    pub fn publisher_to(&self, subscriber: SessionId) -> Option<SessionId> {
        self.publishers_subscribers
            .get_key(&subscriber)
            .map(|id| id.to_owned())
    }

    pub fn create_stream(
        &mut self,
        id: StreamId,
        publisher: SessionId,
        agent_id: AgentId,
    ) -> Result<(), Error> {
        janus_verb!(
            "[CONFERENCE] Creating stream {}. Publisher: {}",
            id,
            publisher
        );

        let maybe_old_publisher = self.publishers.remove(&id);
        self.publishers.insert(id, publisher.clone());

        if let Some(old_publisher) = maybe_old_publisher {
            if let Some(subscribers) = self.publishers_subscribers.remove_key(&old_publisher) {
                for subscriber in subscribers {
                    self.publishers_subscribers
                        .associate(publisher.clone(), subscriber.clone());
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
    ) -> Result<(), Error> {
        let maybe_publisher = self.publishers.get(&id).map(|p| p.to_owned());

        match maybe_publisher {
            None => bail!("Stream {} does not exist", id),
            Some(publisher) => {
                janus_verb!(
                    "[CONFERENCE] Joining to stream {}. Subscriber: {}",
                    id,
                    subscriber
                );

                self.publishers_subscribers.associate(publisher, subscriber);
                self.agents.associate(agent_id, subscriber);
                Ok(())
            }
        }
    }

    pub fn remove_stream(&mut self, id: StreamId) -> Result<(), Error> {
        janus_verb!("[CONFERENCE] Removing stream {}", id);
        let maybe_publisher = self.publishers.get(&id).map(|p| p.to_owned());

        if let Some(publisher) = maybe_publisher {
            self.stop_recording(publisher)?;
            self.publishers.remove(&id);
            self.publishers_subscribers.remove_key(&publisher);
            self.agents.remove_value(&publisher);
        }

        Ok(())
    }

    fn stop_recording(&mut self, publisher: SessionId) -> Result<(), Error> {
        let state = self.state_mut(publisher)?;

        if let Some(recorder) = state.recorder() {
            janus_verb!("[CONFERENCE] Stopping recording {}", publisher);

            recorder
                .stop_recording()
                .map_err(|err| format_err!("Failed to stop recording {}: {}", publisher, err))?;
        }

        state.unset_recorder();
        Ok(())
    }

    pub fn vacuum_publishers(&mut self, timeout: &Duration) -> Result<(), Error> {
        for (stream_id, publisher) in self.publishers.clone().into_iter() {
            match self.vacuum_publisher(publisher, timeout) {
                Ok(false) => (),
                Ok(true) => janus_warn!(
                    "[CONFERENCE] Publisher {} timed out on stream {}; No RTP packets from PeerConnection in {} seconds",
                    publisher,
                    stream_id,
                    timeout.as_secs()
                ),
                Err(err) => janus_err!(
                    "[CONFERENCE] Failed to vacuum publisher {} on stream {}: {}",
                    publisher,
                    stream_id,
                    err
                ),
            }
        }

        Ok(())
    }

    fn vacuum_publisher(
        &mut self,
        publisher: SessionId,
        timeout: &Duration,
    ) -> Result<bool, Error> {
        let state = self.state(publisher)?;

        let is_timed_out = match state.since_last_rtp_packet_timestamp() {
            Ok(None) => false,
            Ok(Some(duration)) => duration >= *timeout,
            Err(err) => bail!("Failed to vacuum publisher {}: {}", publisher, err),
        };

        if is_timed_out {
            let session = self.session(publisher)?.lock().map_err(|err| {
                format_err!("Failed to acquire session mutex {}: {}", publisher, err)
            })?;

            janus_callbacks::end_session(&session);
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

    pub fn with_read_lock<F, R>(&self, callback: F) -> Result<R, Error>
    where
        F: FnOnce(RwLockReadGuard<Switchboard>) -> Result<R, Error>,
    {
        match self.0.read() {
            Ok(switchboard) => callback(switchboard),
            Err(_) => Err(err_msg("Failed to acquire switchboard read lock")),
        }
    }

    pub fn with_write_lock<F, R>(&self, callback: F) -> Result<R, Error>
    where
        F: FnOnce(RwLockWriteGuard<Switchboard>) -> Result<R, Error>,
    {
        match self.0.write() {
            Ok(switchboard) => callback(switchboard),
            Err(_) => Err(err_msg("Failed to acquire switchboard write lock")),
        }
    }

    pub fn vacuum_publishers_loop(&self, interval: Duration) {
        janus_info!("[CONFERENCE] Vacuum thread is alive.");

        loop {
            self.with_write_lock(|mut switchboard| switchboard.vacuum_publishers(&interval))
                .unwrap_or_else(|err| janus_err!("[CONFERENCE] {}", err));

            thread::sleep(interval);
        }
    }
}
