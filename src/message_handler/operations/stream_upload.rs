use failure::{err_msg, Error};
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::recorder::{Recorder, RecorderError};
use crate::uploader::Uploader;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    id: String,
    bucket: String,
    object: String,
}

#[derive(Serialize)]
struct Response {
    id: String,
    started_at: u64,
    time: Vec<(u64, u64)>,
}

impl<C> super::Operation<C> for Request {
    fn call(&self, _request: &super::Request<C>) -> super::OperationResult {
        janus_info!(
            "[CONFERENCE] Calling stream.upload operation with id {}",
            self.id
        );

        let app = app!().map_err(internal_error)?;
        let mut recorder = Recorder::new(&app.config.recordings, &self.id);

        janus_info!("[CONFERENCE] Upload task started. Finishing record");
        let (started_at, time) = recorder.finish_record().map_err(recorder_error)?;

        janus_info!("[CONFERENCE] Uploading record");
        let uploader = Uploader::new(app.config.uploading.clone())
            .map_err(|err| internal_error(format_err!("Failed to init uploader: {}", err)))?;

        let path = recorder.get_full_record_path();

        uploader
            .upload_file(&path, &self.bucket, &self.object)
            .map_err(internal_error)?;

        janus_info!("[CONFERENCE] Uploading finished, deleting source files");
        recorder.delete_record().map_err(recorder_error)?;

        Ok(Response {
            id: self.id.clone(),
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
            error(StatusCode::NOT_FOUND, err_msg("Record not found"))
        }
    }
}
