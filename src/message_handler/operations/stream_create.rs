use crate::{
    http::server::{reader_config_update, writer_config_update},
    switchboard::{AgentId, StreamId},
};
use anyhow::Result;
use anyhow::{format_err, Error};

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    pub id: StreamId,
    pub agent_id: AgentId,
    pub writer_config: Option<WriterConfig>,
    pub reader_configs: Option<Vec<ReaderConfig>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ReaderConfig {
    reader_id: AgentId,
    receive_video: bool,
    receive_audio: bool,
}

#[derive(Deserialize, Debug, Clone)]
pub struct WriterConfig {
    send_video: bool,
    send_audio: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    video_remb: Option<u32>,
}

impl Request {
    pub fn stream_create(&self, request: &super::Request) -> Result<()> {
        verb!("Calling stream.create operation"; {"rtc_id": self.id});

        let app = app!()?;
        app.switchboard.with_write_lock(|mut switchboard| {
            switchboard.create_stream(self.id, request.session_id(), self.agent_id.to_owned())?;
            let mut start_recording = || {
                if app.config.recordings.enabled {
                    let recorder = app.recorders_creator.new_handle(self.id);
                    recorder.start_recording()?;
                    verb!("Attaching recorder"; {"handle_id": request.session_id()});
                    let session_state = switchboard.state_mut(request.session_id())?;
                    session_state.set_recorder(recorder);
                    session_state.set_audio_level_ext_id(request.audio_level_ext_id());
                }

                Ok(())
            };

            start_recording().or_else(|err: Error| {
                err!("Failed to start recording; stopping the stream"; {"rtc_id": self.id});

                switchboard.remove_stream(self.id).map_err(|remove_err| {
                    format_err!(
                        "Failed to remove stream {}: {} while recovering from another error: {}",
                        self.id,
                        remove_err,
                        err
                    )
                })?;
                Ok(())
            })
        })?;
        if let Some(config) = &self.writer_config {
            let config_item = writer_config_update::ConfigItem {
                stream_id: self.id,
                send_video: config.send_video,
                send_audio: config.send_audio,
                video_remb: config.video_remb,
            };
            writer_config_update::writer_config_update(writer_config_update::Request {
                configs: vec![config_item],
            })?;
        }

        if let Some(configs) = &self.reader_configs {
            let configs = configs
                .iter()
                .map(|c| reader_config_update::ConfigItem {
                    stream_id: self.id,
                    receive_video: c.receive_video,
                    receive_audio: c.receive_audio,
                    reader_id: c.reader_id.clone(),
                })
                .collect();
            reader_config_update::reader_config_update(reader_config_update::Request { configs })?;
        }

        Ok(())
    }
}
