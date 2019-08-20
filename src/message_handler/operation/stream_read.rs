use std::sync::Arc;

use http::StatusCode;
use serde_json::Value as JsonValue;

use super::{Operation as OperationTrait, OperationError};
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
        janus_info!("[CONFERENCE] Handling read message with id {}", self.id);

        app!()?
            .switchboard
            .with_write_lock(|mut switchboard| switchboard.join_stream(&self.id, session.clone()))
            .map_err(|err| OperationError::new(StatusCode::NOT_FOUND, err))?;

        respond(Ok(json!({})));
        Ok(())
    }

    fn error_kind(&self) -> (&str, &str) {
        ("stream_read_error", "Error reading a stream")
    }

    fn is_handle_jsep(&self) -> bool {
        true
    }
}
