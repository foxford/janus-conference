use std::sync::{
    atomic::{AtomicIsize, Ordering},
    Arc, Mutex,
};

use failure::Error;
use janus::session::SessionWrapper;

use messages::JsepKind;

#[derive(Debug)]
pub struct SessionState {
    fir_seq: AtomicIsize,
    pub offer: Arc<Mutex<Option<JsepKind>>>,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            fir_seq: AtomicIsize::new(0),
            offer: Arc::new(Mutex::new(None)),
        }
    }

    pub fn incr_fir_seq(&self) -> isize {
        self.fir_seq.fetch_add(1, Ordering::Relaxed)
    }

    pub fn set_offer(&self, offer: JsepKind) -> Result<(), Error> {
        *self.offer.lock().map_err(|err| format_err!("{}", err))? = Some(offer);
        Ok(())
    }
}

pub type Session = SessionWrapper<SessionState>;
