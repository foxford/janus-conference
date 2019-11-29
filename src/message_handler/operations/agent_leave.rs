use std::sync::Arc;

use failure::Error;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::session::Session;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {}

#[derive(Serialize)]
struct Response {}

impl super::Operation<Arc<Session>> for Request {
    fn call(&self, request: &super::Request<Arc<Session>>) -> super::OperationResult {
        let session = request.context();

        janus_info!("[CONFERENCE] Agent left {:p}", session.handle);

        let error = |status: StatusCode, err: Error| {
            SvcError::builder()
                .kind("agent_leave_error", "Error disconnecting agent")
                .status(status)
                .detail(&err.to_string())
                .build()
        };

        app!()
            .map_err(|err| error(StatusCode::INTERNAL_SERVER_ERROR, err))?
            .switchboard
            .with_write_lock(|mut switchboard| switchboard.disconnect(session))
            .map_err(|err| error(StatusCode::UNPROCESSABLE_ENTITY, err))?;

        Ok(Response {}.into())
    }

    fn is_handle_jsep(&self) -> bool {
        false
    }
}
