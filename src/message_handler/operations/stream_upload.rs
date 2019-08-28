use failure::Error;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::recorder::Recorder;
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

        let internal_error = |err: Error| {
            SvcError::builder()
                .kind(
                    "stream_upload_error",
                    "Error uploading a recording of stream",
                )
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .detail(&err.to_string())
                .build()
        };

        let app = app!().map_err(internal_error)?;
        let mut recorder = Recorder::new(&app.config.recordings, &self.id);

        janus_info!("[CONFERENCE] Upload task started. Finishing record");
        let (started_at, time) = recorder.finish_record().map_err(internal_error)?;

        janus_info!("[CONFERENCE] Uploading record");
        let uploader = Uploader::new(app.config.uploading.clone())
            .map_err(|err| internal_error(format_err!("Failed to init uploader: {}", err)))?;

        let path = recorder.get_full_record_path();

        uploader
            .upload_file(&path, &self.bucket, &self.object)
            .map_err(internal_error)?;

        janus_info!("[CONFERENCE] Uploading finished");

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
