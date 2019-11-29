use std::sync::{
    atomic::{AtomicIsize, Ordering},
    Arc, Mutex, RwLock,
};

use std::time::SystemTime;

use failure::Error;
use janus::session::SessionWrapper;

use crate::jsep::{Jsep, JsepStore};

#[derive(Debug)]
pub struct SessionState {
    fir_seq: AtomicIsize,
    // TODO: Do we really need Arc? The struct doesn't derive Clone & is under Arc already.
    //       Also get rid of pub field in favor of getter/setter.
    pub offer: Arc<Mutex<Option<Jsep>>>,
    last_rtp_packet_timestamp: Arc<Mutex<Option<SystemTime>>>,
    closing: RwLock<bool>,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            fir_seq: AtomicIsize::new(0),
            offer: Arc::new(Mutex::new(None)),
            last_rtp_packet_timestamp: Arc::new(Mutex::new(None)),
            closing: RwLock::new(false),
        }
    }

    pub fn incr_fir_seq(&self) -> isize {
        self.fir_seq.fetch_add(1, Ordering::Relaxed)
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

    pub fn closing(&self) -> &RwLock<bool> {
        &self.closing
    }
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.fir_seq.load(Ordering::SeqCst))
    }
}

pub type Session = SessionWrapper<SessionState>;

impl JsepStore for Arc<Session> {
    fn set_jsep(&self, offer: Jsep) -> Result<(), Error> {
        *self.offer.lock().map_err(|err| format_err!("{}", err))? = Some(offer);
        Ok(())
    }
}
