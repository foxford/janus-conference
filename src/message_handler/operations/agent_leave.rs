use anyhow::{format_err, Error};
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::janus_callbacks;
use crate::switchboard::AgentId;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    agent_id: AgentId,
}

#[derive(Serialize)]
struct Response {}

impl super::Operation for Request {
    fn call(&self, request: &super::Request) -> super::OperationResult {
        janus_info!("[CONFERENCE] Calling agent.leave operation");

        let error = |status: StatusCode, err: Error| {
            SvcError::builder()
                .kind("agent_leave_error", "Error handling left agent")
                .status(status)
                .detail(&err.to_string())
                .build()
        };

        app!()
            .map_err(|err| error(StatusCode::INTERNAL_SERVER_ERROR, err))?
            .switchboard
            .with_read_lock(|switchboard| {
                for session_id in switchboard.agent_sessions(&self.agent_id) {
                    let session = switchboard.session(*session_id)?.lock().map_err(|err| {
                        format_err!(
                            "Failed to acquire session mutex for id = {}: {}",
                            request.session_id(),
                            err
                        )
                    })?;

                    janus_callbacks::end_session(&session);
                }

                Ok(())
            })
            .map_err(|err| error(StatusCode::NOT_FOUND, err))?;

        Ok(Response {}.into())
    }

    fn is_handle_jsep(&self) -> bool {
        false
    }
}
