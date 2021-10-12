use anyhow::{anyhow, Result};
use axum::Json;
use serde::Deserialize;

use crate::{
    send_fir,
    switchboard::{StreamId, WriterConfig},
};

#[derive(Debug, Deserialize)]
pub struct Request {
    pub configs: Vec<ConfigItem>,
}

#[derive(Debug, Deserialize)]
pub struct ConfigItem {
    pub stream_id: StreamId,
    pub send_video: bool,
    pub send_audio: bool,
    pub video_remb: Option<u32>,
}

pub fn writer_config_update(request: Request) -> Result<()> {
    let app = app!()?;

    for config_item in &request.configs {
        if let Some(video_remb) = config_item.video_remb {
            if video_remb > app.config.constraint.writer.max_video_remb {
                return Err(anyhow!(
                    "Invalid video_remb: {} > {}",
                    video_remb,
                    app.config.constraint.writer.max_video_remb,
                ));
            }
        }
    }

    app.switchboard.with_write_lock(|mut switchboard| {
        for config_item in &request.configs {
            let mut writer_config = WriterConfig::new();
            writer_config.set_send_video(config_item.send_video);
            writer_config.set_send_audio(config_item.send_audio);

            if let Some(video_remb) = config_item.video_remb {
                writer_config.set_video_remb(video_remb);
            }
            let prev_config = switchboard.set_writer_config(config_item.stream_id, writer_config);
            if let (Some(prev_config), Some(session_id)) =
                (prev_config, switchboard.publisher_of(config_item.stream_id))
            {
                if config_item.send_video && !prev_config.send_video() {
                    send_fir(session_id, &switchboard);
                }
            }
        }

        Ok(())
    })?;
    Ok(())
}
