mod generic;
mod operations;

use std::ffi::CString;
use std::sync::Arc;

use failure::Error;
use janus::JanssonValue;

use self::generic::{MessageHandlingLoop as GenericLoop, Router, Sender};
use crate::janus_callbacks;
use crate::session::Session;

pub use self::generic::{Operation, OperationResult, Request};

pub type Context = Arc<Session>;

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

impl Into<Box<dyn Operation<Context>>> for Method {
    fn into(self) -> Box<dyn Operation<Context>> {
        match self {
            Method::AgentLeave(op) => Box::new(op),
            Method::StreamCreate(op) => Box::new(op),
            Method::StreamRead(op) => Box::new(op),
            Method::StreamUpload(op) => Box::new(op),
        }
    }
}

impl Router<Context> for Method {}

#[derive(Clone)]
pub struct JanusSender;

impl JanusSender {
    pub fn new() -> Self {
        Self {}
    }
}

impl Sender<Context> for JanusSender {
    fn send(
        &self,
        session: &Context,
        transaction: &str,
        payload: Option<JanssonValue>,
        jsep_answer: Option<JanssonValue>,
    ) -> Result<(), Error> {
        CString::new(transaction.to_owned())
            .map_err(Error::from)
            .and_then(|transaction| {
                janus_callbacks::push_event(&session, transaction.into_raw(), payload, jsep_answer)
                    .map_err(Error::from)
            })
    }
}

pub type MessageHandlingLoop = GenericLoop<Context, Method, JanusSender>;
