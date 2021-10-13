mod generic;
mod operations;

use std::ffi::CString;

use self::generic::Sender;
use crate::switchboard::SessionId;
use crate::{janus_callbacks, switchboard::StreamId};
use anyhow::{Context, Result};
use janus_plugin::JanssonValue;
use serde::Deserialize;

pub use self::generic::{
    handle_request, prepare_request, send_response, send_speaking_notification, PreparedRequest,
    Request,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "method")]
pub enum Method {
    #[serde(rename = "stream.create")]
    StreamCreate(operations::stream_create::Request),
    #[serde(rename = "stream.read")]
    StreamRead(operations::stream_read::Request),
}

impl Method {
    pub fn stream_id(&self) -> StreamId {
        match self {
            Method::StreamCreate(x) => x.id,
            Method::StreamRead(x) => x.id,
        }
    }
}

#[derive(Clone, Debug)]
pub struct JanusSender;

impl JanusSender {
    pub fn new() -> Self {
        Self {}
    }
}

impl Sender for JanusSender {
    fn send(
        &self,
        session_id: SessionId,
        transaction: &str,
        payload: Option<JanssonValue>,
        jsep_answer: Option<JanssonValue>,
    ) -> Result<()> {
        app!()?.switchboard.with_read_lock(move |switchboard| {
            let session = switchboard.session(session_id)?;

            let txn = CString::new(transaction.to_owned())
                .context("Failed to cast transaction to CString")?;

            janus_callbacks::push_event(&*session, txn.into_raw(), payload, jsep_answer)
                .context("Failed to push event")
        })
    }
}
