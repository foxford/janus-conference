mod operation;
mod request;
mod response;

use std::convert::TryFrom;
use std::marker::PhantomData;
use std::sync::mpsc;

use failure::{err_msg, Error};
use futures::{executor::ThreadPool, future::lazy};
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
    thread_pool: ThreadPool,
    tx: mpsc::SyncSender<Message>,
    rx: mpsc::Receiver<Message>,
    router: PhantomData<R>,
    sender: S,
}

/// R is for Router. It's an enum that can convert into operation.
/// S is for Sender. It actually sends the response.
impl<R, S> MessageHandlingLoop<R, S>
where
    R: Router,
    S: 'static + Clone + Send + Sender,
{
    pub fn new(sender: S) -> Self {
        let (tx, rx) = mpsc::sync_channel(10);

        Self {
            thread_pool: ThreadPool::new().expect("Failed to created thread pool"),
            tx,
            rx,
            router: PhantomData,
            sender,
        }
    }

    pub fn start(&self) {
        let tx = self.tx.to_owned();
        let sender = self.sender.to_owned();
        let handler: MessageHandler<S> = MessageHandler::new(tx, sender);

        loop {
            match self.rx.recv().ok() {
                None => (),
                Some(Message::Request(operation, request)) => {
                    janus_info!("[CONFERENCE] Scheduling request handling");
                    let handler = handler.clone();

                    self.thread_pool.spawn_ok(lazy(move |_| {
                        handler.handle_request(operation, request);
                    }));
                }
                Some(Message::Response(response)) => {
                    handler.handle_response(response);
                }
            }
        }
    }

    /// Determines the operation by Router, builds a request object and pushes it
    /// to the message handling queue.
    pub fn schedule_request(
        &self,
        session_id: SessionId,
        transaction: &str,
        payload: &JanssonValue,
        jsep_offer: Option<JanssonValue>,
    ) -> Result<(), Error> {
        janus_info!("[CONFERENCE] Scheduling request");
        let request = Request::new(session_id, &transaction);

        match utils::jansson_to_serde::<R>(payload) {
            Ok(route) => {
                janus_info!("[CONFERENCE] Pushing request to queue");

                let request = match jsep_offer {
                    None => request,
                    Some(jansson_value) => {
                        let jsep = utils::jansson_to_serde::<Jsep>(&jansson_value)?;
                        let json_value = serde_json::to_value(jsep)?;
                        request.set_jsep_offer(json_value)
                    }
                };

                self.tx
                    .send(Message::Request(route.into(), request))
                    .map_err(|err| format_err!("Failed to schedule request: {}", err))
            }
            Err(err) => {
                janus_info!("[CONFERENCE] Bad request. Couldn't determine method");

                let err = SvcError::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .detail(&err.to_string())
                    .build();

                let tx = self.tx.to_owned();
                let sender = self.sender.to_owned();
                let handler: MessageHandler<S> = MessageHandler::new(tx, sender);
                handler.schedule_response(request, err.into(), None);
                Ok(())
            }
        }
    }
}

#[derive(Clone)]
struct MessageHandler<S> {
    tx: mpsc::SyncSender<Message>,
    sender: S,
}

impl<S: Sender> MessageHandler<S> {
    pub fn new(tx: mpsc::SyncSender<Message>, sender: S) -> Self {
        Self { tx, sender }
    }

    /// Handles JSEP if needed, calls the operation and schedules its response.
    fn handle_request(&self, operation: Box<dyn Operation>, request: Request) {
        janus_info!("[CONFERENCE] Handling request");

        let jsep_answer_result = match operation.is_handle_jsep() {
            true => Self::handle_jsep(&request),
            false => Ok(None),
        };

        match jsep_answer_result {
            Ok(jsep_answer) => {
                janus_info!("[CONFERENCE] Calling operation");

                let payload = match operation.call(&request) {
                    Ok(payload) => JsonValue::from(payload).into(),
                    Err(err) => {
                        self.notify_error(&err);
                        err.into()
                    }
                };

                self.schedule_response(request, payload, jsep_answer);
            }
            Err(err) => {
                self.notify_error(&err);
                self.schedule_response(request, err.into(), None)
            }
        }
    }

    /// Serializes the response and pushes it to Janus for sending to the client.
    fn handle_response(&self, response: Response) {
        janus_info!("[CONFERENCE] Handling response");

        let jsep_answer = match response.jsep_answer() {
            None => None,
            Some(json_value) => match utils::serde_to_jansson(&json_value) {
                Ok(jansson_value) => Some(jansson_value),
                Err(err) => {
                    janus_err!("[CONFERENCE] Failed to serialize JSEP answer: {}", err);
                    return;
                }
            },
        };

        janus_info!("[CONFERENCE] Sending response ({})", response.transaction());

        JanssonValue::try_from(response.payload())
            .and_then(|payload| {
                self.sender.send(
                    response.session_id(),
                    response.transaction(),
                    Some(payload),
                    jsep_answer,
                )
            })
            .unwrap_or_else(|err| janus_err!("[CONFERENCE] Error sending response: {}", err));
    }

    /// Builds a response object for the reuqest and pushes it to the message handling queue.
    fn schedule_response(
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

        janus_info!(
            "[CONFERENCE] Scheduling response ({})",
            response.transaction()
        );

        self.tx
            .send(Message::Response(response))
            .unwrap_or_else(|err| janus_err!("[CONFERENCE] Failed to schedule response: {}", err));
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
            None => Err(err_msg("JSEP is empty")),
        };

        match negotiation_result {
            Ok(None) => Ok(None),
            Ok(Some(answer)) => match serde_json::to_value(answer) {
                Ok(jsep) => Ok(Some(jsep)),
                Err(err) => {
                    return Err(error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format_err!("Failed to serialize JSEP answer: {}", err),
                    ));
                }
            },
            Err(err) => {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    format_err!("Failed to negotiate JSEP: {}", err),
                ));
            }
        }
    }

    fn notify_error(&self, err: &SvcError) {
        if err.status_code() == StatusCode::INTERNAL_SERVER_ERROR {
            janus_info!("[CONFERENCE] Sending error to Sentry");

            sentry::send(err.to_owned()).unwrap_or_else(|err| {
                janus_err!("[CONFERENCE] Failed to send error to Sentry: {}", err);
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
    ) -> Result<(), Error>;
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use serde_json::json;

    use super::MessageHandlingLoop;
    use super::Router;
    use super::{Operation, OperationResult, Request};
    use crate::failure::Error;
    use crate::janus::{JanssonDecodingFlags, JanssonEncodingFlags, JanssonValue};
    use crate::switchboard::SessionId;

    #[derive(Clone, Debug, Deserialize)]
    struct PingRequest {}

    #[derive(Serialize)]
    struct PingResponse {
        message: String,
        session_id: String,
    }

    impl Operation for PingRequest {
        fn call(&self, request: &Request) -> OperationResult {
            Ok(PingResponse {
                message: String::from("pong"),
                session_id: request.session_id().to_string(),
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
        payload: Option<JanssonValue>,
        jsep_answer: Option<JanssonValue>,
    }

    #[derive(Clone)]
    struct TestSender {
        tx: mpsc::Sender<TestResponse>,
    }

    impl TestSender {
        fn new() -> (Self, mpsc::Receiver<TestResponse>) {
            let (tx, rx) = mpsc::channel();
            (Self { tx }, rx)
        }
    }

    impl super::Sender for TestSender {
        fn send(
            &self,
            session_id: SessionId,
            transaction: &str,
            payload: Option<JanssonValue>,
            jsep_answer: Option<JanssonValue>,
        ) -> Result<(), Error> {
            self.tx
                .send(TestResponse {
                    session_id,
                    transaction: transaction.to_owned(),
                    payload,
                    jsep_answer,
                })
                .map_err(|err| format_err!("Failed to send test response: {}", err))
        }
    }

    #[test]
    fn handle_message() -> Result<(), Error> {
        let (sender, rx) = TestSender::new();
        let session_id = SessionId::new();

        thread::spawn(move || {
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
            Some(jansson_value) => {
                let json_str = jansson_value.to_libcstring(
                    JanssonEncodingFlags::JSON_COMPACT | JanssonEncodingFlags::JSON_PRESERVE_ORDER,
                );

                let expected_json = json!({
                    "message": "pong",
                    "session_id": session_id.to_string(),
                    "status": "200"
                });

                assert_eq!(json_str.to_string_lossy(), expected_json.to_string());
            }
        }

        Ok(())
    }
}
