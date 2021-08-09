use anyhow::Error;
use async_trait::async_trait;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::{message_handler::generic::MethodKind, switchboard::StreamId};

#[derive(Clone, Debug, Deserialize)]
pub struct Request {}

#[derive(Serialize)]
struct Response {}

#[async_trait]
impl super::Operation for Request {
    async fn call(&self, request: &super::Request) -> super::OperationResult {
        let app = app!().map_err(internal_error)?;

        app.switchboard
            .with_write_lock(|mut switchboard| {
                switchboard.touch_session(request.session_id());
                Ok(())
            })
            .map_err(internal_error)?;

        Ok(Response {}.into())
    }

    fn stream_id(&self) -> Option<StreamId> {
        None
    }

    fn method_kind(&self) -> Option<MethodKind> {
        Some(MethodKind::ServicePing)
    }
}

fn internal_error(err: Error) -> SvcError {
    SvcError::builder()
        .kind("touch_session_error", "Error toucing session")
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .detail(&err.to_string())
        .build()
}
