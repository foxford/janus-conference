use std::sync::atomic::AtomicIsize;
use std::sync::Mutex;

use janus::session::SessionWrapper;

#[derive(Debug)]
pub struct SessionState {
    pub destroyed: Mutex<bool>,
    pub fir_seq: AtomicIsize,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            destroyed: Mutex::new(false),
            fir_seq: AtomicIsize::new(0),
        }
    }
}

pub type Session = SessionWrapper<SessionState>;
