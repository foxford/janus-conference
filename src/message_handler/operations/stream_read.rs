use anyhow::Error;
use async_trait::async_trait;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::{
    message_handler::generic::MethodKind,
    switchboard::{AgentId, JoinStreamError, StreamId},
};

use super::stream_create::ReaderConfig;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    id: StreamId,
    agent_id: AgentId,
    #[serde(default)]
    reader_configs: Option<Vec<ReaderConfig>>,
}

#[derive(Serialize)]
struct Response {}

#[async_trait]
impl super::Operation for Request {
    async fn call(&self, request: &super::Request) -> super::OperationResult {
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
                // this wrapped in `Ok` to avoid using anyhow so we can distingiush
                // different error types
                // TODO: refactor `with_write_lock` to allow custom error types
                Ok(switchboard
                    .join_stream(self.id, request.session_id(), self.agent_id.to_owned())
                    .map_err(|e| match e {
                        JoinStreamError::StreamNotFound => error(
                            StatusCode::NOT_FOUND,
                            anyhow!("Stream {} does not exist", self.id),
                        ),
                        JoinStreamError::SessionNotFound => {
                            error(StatusCode::NOT_FOUND, anyhow!("Session does not exist"))
                        }
                        JoinStreamError::TooManyAgents => error(
                            StatusCode::SERVICE_UNAVAILABLE,
                            anyhow!("Too many agents on server"),
                        ),
                    }))
            })
            .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))??;

        if let Some(configs) = &self.reader_configs {
            let configs = configs
                .iter()
                .map(|c| super::reader_config_update::ConfigItem {
                    stream_id: self.id,
                    receive_video: c.receive_video,
                    receive_audio: c.receive_audio,
                    reader_id: c.reader_id.clone(),
                })
                .collect();
            super::reader_config_update::Request { configs }
                .call(request)
                .await?;
        }

        Ok(Response {}.into())
    }

    fn stream_id(&self) -> Option<StreamId> {
        Some(self.id)
    }

    fn method_kind(&self) -> Option<MethodKind> {
        Some(MethodKind::StreamRead)
    }
}
