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
            "Calling stream.read operation";
            {"rtc_id": self.id, "handle_id": request.session_id()}
        );

        let error = |status: StatusCode, err: Error| {
            SvcError::builder()
                .kind("stream_read_error", "Error reading a stream")
                .status(status)
                .detail(&err.to_string())
                .build()
        };

        let id = self.id;
        let session_id = request.session_id().to_owned();

        app!()
            .map_err(|err| error(StatusCode::INTERNAL_SERVER_ERROR, err))?
            .switchboard_dispatcher
            .dispatch(move |switchboard| switchboard.add_reader(id, session_id))
            .await
            .map_err(|err| error(StatusCode::UNPROCESSABLE_ENTITY, err))?;

        Ok(Response {}.into())
    }

    fn is_handle_jsep(&self) -> bool {
        false
    }
}
