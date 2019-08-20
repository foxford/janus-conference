use std::sync::Arc;

use futures::lazy;
use serde_json::Value as JsonValue;

use super::{Operation as OperationTrait, OperationError};
use crate::recorder::Recorder;
use crate::session::Session;
use crate::uploader::Uploader;

#[derive(Clone, Debug, Deserialize)]
pub struct Operation {
    id: String,
    bucket: String,
    object: String,
}

impl OperationTrait for Operation {
    fn call(
        &self,
        _session: Arc<Session>,
        respond: Box<dyn Fn(Result<JsonValue, OperationError>) + Send>,
    ) -> Result<(), OperationError> {
        janus_info!("[CONFERENCE] Handling upload message with id {}", self.id);

        let app = app!()?;
        let mut recorder = Recorder::new(&app.config.recordings, &self.id);
        let id = self.id.to_owned();
        let bucket = self.bucket.to_owned();
        let object = self.object.to_owned();

        app.thread_pool.spawn(lazy(move || {
            let mut job = move || {
                janus_info!("[CONFERENCE] Upload task started. Finishing record");
                let (started_at, time) = recorder.finish_record()?;

                janus_info!("[CONFERENCE] Uploading record");
                let uploader = Uploader::new(app!()?.config.uploading.clone())
                    .map_err(|err| format_err!("Failed to init uploader: {}", err))?;

                let path = recorder.get_full_record_path();
                uploader.upload_file(&path, &bucket, &object)?;
                janus_info!("[CONFERENCE] Uploading finished");

                Ok(json!({ "id": id, "started_at": started_at, "time": time }))
            };

            respond(job());
            Ok(())
        }));

        Ok(())
    }

    fn error_kind(&self) -> (&str, &str) {
        (
            "stream_upload_error",
            "Error uploading a recording of stream",
        )
    }

    fn is_handle_jsep(&self) -> bool {
        false
    }
}
