use anyhow::Error;
use async_trait::async_trait;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::switchboard::StreamId;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    id: StreamId,
}

#[derive(Serialize)]
struct Response {}

#[async_trait]
impl super::Operation for Request {
    async fn call(&self, request: &super::Request) -> super::OperationResult {
        verb!(
            "Calling stream.create operation";
            {"rtc_id": self.id, "handle_id": request.session_id()}
        );

        let internal_error = |err: Error| {
            SvcError::builder()
                .kind("stream_create_error", "Error creating a stream")
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .detail(&err.to_string())
                .build()
        };

        let app = app!().map_err(internal_error)?;

        app.switchboard
            .with_write_lock(|mut switchboard| {
                switchboard.set_writer(self.id, request.session_id())
            })
            .map_err(internal_error)?;

        Ok(Response {}.into())
    }

    fn is_handle_jsep(&self) -> bool {
        false
    }
}
