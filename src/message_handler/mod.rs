mod generic;
mod operations;

use std::ffi::CString;

use anyhow::{format_err, Context, Result};
use janus::JanssonValue;

use self::generic::{MessageHandlingLoop as GenericLoop, Router, Sender};
use crate::janus_callbacks;
use crate::switchboard::SessionId;

pub use self::generic::{Operation, OperationResult, Request};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "method")]
pub enum Method {
    #[serde(rename = "agent.leave")]
    AgentLeave(operations::agent_leave::Request),
    #[serde(rename = "stream.create")]
    StreamCreate(operations::stream_create::Request),
    #[serde(rename = "stream.read")]
    StreamRead(operations::stream_read::Request),
    #[serde(rename = "stream.upload")]
    StreamUpload(operations::stream_upload::Request),
}

impl Into<Box<dyn Operation>> for Method {
    fn into(self) -> Box<dyn Operation> {
        match self {
            Method::AgentLeave(op) => Box::new(op),
            Method::StreamCreate(op) => Box::new(op),
            Method::StreamRead(op) => Box::new(op),
            Method::StreamUpload(op) => Box::new(op),
        }
    }
}

impl Router for Method {}

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
