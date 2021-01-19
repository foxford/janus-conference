use anyhow::Error;
use async_trait::async_trait;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::jsep::Jsep;

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
                .kind("stream_read_error", "Error reading a stream")
                .status(status)
                .detail(&err.to_string())
                .build()
        };

        let jsep_offer = request
            .jsep_offer()
            .ok_or_else(|| error(StatusCode::BAD_REQUEST, anyhow!("Missing JSEP")))?;

        let will_be_writer =
            Jsep::is_writer(jsep_offer).map_err(|err| error(StatusCode::BAD_REQUEST, err))?;

        let app = app!().map_err(|err| error(StatusCode::INTERNAL_SERVER_ERROR, err))?;
        let session_id = request.session_id().to_owned();

        app.switchboard_dispatcher
            .dispatch(move |switchboard| -> anyhow::Result<()> {
                if will_be_writer {
                    // Reader becomes writer.
                    if let Some(stream_id) = switchboard.read_by(session_id) {
                        switchboard.remove_reader(stream_id, session_id);
                        switchboard.set_writer(stream_id, session_id)?;
                    }
                } else {
                    // Writer becomes reader.
                    if let Some(stream_id) = switchboard.written_by(session_id) {
                        switchboard.remove_writer(stream_id)?;
                        switchboard.add_reader(stream_id, session_id);
                    }
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
