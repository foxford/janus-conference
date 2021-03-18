use anyhow::Error;
use async_trait::async_trait;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::switchboard::{ReaderConfig, SessionId, StreamId};

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    configs: Vec<ConfigItem>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConfigItem {
    reader_id: SessionId,
    stream_id: StreamId,
    receive_video: bool,
    receive_audio: bool,
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
                        config_item.reader_id,
                        ReaderConfig::new(config_item.receive_video, config_item.receive_audio),
                    )?;
                }

                Ok(())
            })
            .map_err(internal_error)?;

        Ok(Response {}.into())
    }

    fn stream_id(&self) -> Option<StreamId> {
        None
    }
}

fn internal_error(err: Error) -> SvcError {
    SvcError::builder()
        .kind("reader_config_update_error", "Error updating reader config")
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .detail(&err.to_string())
        .build()
}
