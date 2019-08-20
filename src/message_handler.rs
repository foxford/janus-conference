mod operation;
mod router;

use std::sync::{mpsc, Arc};

use failure::{err_msg, Error};
use http::StatusCode;
use janus::{JanssonDecodingFlags, JanssonEncodingFlags, JanssonValue};
use serde_json::Value as JsonValue;
use svc_error::Error as SvcError;

use self::operation::OperationError;
use self::router::Method;
use crate::jsep::Jsep;
use crate::session::Session;
use crate::utils;

#[derive(Clone, Debug)]
pub struct Message {
    session: Arc<Session>,
    transaction: String,
    method: Option<Method>,
    jsep: Option<String>,
}

impl Message {
    fn new(session: Arc<Session>, transaction: &str) -> Self {
        Self {
            session,
            transaction: transaction.to_owned(),
            method: None,
            jsep: None,
        }
    }

    fn set_method(self, method: Method) -> Self {
        Self {
            method: Some(method),
            ..self
        }
    }

    fn set_jsep(self, jsep: JanssonValue) -> Result<Self, Error> {
        // TODO: suboptimal serialization to String for making Message thread safe.
        let jsep = match jsep.to_libcstring(JanssonEncodingFlags::empty()).to_str() {
            Ok(jsep) => Some(jsep.to_owned()),
            Err(err) => bail!("Failed to serialize JSEP: {}", err),
        };

        Ok(Self { jsep, ..self })
    }

    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }

    pub fn transaction(&self) -> &str {
        &self.transaction
    }
}

#[derive(Serialize)]
struct Response {
    #[serde(with = "crate::serde::HttpStatusCodeRef")]
    status: StatusCode,
    #[serde(flatten)]
    response: Option<JsonValue>,
    #[serde(flatten)]
    error: Option<SvcError>,
}

impl Response {
    pub fn new(response: Option<JsonValue>, error: Option<SvcError>) -> Self {
        let status = match &error {
            None => StatusCode::OK,
            Some(err) => err.status_code(),
        };

        Self {
            status,
            response,
            error,
        }
    }
}

enum Event {
    Request(Message),
    Response {
        request_msg: Message,
        response: Option<JanssonValue>,
        jsep: Option<JanssonValue>,
    },
}

#[derive(Debug)]
pub struct MessageHandler {
    tx: mpsc::SyncSender<Event>,
    rx: mpsc::Receiver<Event>,
}

impl MessageHandler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel(10);
        Self { tx, rx }
    }

    pub fn schedule_handling(
        &self,
        session: Arc<Session>,
        transaction: &str,
        json: &JanssonValue,
        jsep: Option<JanssonValue>,
    ) -> Result<(), Error> {
        match utils::jansson_to_serde::<Method>(json) {
            Ok(method) => {
                let msg = Message::new(session, &transaction).set_method(method);

                let msg = match jsep {
                    None => msg,
                    Some(jsep) => msg.set_jsep(jsep)?,
                };

                self.tx
                    .send(Event::Request(msg))
                    .map_err(|err| format_err!("Failed to queue request: {}", err))
            }
            Err(err) => {
                let msg = Message::new(session, &transaction);
                Self::respond(&self.tx, &msg, Err(err.into()), &None);
                Ok(())
            }
        }
    }

    pub fn run<F>(&self, response_callback: F)
    where
        F: Fn(&Message, Option<JanssonValue>, Option<JanssonValue>),
    {
        for item in self.rx.iter() {
            match item {
                Event::Request(msg) => {
                    match msg.method.clone() {
                        Some(method) => {
                            janus_info!("[CONFERENCE] Handling request");

                            Self::handle_jsep(&msg)
                                .and_then(|jsep| {
                                    let tx = self.tx.clone();
                                    let msg_clone = msg.clone();
                                    let session = msg.session.clone();

                                    let respond = move |result| {
                                        Self::respond(&tx, &msg_clone, result, &jsep)
                                    };

                                    method.operation().call(session, Box::new(respond))
                                })
                                .unwrap_or_else(|err| Self::respond(&self.tx, &msg, Err(err), &None));
                        }
                        None => {
                            janus_err!("[CONFERENCE] Missing method in request");
                            let err = err_msg("Missing method in request");
                            Self::respond(&self.tx, &msg, Err(err.into()), &None);
                        }
                    }
                }
                Event::Response {
                    request_msg,
                    response,
                    jsep,
                } => {
                    response_callback(&request_msg, response, jsep);
                }
            }
        }
    }

    fn respond(
        tx: &mpsc::SyncSender<Event>,
        msg: &Message,
        result: Result<JsonValue, OperationError>,
        jsep: &Option<JsonValue>,
    ) {
        let (response, jsep) = match result {
            Ok(response) => match Self::build_ok_response(response) {
                Ok(response) => (response, jsep),
                Err(err) => (Self::build_error_response(msg, err.into()), &None),
            },
            Err(err) => {
                janus_err!("Error processing message: {}", err);
                (Self::build_error_response(msg, err), &None)
            }
        };

        let (response, jsep) = match jsep {
            None => (response, None),
            Some(value) => match utils::serde_to_jansson(&value) {
                Ok(jansson_value) => (response, Some(jansson_value)),
                Err(err) => {
                    janus_err!("Failed to serialize JSEP: {}", err);
                    (Self::build_error_response(msg, err.into()), None)
                }
            },
        };

        let response_event = Event::Response {
            request_msg: msg.to_owned(),
            response: Some(response),
            jsep,
        };

        janus_info!("[CONFERENCE] Scheduling response ({})", msg.transaction);
        tx.send(response_event).ok();
    }

    fn build_ok_response(response: JsonValue) -> Result<JanssonValue, Error> {
        let response = serde_json::to_value(Response::new(Some(response), None))?;
        utils::serde_to_jansson(&response)
    }

    fn build_error_response(msg: &Message, err: OperationError) -> JanssonValue {
        let builder = SvcError::builder()
            .status(err.status())
            .detail(&err.cause().to_string());

        let builder = match &msg.method {
            None => builder,
            Some(method) => {
                let (kind, title) = method.operation().error_kind();
                builder.kind(kind, title)
            }
        };

        let err = builder.build();

        serde_json::to_value(Response::new(None, Some(err)))
            .map_err(|_| err_msg("Error dumping response to JSON"))
            .and_then(|response| utils::serde_to_jansson(&response))
            .unwrap_or_else(|err| {
                let err = format!("Error serializing other error: {}", &err);

                JanssonValue::from_str(&err, JanssonDecodingFlags::empty())
                    .unwrap_or_else(|_| Self::json_serialization_fallback_error())
            })
    }

    // TODO: make it `const fn` in future Rust versions. Now it fails with:
    // `error: trait bounds other than `Sized` on const fn parameters are unstable`
    fn json_serialization_fallback_error() -> JanssonValue {
        // `unwrap` is ok here because we're converting a constant string.
        JanssonValue::from_str("JSON serialization error", JanssonDecodingFlags::empty()).unwrap()
    }

    fn handle_jsep(msg: &Message) -> Result<Option<JsonValue>, OperationError> {
        let should_handle_jsep = match &msg.method {
            None => false,
            Some(method) => method.operation().is_handle_jsep(),
        };

        if !should_handle_jsep {
            return Ok(None);
        }

        let result = match &msg.jsep {
            Some(jsep) => Jsep::negotiate(&jsep),
            None => Err(err_msg("JSEP is empty")),
        };

        let result = match result {
            Err(err) => Err(err),
            Ok(None) => Ok(None),
            Ok(Some((offer, answer))) => msg.session.set_offer(offer).and_then(|_| {
                serde_json::to_value(answer)
                    .map(|jsep| Some(jsep))
                    .map_err(|err| format_err!("Failed to serialize JSEP answer: {}", err))
            }),
        };

        result.map_err(|err| {
            OperationError::new(
                StatusCode::BAD_REQUEST,
                format_err!("Failed to deserialize JSEP: {}", err),
            )
        })
    }
}
