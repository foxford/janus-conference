use std::sync::Mutex;

use janus::session::SessionWrapper;

#[derive(Debug)]
pub struct SessionState {
    pub destroyed: Mutex<bool>,
}

pub type Session = SessionWrapper<SessionState>;
