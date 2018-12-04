use std::sync::atomic::AtomicIsize;

use janus::session::SessionWrapper;

#[derive(Debug)]
pub struct SessionState {
    pub fir_seq: AtomicIsize,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            fir_seq: AtomicIsize::new(0),
        }
    }
}

pub type Session = SessionWrapper<SessionState>;
