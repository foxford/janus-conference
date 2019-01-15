use std::collections::HashMap;
use std::sync::Arc;

use failure::{err_msg, Error};

use bidirectional_multimap::BidirectionalMultimap;
use messages::RoomId;
use session::Session;

#[derive(Debug)]
pub struct Switchboard {
    sessions: Vec<Box<Arc<Session>>>,
    publishers: HashMap<RoomId, Arc<Session>>,
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

    pub fn create_room(&mut self, room_id: RoomId, publisher: Arc<Session>) {
        let old_publisher = self.publishers.remove(&room_id);
        self.publishers.insert(room_id, publisher.clone());

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

    pub fn join_room(&mut self, room_id: &RoomId, subscriber: Arc<Session>) -> Result<(), Error> {
        match self.publishers.get(room_id) {
            Some(publisher) => self
                .publishers_subscribers
                .associate(publisher.clone(), subscriber),
            None => {
                return Err(err_msg("Failed to join non-existent room"));
            }
        }

        Ok(())
    }
}
