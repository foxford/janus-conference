use failure::Error;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::recorder::Recorder;
use crate::switchboard::{AgentId, StreamId};

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    id: StreamId,
    agent_id: AgentId,
}

#[derive(Serialize)]
struct Response {}

impl super::Operation for Request {
    fn call(&self, request: &super::Request) -> super::OperationResult {
        janus_info!(
            "[CONFERENCE] Calling stream.create operation with id {}",
            self.id
        );

        let internal_error = |err: Error| {
            SvcError::builder()
                .kind("stream_create_error", "Error creating a stream")
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .detail(&err.to_string())
                .build()
        };

        let app = app!().map_err(internal_error)?;

        app.switchboard.with_write_lock(|mut switchboard| {
            switchboard.create_stream(self.id, request.session_id(), self.agent_id.clone())?;

            let mut start_recording = || {
                if app.config.recordings.enabled {
                    let mut recorder = Recorder::new(&app.config.recordings, self.id);
                    recorder.start_recording()?;
                    janus_verb!("[CONFERENCE] Attaching recorder for {}", request.session_id());
                    switchboard.state_mut(request.session_id())?.set_recorder(recorder);
                }

                Ok(())
            };

            start_recording().or_else(|err: Error| {
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

    fn is_handle_jsep(&self) -> bool {
        true
    }
}
