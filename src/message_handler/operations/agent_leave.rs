use anyhow::{format_err, Error};
use async_trait::async_trait;
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

#[async_trait]
impl super::Operation for Request {
    async fn call(&self, request: &super::Request) -> super::OperationResult {
        verb!("Calling agent.leave operation"; {"agent_id": self.agent_id});

        let error = |status: StatusCode, err: Error| {
            SvcError::builder()
                .kind("agent_leave_error", "Error handling left agent")
                .status(status)
                .detail(&err.to_string())
                .build()
        };

        let agent_id = self.agent_id.clone();
        let req_session_id = request.session_id().to_owned();

        app!()
            .map_err(|err| error(StatusCode::INTERNAL_SERVER_ERROR, err))?
            .switchboard_dispatcher
            .dispatch(move |switchboard| -> anyhow::Result<()> {
                for session_id in switchboard.agent_sessions(&agent_id) {
                    let session = switchboard.session(*session_id)?.lock().map_err(|err| {
                        format_err!(
                            "Failed to acquire session mutex for id = {}: {}",
                            req_session_id,
                            err
                        )
                    })?;

                    info!(
                        "Agent left; finishing session";
                        {"agent_id": agent_id, "session_id": session_id}
                    );

                    janus_callbacks::end_session(&session);
                }

                Ok(())
            })
            .await
            .map_err(|err| error(StatusCode::INTERNAL_SERVER_ERROR, err))?
            .map_err(|err| error(StatusCode::NOT_FOUND, err))?;

        Ok(Response {}.into())
    }

    fn is_handle_jsep(&self) -> bool {
        false
    }
}
