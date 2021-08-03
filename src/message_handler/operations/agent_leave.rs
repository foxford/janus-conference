use anyhow::Error;
use async_trait::async_trait;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::switchboard::{AgentId, StreamId};
use crate::{janus_callbacks, message_handler::generic::MethodKind};

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    agent_id: AgentId,
}

#[derive(Serialize)]
struct Response {}

#[async_trait]
impl super::Operation for Request {
    async fn call(&self, _request: &super::Request) -> super::OperationResult {
        verb!("Calling agent.leave operation"; {"agent_id": self.agent_id});

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
                    let session = switchboard.session(*session_id)?;

                    info!(
                        "Agent left; finishing session";
                        {"agent_id": self.agent_id, "session_id": session_id}
                    );

                    janus_callbacks::end_session(session);
                }

                Ok(())
            })
            .map_err(|err| error(StatusCode::NOT_FOUND, err))?;

        Ok(Response {}.into())
    }

    fn stream_id(&self) -> Option<StreamId> {
        None
    }

    fn method_kind(&self) -> Option<MethodKind> {
        Some(MethodKind::AgentLeave)
    }
}
