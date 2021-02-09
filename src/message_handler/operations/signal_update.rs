use anyhow::Error;
use async_trait::async_trait;
use http::StatusCode;
use svc_error::Error as SvcError;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {}

#[derive(Serialize)]
struct Response {}

#[async_trait]
impl super::Operation for Request {
    async fn call(&self, request: &super::Request) -> super::OperationResult {
        verb!("Calling signal.update operation"; {"handle_id": request.session_id()});

        let error = |status: StatusCode, err: Error| {
            SvcError::builder()
                .kind("signal_update_error", "Error updating signal")
                .status(status)
                .detail(&err.to_string())
                .build()
        };

        let app = app!().map_err(|err| error(StatusCode::INTERNAL_SERVER_ERROR, err))?;
        let session_id = request.session_id().to_owned();

        app.switchboard_dispatcher
            .dispatch(move |switchboard| -> anyhow::Result<()> {
                if let Some(stream_id) = switchboard.read_by(session_id) {
                    switchboard.remove_reader(stream_id, session_id);
                } else if let Some(stream_id) = switchboard.written_by(session_id) {
                    switchboard.remove_writer(stream_id)?;
                }

                Ok(())
            })
            .await
            .map_err(|err| error(StatusCode::INTERNAL_SERVER_ERROR, err))?
            .map_err(|err| error(StatusCode::INTERNAL_SERVER_ERROR, err))?;

        Ok(Response {}.into())
    }

    fn is_handle_jsep(&self) -> bool {
        true
    }
}
