use anyhow::Error;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::{
    message_handler::generic::MethodKind,
    switchboard::{AgentId, StreamId},
};

use super::SyncOperation;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    id: StreamId,
    agent_id: AgentId,
}

#[derive(Serialize)]
struct Response {}

impl SyncOperation for Request {
    fn sync_call(&self, request: &super::Request) -> super::OperationResult {
        verb!("Calling stream.read operation"; {"rtc_id": self.id});

        let error = |status: StatusCode, err: Error| {
            SvcError::builder()
                .kind("stream_read_error", "Error reading a stream")
                .status(status)
                .detail(&err.to_string())
                .build()
        };

        app!()
            .map_err(|err| error(StatusCode::INTERNAL_SERVER_ERROR, err))?
            .switchboard
            .with_write_lock(|mut switchboard| {
                switchboard.join_stream(self.id, request.session_id(), self.agent_id.to_owned())
            })
            .map_err(|err| error(StatusCode::NOT_FOUND, err))?;

        Ok(Response {}.into())
    }

    fn method_kind(&self) -> Option<MethodKind> {
        Some(MethodKind::StreamRead)
    }

    fn stream_id(&self) -> Option<StreamId> {
        Some(self.id)
    }
}
