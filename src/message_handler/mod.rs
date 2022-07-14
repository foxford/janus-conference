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
    handle_request, handle_request_sync, prepare_request, send_response,
    send_speaking_notification, MethodKind, Operation, OperationResult, PreparedRequest, Request,
    SyncOperation,
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

impl Method {
    pub fn try_as_sync(self) -> Result<SyncMethod, Self> {
        match self {
            Method::AgentLeave(r) => Ok(SyncMethod::AgentLeave(r)),
            Method::ReaderConfigUpdate(r) => Ok(SyncMethod::ReaderConfigUpdate(r)),
            Method::StreamCreate(r) => Ok(SyncMethod::StreamCreate(r)),
            Method::StreamRead(r) => Ok(SyncMethod::StreamRead(r)),
            async_method @ Method::StreamUpload(_) => Err(async_method),
            Method::WriterConfigUpdate(r) => Ok(SyncMethod::WriterConfigUpdate(r)),
            Method::ServicePing(r) => Ok(SyncMethod::ServicePing(r)),
        }
    }
}

#[async_trait]
impl Operation for Method {
    async fn call(&self, request: &generic::Request) -> OperationResult {
        match self {
            Method::AgentLeave(x) => Operation::call(x, request).await,
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
            Method::AgentLeave(x) => Operation::stream_id(x),
            Method::ReaderConfigUpdate(x) => Operation::stream_id(x),
            Method::StreamCreate(x) => Operation::stream_id(x),
            Method::StreamRead(x) => Operation::stream_id(x),
            Method::StreamUpload(x) => x.stream_id(),
            Method::WriterConfigUpdate(x) => Operation::stream_id(x),
            Method::ServicePing(x) => Operation::stream_id(x),
        }
    }

    fn method_kind(&self) -> Option<MethodKind> {
        match self {
            Method::AgentLeave(x) => Operation::method_kind(x),
            Method::ReaderConfigUpdate(x) => Operation::method_kind(x),
            Method::StreamCreate(x) => Operation::method_kind(x),
            Method::StreamRead(x) => Operation::method_kind(x),
            Method::StreamUpload(x) => x.method_kind(),
            Method::WriterConfigUpdate(x) => Operation::method_kind(x),
            Method::ServicePing(x) => Operation::method_kind(x),
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

#[derive(Debug, Clone)]
pub enum SyncMethod {
    AgentLeave(operations::agent_leave::Request),
    ReaderConfigUpdate(operations::reader_config_update::Request),
    StreamCreate(operations::stream_create::Request),
    StreamRead(operations::stream_read::Request),
    WriterConfigUpdate(operations::writer_config_update::Request),
    ServicePing(operations::service_ping::Request),
}

impl SyncOperation for SyncMethod {
    fn sync_call(&self, request: &generic::Request) -> OperationResult {
        match self {
            SyncMethod::AgentLeave(r) => r.sync_call(request),
            SyncMethod::ReaderConfigUpdate(r) => r.sync_call(request),
            SyncMethod::StreamCreate(r) => r.sync_call(request),
            SyncMethod::StreamRead(r) => r.sync_call(request),
            SyncMethod::WriterConfigUpdate(r) => r.sync_call(request),
            SyncMethod::ServicePing(r) => r.sync_call(request),
        }
    }

    fn method_kind(&self) -> Option<MethodKind> {
        match self {
            SyncMethod::AgentLeave(r) => SyncOperation::method_kind(r),
            SyncMethod::ReaderConfigUpdate(r) => SyncOperation::method_kind(r),
            SyncMethod::StreamCreate(r) => SyncOperation::method_kind(r),
            SyncMethod::StreamRead(r) => SyncOperation::method_kind(r),
            SyncMethod::WriterConfigUpdate(r) => SyncOperation::method_kind(r),
            SyncMethod::ServicePing(r) => SyncOperation::method_kind(r),
        }
    }

    fn stream_id(&self) -> Option<crate::switchboard::StreamId> {
        match self {
            SyncMethod::AgentLeave(r) => SyncOperation::stream_id(r),
            SyncMethod::ReaderConfigUpdate(r) => SyncOperation::stream_id(r),
            SyncMethod::StreamCreate(r) => SyncOperation::stream_id(r),
            SyncMethod::StreamRead(r) => SyncOperation::stream_id(r),
            SyncMethod::WriterConfigUpdate(r) => SyncOperation::stream_id(r),
            SyncMethod::ServicePing(r) => SyncOperation::stream_id(r),
        }
    }
}
