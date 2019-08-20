use std::fmt;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value as JsonValue;
use svc_error::Error as SvcError;

use crate::session::Session;

pub mod stream_create;
pub mod stream_read;
pub mod stream_upload;

pub trait Operation: fmt::Debug + Send {
    /// Operation implementation
    fn call(&self, session: Arc<Session>) -> self::Result;

    /// Whether MessageHandler should process SDP offer/answer before calling this operation.
    fn is_handle_jsep(&self) -> bool;
}

pub type Result = std::result::Result<Success, SvcError>;

pub struct Success {
    payload: JsonValue,
}

// TODO: Replace with TryFrom. Currently it gives a collision with the blanket implementation.
// https://github.com/rust-lang/rust/issues/50133
impl<T> From<T> for Success
where
    T: Serialize,
{
    fn from(response: T) -> Self {
        match serde_json::to_value(&response) {
            Ok(payload) => Self { payload },
            Err(err) => {
                janus_err!("Failed to serialize response payload: {}", err);
                Self {
                    payload: serde_json::from_str("Serialization error").unwrap(),
                }
            }
        }
    }
}

impl From<Success> for JsonValue {
    fn from(success: Success) -> Self {
        success.payload
    }
}
