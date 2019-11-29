use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::time::{Duration, SystemTime};

use failure::{err_msg, Error};

use crate::bidirectional_multimap::BidirectionalMultimap;
use crate::janus_callbacks;
use crate::recorder::Recorder;
use crate::session::Session;

#[derive(Debug)]
pub struct Switchboard {
    sessions: Vec<Box<Arc<Session>>>,
    publishers: HashMap<String, Arc<Session>>,
    publishers_subscribers: BidirectionalMultimap<Arc<Session>, Arc<Session>>,
    recorders: HashMap<Arc<Session>, Recorder>,
}

impl Switchboard {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            publishers: HashMap::new(),
            publishers_subscribers: BidirectionalMultimap::new(),
            recorders: HashMap::new(),
        }
    }

    pub fn connect(&mut self, session: Box<Arc<Session>>) {
        self.sessions.push(session);
    }

    pub fn disconnect(&mut self, session: &Session) -> Result<(), Error> {
        janus_info!("[CONFERENCE] Disconnecting session {:p}.", session.handle);

        let ids: Vec<String> = self
            .publishers
            .iter()
            .filter(|(_, s)| s.handle == session.handle)
            .map(|(id, _)| id.to_string())
            .collect();

        for id in ids {
            self.remove_stream(&id)?;
        }

        self.sessions.retain(|s| s.handle != session.handle);
        self.publishers_subscribers.remove_value(session);
        janus_callbacks::end_session(session)
    }

    pub fn subscribers_to(&self, publisher: &Session) -> &[Arc<Session>] {
        self.publishers_subscribers.get_values(publisher)
    }

    pub fn publisher_to(&self, subscriber: &Session) -> Option<&Arc<Session>> {
        self.publishers_subscribers.get_key(subscriber)
    }

    pub fn attach_recorder(&mut self, publisher: Arc<Session>, recorder: Recorder) {
        janus_info!("[CONFERENCE] Attaching recorder for {:p}", publisher.handle);
        self.recorders.insert(publisher, recorder);
    }

    pub fn recorder_for(&self, publisher: &Session) -> Option<&Recorder> {
        self.recorders.get(publisher)
    }

    pub fn create_stream(&mut self, id: &str, publisher: Arc<Session>) {
        janus_info!(
            "[CONFERENCE] Creating stream {}. Publisher: {:p}",
            id,
            publisher.handle
        );

        let maybe_old_publisher = self.publishers.remove(id);
        self.publishers.insert(id.to_string(), publisher.clone());

        if let Some(old_publisher) = maybe_old_publisher {
            if let Some(subscribers) = self.publishers_subscribers.remove_key(&old_publisher) {
                for subscriber in subscribers {
                    self.publishers_subscribers
                        .associate(publisher.clone(), subscriber.clone());
                }
            }
        }
    }

    pub fn join_stream(&mut self, id: &str, subscriber: Arc<Session>) -> Result<(), Error> {
        match self.publishers.get(id) {
            Some(publisher) => {
                janus_info!(
                    "[CONFERENCE] Joining to stream {}. Subscriber: {:p}",
                    id,
                    subscriber.handle
                );

                self.publishers_subscribers
                    .associate(publisher.clone(), subscriber);

                Ok(())
            }
            None => Err(format_err!("Stream with Id = {} does not exist", id)),
        }
    }

    pub fn remove_stream(&mut self, id: &str) -> Result<(), Error> {
        janus_info!("[CONFERENCE] Removing stream {}", id);
        self.stop_recording(id.clone())?;

        match self.publishers.get_mut(id) {
            Some(publisher) => self.publishers_subscribers.remove_key(publisher),
            None => return Ok(()),
        };

        self.publishers.remove(id);
        Ok(())
    }

    pub fn stop_recording(&mut self, id: &str) -> Result<(), Error> {
        if let Some(session) = self.publishers.get(id) {
            if let Some(recorder) = self.recorders.get_mut(session) {
                janus_info!("[CONFERENCE] Stopping recording {}.", id);

                recorder
                    .stop_recording()
                    .map_err(|err| format_err!("Failed to stop recording {}: {}", id, err))?;
            }

            self.recorders.remove(session);
        }

        Ok(())
    }

    pub fn vacuum_publishers(&mut self, timeout: &Duration) -> Result<(), Error> {
        for (stream_id, publisher) in self.publishers.clone().into_iter() {
            match self.vacuum_publisher(&publisher, timeout) {
                Ok(false) => (),
                Ok(true) => janus_info!(
                    "[CONFERENCE] Publisher {:p} timed out on stream {}; PeerConnection closed",
                    publisher.handle,
                    stream_id
                ),
                Err(err) => janus_err!(
                    "[CONFERENCE] Failed to vacuum publisher {:p} on stream {}: {}",
                    publisher.handle,
                    stream_id,
                    err
                ),
            }
        }

        Ok(())
    }

    fn vacuum_publisher(&mut self, publisher: &Session, timeout: &Duration) -> Result<bool, Error> {
        let last_rtp_packet_timestamp = match publisher.last_rtp_packet_timestamp()? {
            Some(timestamp) => timestamp,
            None => return Ok(false),
        };

        let duration = SystemTime::now()
            .duration_since(last_rtp_packet_timestamp)
            .map_err(|err| format_err!("{}", err))?;

        if duration >= *timeout {
            self.disconnect(publisher)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

pub struct LockedSwitchboard(RwLock<Switchboard>);

impl LockedSwitchboard {
    pub fn new() -> Self {
        Self(RwLock::new(Switchboard::new()))
    }

    pub fn with_read_lock<F, R>(&self, callback: F) -> Result<R, Error>
    where
        F: Fn(RwLockReadGuard<Switchboard>) -> Result<R, Error>,
    {
        match self.0.read() {
            Ok(switchboard) => callback(switchboard),
            Err(_) => Err(err_msg("Failed to acquire switchboard read lock")),
        }
    }

    pub fn with_write_lock<F, R>(&self, callback: F) -> Result<R, Error>
    where
        F: Fn(RwLockWriteGuard<Switchboard>) -> Result<R, Error>,
    {
        match self.0.write() {
            Ok(switchboard) => callback(switchboard),
            Err(_) => Err(err_msg("Failed to acquire switchboard write lock")),
        }
    }

    pub fn vacuum_publishers_loop(&self, interval: Duration) {
        janus_verb!("[CONFERENCE] Vacuum thread is alive");

        loop {
            self.with_write_lock(|mut switchboard| switchboard.vacuum_publishers(&interval))
                .unwrap_or_else(|err| janus_err!("[CONFERENCE] {}", err));

            thread::sleep(interval);
        }
    }
}
