use std::sync::{
    atomic::{AtomicIsize, Ordering},
    Arc, Mutex,
};

use std::time::SystemTime;

use failure::Error;
use janus::session::SessionWrapper;

use messages::JsepKind;

#[derive(Debug)]
pub struct SessionState {
    fir_seq: AtomicIsize,
    pub offer: Arc<Mutex<Option<JsepKind>>>,
    last_rtp_packet_timestamp: Arc<Mutex<Option<SystemTime>>>,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            fir_seq: AtomicIsize::new(0),
            offer: Arc::new(Mutex::new(None)),
            last_rtp_packet_timestamp: Arc::new(Mutex::new(None)),
        }
    }

    pub fn incr_fir_seq(&self) -> isize {
        self.fir_seq.fetch_add(1, Ordering::Relaxed)
    }

    pub fn set_offer(&self, offer: JsepKind) -> Result<(), Error> {
        *self.offer.lock().map_err(|err| format_err!("{}", err))? = Some(offer);
        Ok(())
    }

    pub fn last_rtp_packet_timestamp(&self) -> Result<Option<SystemTime>, Error> {
        match self.last_rtp_packet_timestamp.lock() {
            Ok(timestamp) => Ok(timestamp.to_owned()),
            Err(err) => Err(format_err!("{}", err)),
        }
    }

    pub fn set_last_rtp_packet_timestamp(&self, value: Option<SystemTime>) -> Result<(), Error> {
        *self
            .last_rtp_packet_timestamp
            .lock()
            .map_err(|err| format_err!("{}", err))? = value;

        Ok(())
    }
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.fir_seq.load(Ordering::SeqCst))
    }
}

pub type Session = SessionWrapper<SessionState>;
