mod operation;
mod request;
mod response;

use std::convert::TryFrom;

use anyhow::{format_err, Error};
use http::StatusCode;
use janus::JanssonValue;
use serde_json::Value as JsonValue;
use svc_error::{extension::sentry, Error as SvcError};

use self::response::Response;
use crate::switchboard::{AgentId, SessionId, StreamId};
use crate::utils;
use crate::{jsep::Jsep, message_handler::Method};

pub use self::operation::{MethodKind, Operation, Result as OperationResult};
pub use self::request::Request;

use super::JanusSender;

pub struct PreparedRequest<O> {
    request: Request,
    operation: O,
}
impl<O: Operation> PreparedRequest<O> {
    pub fn method_kind(&self) -> Option<MethodKind> {
        self.operation.method_kind()
    }
}

pub fn prepare_request(
    session_id: SessionId,
    transaction: &str,
    payload: &JanssonValue,
    jsep_offer: Option<JanssonValue>,
) -> anyhow::Result<PreparedRequest<Method>> {
    huge!("Start handling request"; {"handle_id": session_id, "transaction": transaction});
    let request = Request::new(session_id, transaction);
    let method = utils::jansson_to_serde::<Method>(payload)?;
    let request = match jsep_offer {
        None => request,
        Some(jansson_value) => {
            let jsep = utils::jansson_to_serde::<Jsep>(&jansson_value)?;
            let json_value = serde_json::to_value(jsep)?;
            let level_id = Jsep::find_audio_ext_id(&json_value);
            request
                .set_audio_level_ext_id(level_id)
                .set_jsep_offer(json_value)
        }
    };
    Ok(PreparedRequest {
        request,
        operation: method,
    })
}

pub async fn handle_request<O: Operation>(request: PreparedRequest<O>) -> Response {
    let result = async {
        let jsep_answer = request
            .operation
            .stream_id()
            .and_then(|stream_id| handle_jsep(&request.request, stream_id).transpose())
            .transpose()?;

        let payload = request
            .operation
            .call(&request.request)
            .await
            .map_err(|err| {
                notify_error(&err);
                err
            })
            .map(JsonValue::from)?;
        Ok::<_, SvcError>((jsep_answer, payload.into()))
    };
    match result.await {
        Ok((Some(jsep), payload)) => Response::new(request.request, payload).set_jsep_answer(jsep),
        Ok((None, payload)) => Response::new(request.request, payload),
        Err(err) => Response::new(request.request, err.into()),
    }
}

pub fn send_response(sender: impl Sender, response: Response) {
    huge!(
        "Handling response";
        {"handle_id": response.session_id(), "transaction": response.transaction()}
    );
    info!("Jsep answer: {:?}", response.jsep_answer());
    let jsep_answer = match response.jsep_answer() {
        None => None,
        Some(json_value) => match utils::serde_to_jansson(json_value) {
            Ok(jansson_value) => Some(jansson_value),
            Err(err) => {
                err!(
                    "Failed to serialize JSEP answer: {}", err;
                    {"handle_id": response.session_id(), "transaction": response.transaction()}
                );

                return;
            }
        },
    };

    huge!(
        "Sending response";
        {"handle_id": response.session_id(), "transaction": response.transaction()}
    );

    JanssonValue::try_from(response.payload())
        .and_then(|payload| {
            sender.send(
                response.session_id(),
                response.transaction(),
                Some(payload),
                jsep_answer,
            )
        })
        .unwrap_or_else(|err| {
            err!(
                "Error sending response: {}", err;
                {"handle_id": response.session_id(), "transaction": response.transaction()}
            );
        });
}

pub fn send_speaking_notification(
    sender: &JanusSender,
    session_id: SessionId,
    agent_id: Option<&AgentId>,
    is_speaking: bool,
) -> anyhow::Result<()> {
    let notification = serde_json::json!({
        "agent_id": agent_id,
        "speaking": is_speaking,
    });
    sender.send(
        session_id,
        "SpeakingNotification",
        Some(utils::serde_to_jansson(&notification)?),
        None,
    )?;
    Ok(())
}

fn notify_error(err: &SvcError) {
    if err.status_code() == StatusCode::INTERNAL_SERVER_ERROR {
        huge!("Sending error to Sentry");

        sentry::send(err.to_owned()).unwrap_or_else(|err| {
            warn!("Failed to send error to Sentry: {}", err);
        });
    }
}

fn handle_jsep(request: &Request, stream_id: StreamId) -> Result<Option<JsonValue>, SvcError> {
    let error = |status: StatusCode, err: Error| {
        SvcError::builder()
            .status(status)
            .detail(&format!("Failed to handle JSEP: {}", err))
            .build()
    };

    let negotiation_result = match &request.jsep_offer() {
        Some(jsep_offer) => Jsep::negotiate(jsep_offer, stream_id),
        None => Err(format_err!("JSEP is empty")),
    };

    match negotiation_result {
        Ok(None) => Ok(None),
        Ok(Some(answer)) => match serde_json::to_value(answer) {
            Ok(jsep) => Ok(Some(jsep)),
            Err(err) => Err(error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format_err!("Failed to serialize JSEP answer: {}", err),
            )),
        },
        Err(err) => Err(error(
            StatusCode::BAD_REQUEST,
            format_err!("Failed to negotiate JSEP: {}", err),
        )),
    }
}

pub trait Sender {
    fn send(
        &self,
        session_id: SessionId,
        transaction: &str,
        payload: Option<JanssonValue>,
        jsep_answer: Option<JanssonValue>,
    ) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    use anyhow::{bail, Result};
    use async_trait::async_trait;
    use serde_json::{json, Value as JsonValue};

    use super::{Operation, OperationResult, Request};
    use crate::{
        janus::{JanssonEncodingFlags, JanssonValue},
        message_handler::{send_response, PreparedRequest},
    };
    use crate::{
        message_handler::handle_request,
        switchboard::{SessionId, StreamId},
    };

    #[derive(Clone, Debug, Deserialize)]
    struct PingRequest {}

    #[derive(Serialize)]
    struct PingResponse {
        message: String,
        session_id: SessionId,
    }

    #[async_trait]
    impl Operation for PingRequest {
        async fn call(&self, request: &Request) -> OperationResult {
            Ok(PingResponse {
                message: String::from("pong"),
                session_id: request.session_id(),
            }
            .into())
        }

        fn stream_id(&self) -> Option<StreamId> {
            None
        }

        fn method_kind(&self) -> Option<super::MethodKind> {
            None
        }
    }

    struct TestResponse {
        session_id: SessionId,
        transaction: String,
        payload: Option<JsonValue>,
        jsep_answer: Option<JsonValue>,
    }

    #[derive(Clone)]
    struct TestSender {
        tx: Arc<Mutex<mpsc::Sender<TestResponse>>>,
    }

    impl TestSender {
        fn new() -> (Self, mpsc::Receiver<TestResponse>) {
            let (tx, rx) = mpsc::channel();

            let object = Self {
                tx: Arc::new(Mutex::new(tx)),
            };

            (object, rx)
        }
    }

    impl super::Sender for TestSender {
        fn send(
            &self,
            session_id: SessionId,
            transaction: &str,
            payload: Option<JanssonValue>,
            jsep_answer: Option<JanssonValue>,
        ) -> Result<()> {
            let payload = payload.map(|json| {
                let json = json.to_libcstring(JanssonEncodingFlags::empty());
                let json = json.to_string_lossy();
                serde_json::from_str(&json).unwrap()
            });

            let jsep_answer = jsep_answer.map(|json| {
                let json = json.to_libcstring(JanssonEncodingFlags::empty());
                let json = json.to_string_lossy();
                serde_json::from_str(&json).unwrap()
            });

            self.tx
                .lock()
                .map_err(|err| anyhow!("Failed to obtain test sender lock: {}", err))?
                .send(TestResponse {
                    session_id,
                    transaction: transaction.to_owned(),
                    payload,
                    jsep_answer,
                })
                .map_err(|err| anyhow!("Failed to send test response: {}", err))
        }
    }

    #[test]
    fn handle_message() -> Result<()> {
        let (sender, rx) = TestSender::new();
        let session_id = SessionId::new(123);
        let request = PreparedRequest {
            request: Request::new(session_id, "txn"),
            operation: PingRequest {},
        };
        let response = async_std::task::block_on(handle_request(request));
        send_response(sender, response);
        let response = rx.recv_timeout(Duration::from_secs(1))?;
        assert_eq!(response.session_id, session_id);
        assert_eq!(response.transaction, "txn");
        assert!(response.jsep_answer.is_none());

        match response.payload {
            None => bail!("Missing payload"),
            Some(json) => assert_eq!(
                json,
                json!({
                    "message": "pong",
                    "session_id": session_id,
                    "status": "200",
                })
            ),
        }

        Ok(())
    }
}
