use anyhow::{format_err, Error};
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::recorder::{Recorder, RecorderError};
use crate::switchboard::StreamId;
use crate::uploader::Uploader;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    id: StreamId,
    bucket: String,
    object: String,
}

#[derive(Serialize)]
struct Response {
    id: StreamId,
    started_at: u64,
    time: Vec<(u64, u64)>,
}

impl super::Operation for Request {
    fn call(&self, _request: &super::Request) -> super::OperationResult {
        janus_info!(
            "[CONFERENCE] Calling stream.upload operation with id {}",
            self.id
        );

        let app = app!().map_err(internal_error)?;

        app.switchboard
            .with_write_lock(|mut switchboard| {
                // The stream still may be ongoing and we must stop it gracefully.
                if let Some(publisher) = switchboard.publisher_of(self.id) {
                    // At first we synchronously stop the stream and hence the recording
                    // ensuring that it finishes correctly.
                    switchboard.remove_stream(self.id)?;

                    // Then we disconnect the publisher to close its PeerConnection and notify
                    // the frontend. Disconnection also implies stream removal but it's being
                    // performed asynchronously through a janus callback and to avoid race condition
                    // we have preliminary removed the stream in a synchronous way.
                    switchboard.disconnect(publisher)?;
                }

                Ok(())
            })
            .map_err(internal_error)?;

        janus_info!("[CONFERENCE] Finishing record");
        let mut recorder = Recorder::new(&app.config.recordings, self.id);
        let (started_at, time) = recorder.finish_record().map_err(recorder_error)?;

        janus_info!("[CONFERENCE] Uploading record");
        let uploader = Uploader::build(app.config.uploading.clone())
            .map_err(|err| internal_error(format_err!("Failed to init uploader: {}", err)))?;

        let path = recorder.get_full_record_path();

        uploader
            .upload_file(&path, &self.bucket, &self.object)
            .map_err(internal_error)?;

        janus_info!("[CONFERENCE] Uploading finished, deleting source files");
        recorder.delete_record().map_err(recorder_error)?;

        Ok(Response {
            id: self.id,
            started_at,
            time,
        }
        .into())
    }

    fn is_handle_jsep(&self) -> bool {
        false
    }
}

fn error(status: StatusCode, err: Error) -> SvcError {
    SvcError::builder()
        .kind(
            "stream_upload_error",
            "Error uploading a recording of stream",
        )
        .status(status)
        .detail(&err.to_string())
        .build()
}

fn internal_error(err: Error) -> SvcError {
    error(StatusCode::INTERNAL_SERVER_ERROR, err)
}

fn recorder_error(err: RecorderError) -> SvcError {
    match err {
        RecorderError::InternalError(cause) => internal_error(cause),
        RecorderError::IoError(cause) => {
            internal_error(format_err!("Recorder IO error: {}", cause))
        }
        RecorderError::RecordingMissing => {
            error(StatusCode::NOT_FOUND, format_err!("Record not found"))
        }
    }
}
