use std::fmt;
use std::sync::Arc;

use failure::Error;
use http::StatusCode;
use serde_json::Value as JsonValue;

use crate::session::Session;

pub mod stream_create;
pub mod stream_read;
pub mod stream_upload;

pub trait Operation {
    fn call(
        &self,
        session: Arc<Session>,
        respond: Box<dyn Fn(Result<JsonValue, OperationError>) + Send>,
    ) -> Result<(), OperationError>;

    fn error_kind(&self) -> (&str, &str);
    fn is_handle_jsep(&self) -> bool;
}

pub struct OperationError {
    status: StatusCode,
    cause: Error,
}

impl OperationError {
    pub fn new(status: StatusCode, cause: Error) -> Self {
        Self { status, cause }
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn cause(&self) -> &Error {
        &self.cause
    }
}

impl fmt::Display for OperationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Operation error ({}): {}", self.status(), self.cause())
    }
}

impl From<Error> for OperationError {
    fn from(cause: Error) -> Self {
        OperationError::new(StatusCode::INTERNAL_SERVER_ERROR, cause)
    }
}
