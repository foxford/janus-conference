mod operation;
mod request;
mod response;

use std::convert::TryFrom;
use std::marker::PhantomData;

use anyhow::{format_err, Error};
use async_std::task;
use http::StatusCode;
use janus::JanssonValue;
use serde_json::Value as JsonValue;
use svc_error::{extension::sentry, Error as SvcError};

use self::response::{Payload as ResponsePayload, Response};
use crate::jsep::Jsep;
use crate::switchboard::SessionId;
use crate::utils;

pub use self::operation::{Operation, Result as OperationResult};
pub use self::request::Request;

pub trait Router: serde::de::DeserializeOwned + Into<Box<dyn Operation>> {}

enum Message {
    Request(Box<dyn Operation>, Request),
    Response(Response),
}

pub struct MessageHandlingLoop<R, S> {
    tx: async_std::channel::Sender<Message>,
    rx: async_std::channel::Receiver<Message>,
    router: PhantomData<R>,
    sender: S,
}

/// R is for Router. It's an enum that can convert into operation.
/// S is for Sender. It actually sends the response.
impl<R, S> MessageHandlingLoop<R, S>
where
    R: Router,
    S: 'static + Clone + Send + Sync + Sender,
{
    pub fn new(sender: S) -> Self {
        let (tx, rx) = async_std::channel::bounded(1000);

        Self {
            tx,
            rx,
            router: PhantomData,
            sender,
        }
    }

    pub fn start(&self) {
        let tx = self.tx.to_owned();
        let rx = &self.rx;
        let sender = self.sender.to_owned();
        let handler: MessageHandler<S> = MessageHandler::new(tx, sender);

        task::block_on(async {
            loop {
                match rx.recv().await {
                    Ok(Message::Request(operation, request)) => {
                        verb!("Scheduling request handling");
                        let handler = handler.clone();

                        task::spawn(async move {
                            handler.handle_request(operation, request).await;
                        });
                    }
                    Ok(Message::Response(response)) => {
                        handler.handle_response(response);
                    }
                    Err(err) => {
                        err!("Error reading a message from channel: {}", err);
                    }
                }
            }
        });
    }

    /// Determines the operation by Router, builds a request object and pushes it
    /// to the message handling queue.
    pub fn schedule_request(
        &self,
        session_id: SessionId,
        transaction: &str,
        payload: &JanssonValue,
        jsep_offer: Option<JanssonValue>,
    ) -> anyhow::Result<()> {
        huge!("Scheduling request"; {"handle_id": session_id, "transaction": transaction});

        let request = Request::new(session_id, &transaction);

        match utils::jansson_to_serde::<R>(payload) {
            Ok(route) => {
                huge!(
                    "Pushing request to queue";
                    {"handle_id": session_id, "transaction": transaction}
                );

                let request = match jsep_offer {
                    None => request,
                    Some(jansson_value) => {
                        let jsep = utils::jansson_to_serde::<Jsep>(&jansson_value)?;
                        let json_value = serde_json::to_value(jsep)?;
                        request.set_jsep_offer(json_value)
                    }
                };

                let tx = self.tx.clone();
                let message = Message::Request(route.into(), request);
                let transaction = transaction.to_owned();

                task::spawn(async move {
                    if let Err(err) = tx.send(message).await {
                        err!(
                            "Failed to schedule request: {}", err;
                            {"handle_id": session_id, "transaction": transaction}
                        );
                    }
                });

                Ok(())
            }
            Err(err) => {
                verb!(
                    "Bad request. Wrong method or payload.";
                    {"handle_id": session_id, "transaction": transaction}
                );

                let err = SvcError::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .detail(&err.to_string())
                    .build();

                let tx = self.tx.to_owned();
                let sender = self.sender.to_owned();
                let handler: MessageHandler<S> = MessageHandler::new(tx, sender);

                task::spawn(
                    async move { handler.schedule_response(request, err.into(), None).await },
                );

                Ok(())
            }
        }
    }
}

#[derive(Clone)]
struct MessageHandler<S> {
    tx: async_std::channel::Sender<Message>,
    sender: S,
}

impl<S: Sender> MessageHandler<S> {
    pub fn new(tx: async_std::channel::Sender<Message>, sender: S) -> Self {
        Self { tx, sender }
    }

    /// Handles JSEP if needed, calls the operation and schedules its response.
    async fn handle_request(&self, operation: Box<dyn Operation>, request: Request) {
        huge!("Handling request"; {"transaction": request.transaction()});

        let jsep_answer_result = match operation.is_handle_jsep() {
            true => Self::handle_jsep(&request),
            false => Ok(None),
        };

        match jsep_answer_result {
            Ok(jsep_answer) => {
                huge!("Calling operation"; {"transaction": request.transaction()});

                let payload = match operation.call(&request).await {
                    Ok(payload) => JsonValue::from(payload).into(),
                    Err(err) => {
                        self.notify_error(&err);
                        err.into()
                    }
                };

                self.schedule_response(request, payload, jsep_answer).await;
            }
            Err(err) => {
                self.notify_error(&err);
                self.schedule_response(request, err.into(), None).await
            }
        }
    }

    /// Serializes the response and pushes it to Janus for sending to the client.
    fn handle_response(&self, response: Response) {
        huge!(
            "Handling response";
            {"handle_id": response.session_id(), "transaction": response.transaction()}
        );

        let jsep_answer = match response.jsep_answer() {
            None => None,
            Some(json_value) => match utils::serde_to_jansson(&json_value) {
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
                self.sender.send(
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

    /// Builds a response object for the request and pushes it to the message handling queue.
    async fn schedule_response(
        &self,
        request: Request,
        payload: ResponsePayload,
        jsep_answer: Option<JsonValue>,
    ) {
        let response = Response::new(request, payload);

        let response = match jsep_answer {
            None => response,
            Some(jsep_answer) => response.set_jsep_answer(jsep_answer),
        };

        let session_id = response.session_id().to_owned();
        let transaction = response.transaction().to_owned();
        huge!("Scheduling response"; {"handle_id": session_id, "transaction": transaction});

        self.tx
            .send(Message::Response(response))
            .await
            .unwrap_or_else(move |err| {
                err!(
                    "Failed to schedule response: {}", err;
                    {"handle_id": session_id, "transaction": transaction}
                );
            });
    }

    /// Parses SDP offer, returns the answer which is intended to send in the response.
    fn handle_jsep(request: &Request) -> Result<Option<JsonValue>, SvcError> {
        let error = |status: StatusCode, err: Error| {
            SvcError::builder()
                .status(status)
                .detail(&format!("Failed to handle JSEP: {}", err))
                .build()
        };

        let negotiation_result = match &request.jsep_offer() {
            Some(jsep_offer) => Jsep::negotiate(jsep_offer),
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

    fn notify_error(&self, err: &SvcError) {
        if err.status_code() == StatusCode::INTERNAL_SERVER_ERROR {
            huge!("Sending error to Sentry");

            sentry::send(err.to_owned()).unwrap_or_else(|err| {
                warn!("Failed to send error to Sentry: {}", err);
            });
        }
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
    use async_std::task;
    use async_trait::async_trait;
    use serde_json::{json, Value as JsonValue};

    use super::MessageHandlingLoop;
    use super::Router;
    use super::{Operation, OperationResult, Request};
    use crate::janus::{JanssonDecodingFlags, JanssonEncodingFlags, JanssonValue};
    use crate::switchboard::SessionId;

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

        fn is_handle_jsep(&self) -> bool {
            false
        }
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(tag = "method")]
    enum TestRouter {
        #[serde(rename = "ping")]
        Ping(PingRequest),
    }

    impl Into<Box<dyn Operation>> for TestRouter {
        fn into(self) -> Box<dyn Operation> {
            match self {
                TestRouter::Ping(op) => Box::new(op),
            }
        }
    }

    impl Router for TestRouter {}

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

        task::spawn(async move {
            let message_handling_loop: MessageHandlingLoop<TestRouter, TestSender> =
                MessageHandlingLoop::new(sender);

            let json =
                JanssonValue::from_str("{\"method\": \"ping\"}", JanssonDecodingFlags::empty())
                    .unwrap();

            message_handling_loop
                .schedule_request(session_id, "txn", &json, None)
                .unwrap();

            message_handling_loop.start();
        });

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
