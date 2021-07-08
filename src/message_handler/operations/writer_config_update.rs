use anyhow::Error;
use async_trait::async_trait;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::{
    message_handler::generic::MethodKind,
    send_fir,
    switchboard::{StreamId, WriterConfig},
};

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    configs: Vec<ConfigItem>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConfigItem {
    stream_id: StreamId,
    send_video: bool,
    send_audio: bool,
    video_remb: Option<u32>,
}

#[derive(Serialize)]
struct Response {}

#[async_trait]
impl super::Operation for Request {
    async fn call(&self, _request: &super::Request) -> super::OperationResult {
        verb!("Calling writer_config.update operation");
        let app = app!().map_err(internal_error)?;

        // Validate REMBs.
        for config_item in &self.configs {
            if let Some(video_remb) = config_item.video_remb {
                if video_remb > app.config.constraint.writer.max_video_remb {
                    return Err(bad_request_error(anyhow!(
                        "Invalid video_remb: {} > {}",
                        video_remb,
                        app.config.constraint.writer.max_video_remb,
                    )));
                }
            }
        }

        // Update writer config for the stream.
        app.switchboard
            .with_write_lock(|mut switchboard| {
                for config_item in &self.configs {
                    let mut writer_config = WriterConfig::new();
                    writer_config.set_send_video(config_item.send_video);
                    writer_config.set_send_audio(config_item.send_audio);

                    if let Some(video_remb) = config_item.video_remb {
                        writer_config.set_video_remb(video_remb);
                    }
                    let prev_config =
                        switchboard.set_writer_config(config_item.stream_id, writer_config);
                    if let (Some(prev_config), Some(session_id)) =
                        (prev_config, switchboard.publisher_of(config_item.stream_id))
                    {
                        if (config_item.send_audio && !prev_config.send_audio())
                            || (config_item.send_video && !prev_config.send_video())
                        {
                            send_fir(session_id, &switchboard);
                        }
                    }
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
        Some(MethodKind::WriterConfigUpdate)
    }
}

fn error(status: StatusCode, err: Error) -> SvcError {
    SvcError::builder()
        .kind("writer_config_update_error", "Error updating writer config")
        .status(status)
        .detail(&err.to_string())
        .build()
}

fn bad_request_error(err: Error) -> SvcError {
    error(StatusCode::BAD_REQUEST, err)
}

fn internal_error(err: Error) -> SvcError {
    error(StatusCode::INTERNAL_SERVER_ERROR, err)
}
