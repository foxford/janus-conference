use std::collections::HashMap;
use std::sync::Arc;

use failure::Error;

use bidirectional_multimap::BidirectionalMultimap;
use messages::StreamId;
use session::Session;

#[derive(Debug)]
pub struct Switchboard {
    sessions: Vec<Box<Arc<Session>>>,
    publishers: HashMap<StreamId, Arc<Session>>,
    publishers_subscribers: BidirectionalMultimap<Arc<Session>, Arc<Session>>,
}

impl Switchboard {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            publishers: HashMap::new(),
            publishers_subscribers: BidirectionalMultimap::new(),
        }
    }

    pub fn connect(&mut self, session: Box<Arc<Session>>) {
        self.sessions.push(session);
    }

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

    pub fn publisher_by_stream(&self, id: &StreamId) -> Option<&Arc<Session>> {
        self.publishers.get(id)
    }

    pub fn create_stream(&mut self, id: StreamId, publisher: Arc<Session>) {
        let old_publisher = self.publishers.remove(&id);
        self.publishers.insert(id, publisher.clone());

        match old_publisher {
            Some(old_publisher) => {
                let maybe_subscribers = self.publishers_subscribers.remove_key(&old_publisher);

                if let Some(subscribers) = maybe_subscribers {
                    for subscriber in subscribers {
                        self.publishers_subscribers
                            .associate(publisher.clone(), subscriber.clone());
                    }
                }
            }
            None => {}
        }
    }

    pub fn join_stream(&mut self, id: &StreamId, subscriber: Arc<Session>) -> Result<(), Error> {
        match self.publishers.get(id) {
            Some(publisher) => self
                .publishers_subscribers
                .associate(publisher.clone(), subscriber),
            None => {
                return Err(format_err!("Stream with Id = {} does not exist", id));
            }
        }

        Ok(())
    }
}
