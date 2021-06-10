mod generic;
mod operations;

use std::ffi::CString;

use anyhow::{format_err, Context, Result};
use janus::JanssonValue;

use self::generic::{MessageHandlingLoop as GenericLoop, Router, Sender};
use crate::janus_callbacks;
use crate::switchboard::SessionId;

pub use self::generic::{response::Response, Operation, OperationResult, Request};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "method")]
pub enum Method {
    #[serde(rename = "agent.leave")]
    AgentLeave(operations::agent_leave::Request),
    #[serde(rename = "reader_config.update")]
    ReaderConfigUpdate(operations::reader_config_update::Request),
    #[serde(rename = "stream.create")]
    StreamCreate(operations::stream_create::Request),
    #[serde(rename = "stream.read")]
    StreamRead(operations::stream_read::Request),
    #[serde(rename = "stream.upload")]
    StreamUpload(operations::stream_upload::Request),
    #[serde(rename = "writer_config.update")]
    WriterConfigUpdate(operations::writer_config_update::Request),
}

impl Into<Box<dyn Operation>> for Method {
    fn into(self) -> Box<dyn Operation> {
        match self {
            Method::AgentLeave(op) => Box::new(op),
            Method::ReaderConfigUpdate(op) => Box::new(op),
            Method::StreamCreate(op) => Box::new(op),
            Method::StreamRead(op) => Box::new(op),
            Method::StreamUpload(op) => Box::new(op),
            Method::WriterConfigUpdate(op) => Box::new(op),
        }
    }
}

impl Router for Method {
    fn sync(&self) -> bool {
        match self {
            Method::StreamCreate(_) => true,
            Method::StreamRead(_) => true,
            _ => false,
        }
    }
}

#[derive(Clone)]
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
            let session = switchboard.session(session_id)?.lock().map_err(|err| {
                format_err!(
                    "Failed to acquire mutex for session {}: {}",
                    session_id,
                    err
                )
            })?;

            let txn = CString::new(transaction.to_owned())
                .context("Failed to cast transaction to CString")?;

            janus_callbacks::push_event(&*session, txn.into_raw(), payload, jsep_answer)
                .context("Failed to push event")
        })
    }
}

pub type MessageHandlingLoop = GenericLoop<Method, JanusSender>;
