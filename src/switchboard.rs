use std::collections::HashMap;
use std::sync::Arc;

use failure::Error;

use crate::ConcreteRecorder as Recorder;
use bidirectional_multimap::BidirectionalMultimap;
use messages::StreamId;
use session::Session;

#[derive(Debug)]
pub struct Switchboard {
    sessions: Vec<Box<Arc<Session>>>,
    publishers: HashMap<StreamId, Arc<Session>>,
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

    // We don't remove stream/publisher and publisher/recorder relations
    // since they can be useful even when this publisher is not active anymore.
    pub fn disconnect(&mut self, sess: &Session) {
        self.sessions.retain(|s| s.handle != sess.handle);
        self.publishers_subscribers.remove_key(sess);
        self.publishers_subscribers.remove_value(sess);
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

    pub fn recorder_for_mut(&mut self, publisher: &Session) -> Option<&mut Recorder> {
        self.recorders.get_mut(publisher)
    }

    // TODO: &StreamId -> &str
    pub fn publisher_by_stream(&self, id: &StreamId) -> Option<&Arc<Session>> {
        self.publishers.get(id)
    }

    // TODO: StreamId -> &str
    pub fn create_stream(&mut self, id: StreamId, publisher: Arc<Session>) {
        janus_info!(
            "[CONFERENCE] Creating stream {}. Publisher: {}",
            id,
            **publisher
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
    }

    // TODO: &StreamId -> &str
    pub fn join_stream(&mut self, id: &StreamId, subscriber: Arc<Session>) -> Result<(), Error> {
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

    // TODO: StreamId -> &str
    pub fn remove_stream(&mut self, id: StreamId) {
        janus_info!("[CONFERENCE] Removing stream {}", id);

        if let Some(publisher) = self.publishers.remove(&id) {
            self.publishers_subscribers.remove_key(&publisher);
        }
    }
}
