use anyhow::Error;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::switchboard::{AgentId, StreamId};

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    id: StreamId,
    agent_id: AgentId,
}

#[derive(Serialize)]
struct Response {}

impl super::Operation for Request {
    fn call(&self, request: &super::Request) -> super::OperationResult {
        janus_info!(
            "[CONFERENCE] Calling stream.read operation with id {}",
            self.id
        );

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

    fn is_handle_jsep(&self) -> bool {
        true
    }
}
