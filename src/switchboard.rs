use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use failure::Error;

use crate::janus_callbacks;
use bidirectional_multimap::BidirectionalMultimap;
use recorder::Recorder;
use session::Session;

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
        janus_info!("[CONFERENCE] Disconnecting session {}.", **session);

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
        Ok(())
    }

    pub fn subscribers_to(&self, publisher: &Session) -> &[Arc<Session>] {
        self.publishers_subscribers.get_values(publisher)
    }

    pub fn publisher_to(&self, subscriber: &Session) -> Option<&Arc<Session>> {
        self.publishers_subscribers.get_key(subscriber)
    }

    pub fn attach_recorder(&mut self, publisher: Arc<Session>, recorder: Recorder) {
        janus_info!("[CONFERENCE] Attaching recorder for {}", **publisher);
        self.recorders.insert(publisher, recorder);
    }

    pub fn recorder_for(&self, publisher: &Session) -> Option<&Recorder> {
        self.recorders.get(publisher)
    }

    pub fn create_stream(&mut self, id: &str, publisher: Arc<Session>) {
        janus_info!(
            "[CONFERENCE] Creating stream {}. Publisher: {}",
            id,
            **publisher
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
                    "[CONFERENCE] Joining to stream {}. Subscriber: {}",
                    id,
                    **subscriber
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

    pub fn vacuum_publishers(&self, timeout: &Duration) {
        for (stream_id, publisher) in self.publishers.iter() {
            match Self::vacuum_publisher(&publisher, timeout) {
                Ok(false) => (),
                Ok(true) => {
                    janus_info!(
                        "[CONFERENCE] Publisher {:p} timed out on stream {}; PeerConnection closed",
                        publisher.handle,
                        stream_id
                    );
                }
                Err(err) => {
                    janus_err!(
                        "[CONFERENCE] Failed to vacuum publisher {:p} on stream {}: {}",
                        publisher.handle,
                        stream_id,
                        err
                    );
                }
            }
        }
    }

    fn vacuum_publisher(publisher: &Session, timeout: &Duration) -> Result<bool, Error> {
        let last_rtp_packet_timestamp = match publisher.last_rtp_packet_timestamp()? {
            Some(timestamp) => timestamp,
            None => return Ok(false),
        };

        let duration = SystemTime::now()
            .duration_since(last_rtp_packet_timestamp)
            .map_err(|err| format_err!("{}", err))?;

        if duration >= *timeout {
            janus_callbacks::close_pc(&publisher);
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
