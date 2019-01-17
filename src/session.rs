use std::sync::{
    atomic::{AtomicIsize, Ordering},
    Arc, Mutex,
};

use failure::Error;
use janus::sdp::Sdp;
use janus::session::SessionWrapper;

use messages::JsepKind;

#[derive(Debug)]
pub struct SessionState {
    fir_seq: AtomicIsize,
    pub subscriber_offer: Arc<Mutex<Option<JsepKind>>>,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            fir_seq: AtomicIsize::new(0),
            subscriber_offer: Arc::new(Mutex::new(None)),
        }
    }

    pub fn incr_fir_seq(&self) -> isize {
        self.fir_seq.fetch_add(1, Ordering::Relaxed)
    }

    pub fn set_subscriber_offer(&self, offer: Sdp) -> Result<(), Error> {
        let offer = offer.to_glibstring().to_string_lossy().to_string();

        *self
            .subscriber_offer
            .lock()
            .map_err(|err| format_err!("{}", err))? = Some(JsepKind::Offer { sdp: offer });
        Ok(())
    }
}

pub type Session = SessionWrapper<SessionState>;
