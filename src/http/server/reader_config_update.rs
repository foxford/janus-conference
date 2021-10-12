use crate::switchboard::{AgentId, ReaderConfig, StreamId};
use anyhow::Result;
use axum::Json;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Request {
    pub configs: Vec<ConfigItem>,
}

#[derive(Debug, Deserialize)]
pub struct ConfigItem {
    pub reader_id: AgentId,
    pub stream_id: StreamId,
    pub receive_video: bool,
    pub receive_audio: bool,
}

pub fn reader_config_update(request: Request) -> Result<()> {
    let app = app!()?;

    app.switchboard.with_write_lock(|mut switchboard| {
        for config_item in &request.configs {
            switchboard.update_reader_config(
                config_item.stream_id,
                &config_item.reader_id,
                ReaderConfig::new(config_item.receive_video, config_item.receive_audio),
            )?;
        }

        Ok(())
    })?;
    Ok(())
}
