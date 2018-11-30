use std::sync::Mutex;

use janus::session::SessionWrapper;

#[derive(Debug)]
pub struct SessionState {
    pub destroyed: Mutex<bool>,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            destroyed: Mutex::new(false),
        }
    }
}

pub type Session = SessionWrapper<SessionState>;
