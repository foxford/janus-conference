use std::sync::Arc;

use failure::Error;
use serde_json::Value as JsonValue;

use super::{Operation as OperationTrait, OperationError};
use crate::recorder::Recorder;
use crate::session::Session;

#[derive(Clone, Debug, Deserialize)]
pub struct Operation {
    id: String,
}

impl OperationTrait for Operation {
    fn call(
        &self,
        session: Arc<Session>,
        respond: Box<dyn Fn(Result<JsonValue, OperationError>) + Send>,
    ) -> Result<(), OperationError> {
        janus_info!("[CONFERENCE] Handling create message with id {}", self.id);
        let app = app!()?;

        app.switchboard
            .with_write_lock(move |mut switchboard| {
                switchboard.create_stream(&self.id, session.clone());

                let start_recording_result: Result<(), Error> = {
                    if app.config.recordings.enabled {
                        let mut recorder = Recorder::new(&app.config.recordings, &self.id);
                        recorder.start_recording()?;
                        switchboard.attach_recorder(session.clone(), recorder);
                    }

                    Ok(())
                };

                match start_recording_result {
                    Ok(()) => respond(Ok(json!({}))),
                    Err(err) => switchboard
                        .remove_stream(&self.id)
                        .map_err(|remove_err| format_err!(
                            "Failed to remove stream {}: {} while recovering from another error: {}",
                            self.id, remove_err, err,
                        ))?,
                }

                Ok(())
            })
            .map_err(|err| err.into())
    }

    fn error_kind(&self) -> (&str, &str) {
        ("stream_create_error", "Error creating a stream")
    }

    fn is_handle_jsep(&self) -> bool {
        true
    }
}
