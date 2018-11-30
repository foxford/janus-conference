use std::collections::HashMap;
use std::sync::Arc;

use messages::RoomId;
use session::Session;

#[derive(Debug)]
pub struct Switchboard {
    sessions: Vec<Box<Arc<Session>>>,
    publishers: HashMap<RoomId, Arc<Session>>,
    subscriptions: HashMap<Arc<Session>, Vec<Arc<Session>>>,
}

impl Switchboard {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            publishers: HashMap::new(),
            subscriptions: HashMap::new(),
        }
    }

    pub fn connect(&mut self, session: Box<Arc<Session>>) {
        self.sessions.push(session);
    }

    pub fn disconnect(&mut self, sess: &Session) {
        self.sessions.retain(|s| s.handle != sess.handle);
    }

    pub fn subscribers_for(&self, publisher: &Session) -> impl Iterator<Item = &Arc<Session>> {
        match self.subscriptions.get(publisher) {
            Some(subscribers) => subscribers.iter(),
            None => [].iter(),
        }
    }
}
