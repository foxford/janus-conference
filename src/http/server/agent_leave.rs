use anyhow::Error;
use async_trait::async_trait;
use axum::Json;
use http::StatusCode;
use serde::Deserialize;
use svc_error::Error as SvcError;

use crate::switchboard::{AgentId, StreamId};
use crate::{janus_callbacks, message_handler::generic::MethodKind};

#[derive(Debug, Deserialize)]
pub struct Request {
    agent_id: AgentId,
}

async fn agent_leave(Json(request): Json<Request>) -> Result<()> {
    app!()?.switchboard.with_read_lock(|switchboard| {
        for session_id in switchboard.agent_sessions(&request.agent_id) {
            let session = switchboard.session(*session_id)?;

            info!(
                "Agent left; finishing session";
                {"agent_id": request.agent_id, "session_id": session_id}
            );

            janus_callbacks::end_session(session);
        }

        Ok(())
    })?;
    Ok(())
}
