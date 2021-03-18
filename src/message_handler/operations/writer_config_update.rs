use anyhow::Error;
use async_trait::async_trait;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::switchboard::{StreamId, WriterConfig};

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
    audio_remb: Option<u32>,
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
                if video_remb > app.config.constraint.writer.video.max_remb {
                    return Err(bad_request_error(anyhow!(
                        "Invalid video_remb: {} > {}",
                        video_remb,
                        app.config.constraint.writer.video.max_remb,
                    )));
                }
            }

            if let Some(audio_remb) = config_item.audio_remb {
                if audio_remb > app.config.constraint.writer.audio.max_remb {
                    return Err(bad_request_error(anyhow!(
                        "Invalid audio_remb: {} > {}",
                        audio_remb,
                        app.config.constraint.writer.audio.max_remb,
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

                    if let Some(audio_remb) = config_item.audio_remb {
                        writer_config.set_audio_remb(audio_remb);
                    }

                    switchboard.set_writer_config(config_item.stream_id, writer_config);
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
