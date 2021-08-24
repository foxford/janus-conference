use anyhow::{format_err, Error};
use async_trait::async_trait;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::{
    message_handler::generic::MethodKind,
    switchboard::{AgentId, StreamId},
};

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    id: StreamId,
    agent_id: AgentId,
}

#[derive(Serialize)]
struct Response {}

#[async_trait]
impl super::Operation for Request {
    async fn call(&self, request: &super::Request) -> super::OperationResult {
        verb!("Calling stream.create operation"; {"rtc_id": self.id});

        let internal_error = |err: Error| {
            SvcError::builder()
                .kind("stream_create_error", "Error creating a stream")
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .detail(&err.to_string())
                .build()
        };

        let app = app!().map_err(internal_error)?;

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

                switchboard
                    .remove_stream(self.id)
                    .map_err(|remove_err| {
                        format_err!(
                            "Failed to remove stream {}: {} while recovering from another error: {}",
                            self.id, remove_err, err
                        )
                    })
            })
        })
        .map_err(internal_error)?;

        Ok(Response {}.into())
    }

    fn stream_id(&self) -> Option<StreamId> {
        Some(self.id)
    }

    fn method_kind(&self) -> Option<MethodKind> {
        Some(MethodKind::StreamCreate)
    }
}
