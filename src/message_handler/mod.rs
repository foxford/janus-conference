mod generic;
mod operations;

use std::ffi::CString;

use anyhow::{Context, Result};
use async_trait::async_trait;
use janus::JanssonValue;

use self::generic::Sender;
use crate::janus_callbacks;
use crate::switchboard::SessionId;

pub use self::generic::{
    handle_request, prepare_request, send_response, send_speaking_notification, MethodKind,
    Operation, OperationResult, PreparedRequest, Request,
};

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
    #[serde(rename = "service.ping")]
    ServicePing(operations::service_ping::Request),
}

#[async_trait]
impl Operation for Method {
    async fn call(&self, request: &generic::Request) -> OperationResult {
        match self {
            Method::AgentLeave(x) => x.call(request).await,
            Method::ReaderConfigUpdate(x) => x.call(request).await,
            Method::StreamCreate(x) => x.call(request).await,
            Method::StreamRead(x) => x.call(request).await,
            Method::StreamUpload(x) => x.call(request).await,
            Method::WriterConfigUpdate(x) => x.call(request).await,
            Method::ServicePing(x) => x.call(request).await,
        }
    }

    fn stream_id(&self) -> Option<crate::switchboard::StreamId> {
        match self {
            Method::AgentLeave(x) => x.stream_id(),
            Method::ReaderConfigUpdate(x) => x.stream_id(),
            Method::StreamCreate(x) => x.stream_id(),
            Method::StreamRead(x) => x.stream_id(),
            Method::StreamUpload(x) => x.stream_id(),
            Method::WriterConfigUpdate(x) => x.stream_id(),
            Method::ServicePing(x) => x.stream_id(),
        }
    }

    fn method_kind(&self) -> Option<MethodKind> {
        match self {
            Method::AgentLeave(x) => x.method_kind(),
            Method::ReaderConfigUpdate(x) => x.method_kind(),
            Method::StreamCreate(x) => x.method_kind(),
            Method::StreamRead(x) => x.method_kind(),
            Method::StreamUpload(x) => x.method_kind(),
            Method::WriterConfigUpdate(x) => x.method_kind(),
            Method::ServicePing(x) => x.method_kind(),
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
