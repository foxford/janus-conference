use anyhow::Error;
use async_trait::async_trait;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::{
    message_handler::generic::MethodKind,
    switchboard::{AgentId, ReaderConfig, StreamId},
};

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    pub configs: Vec<ConfigItem>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConfigItem {
    pub reader_id: AgentId,
    pub stream_id: StreamId,
    pub receive_video: bool,
    pub receive_audio: bool,
}

#[derive(Serialize)]
struct Response {}

#[async_trait]
impl super::Operation for Request {
    async fn call(&self, _request: &super::Request) -> super::OperationResult {
        verb!("Calling reader_config.update operation");

        let app = app!().map_err(internal_error)?;

        app.switchboard
            .with_write_lock(|mut switchboard| {
                for config_item in &self.configs {
                    switchboard.update_reader_config(
                        config_item.stream_id,
                        &config_item.reader_id,
                        ReaderConfig::new(config_item.receive_video, config_item.receive_audio),
                    );
                }

                Ok(())
            })
            .map_err(internal_error)?;

        Ok(Response {}.into())
    }

    fn stream_id(&self) -> Option<StreamId> {
        None
    }

    fn method_kind(&self) -> Option<MethodKind> {
        Some(MethodKind::ReaderConfigUpdate)
    }
}

fn internal_error(err: Error) -> SvcError {
    SvcError::builder()
        .kind("reader_config_update_error", "Error updating reader config")
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .detail(&err.to_string())
        .build()
}
