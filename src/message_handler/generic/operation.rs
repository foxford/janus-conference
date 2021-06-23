use std::fmt;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value as JsonValue;
use svc_error::Error as SvcError;

use crate::switchboard::StreamId;

#[async_trait]
pub trait Operation: fmt::Debug + Send + Sync {
    /// Operation implementation
    async fn call(&self, request: &super::Request) -> self::Result;
    fn method_kind(&self) -> Option<MethodKind>;
    /// If it returns `Some(stream_id)` then `MessageHandler` would process SDP offer/answer
    /// using writer config for the stream.
    fn stream_id(&self) -> Option<StreamId>;
}

pub enum MethodKind {
    AgentLeave,
    ReaderConfigUpdate,
    StreamCreate,
    StreamRead,
    StreamUpload,
    WriterConfigUpdate,
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
                err!("Failed to serialize response payload: {}", err);

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
