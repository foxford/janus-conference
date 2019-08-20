mod operation;
mod request;
mod response;
mod router;

use std::convert::TryFrom;
use std::ffi::CString;
use std::sync::{mpsc, Arc, Mutex};

use failure::{err_msg, Error};
use futures::lazy;
use http::StatusCode;
use janus::JanssonValue;
use serde_json::Value as JsonValue;
use svc_error::Error as SvcError;
use tokio_threadpool::ThreadPool;

use self::request::Request;
use self::response::{Payload as ResponsePayload, Response};
use self::router::Method;
use crate::janus_callbacks;
use crate::jsep::Jsep;
use crate::session::Session;
use crate::utils;

enum Message {
    Request(Request),
    Response(Response),
}

pub struct MessageHandler {
    tx: mpsc::SyncSender<Message>,
    rx: Arc<Mutex<mpsc::Receiver<Message>>>,
    thread_pool: ThreadPool,
}

impl MessageHandler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel(10);

        Self {
            tx,
            rx: Arc::new(Mutex::new(rx)),
            thread_pool: ThreadPool::new(),
        }
    }

    /// Starts message handling loop on the thread poll.
    pub fn start(&self) {
        self.thread_pool.spawn(lazy(move || {
            let app = match app!() {
                Ok(app) => app,
                Err(err) => {
                    janus_err!("[CONFERENCE] {}", err);
                    return Ok(());
                }
            };

            match app.message_handler.rx.lock() {
                // Message handling loop.
                Ok(rx) => loop {
                    match rx.recv().ok() {
                        None => (),
                        Some(Message::Request(request)) => {
                            janus_info!("[CONFERENCE] Scheduling request handling");

                            // Handle requests asynchonously.
                            app.message_handler.thread_pool.spawn(lazy(move || {
                                janus_info!("[CONFERENCE] Handling request");
                                app.message_handler.handle_request(request);
                                Ok(())
                            }));
                        }
                        Some(Message::Response(response)) => {
                            janus_info!("[CONFERENCE] Handling response");
                            app.message_handler.handle_response(response);
                        }
                    }
                },
                Err(err) => {
                    janus_err!("[CONFERENCE] Failed to acquire receiver lock: {}", err);
                    return Ok(());
                }
            };
        }));
    }

    /// Handles JSEP if needed, calls the operation and schedules its response.
    fn handle_request(&self, request: Request) {
        match request.operation().clone() {
            None => janus_err!("[CONFERENCE] Missing request operation"),
            Some(operation) => match Self::handle_jsep(&request) {
                Err(err) => self.schedule_response(request, err.into(), None),
                Ok(jsep_answer) => {
                    janus_info!("[CONFERENCE] Calling operation");
                    let session = request.session().clone();

                    let payload = match operation.call(session) {
                        Ok(payload) => JsonValue::from(payload).into(),
                        Err(err) => err.into(),
                    };

                    self.schedule_response(request, payload, jsep_answer);
                }
            },
        }
    }

    /// Serializes the response and pushes it to Janus for sending to the client.
    fn handle_response(&self, response: Response) {
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

        CString::new(response.transaction().to_owned())
            .map_err(Error::from)
            .and_then(|transaction| {
                janus_info!("[CONFERENCE] Sending response ({})", response.transaction());

                JanssonValue::try_from(response.payload()).and_then(|payload| {
                    janus_callbacks::push_event(
                        response.session(),
                        transaction.into_raw(),
                        Some(payload),
                        jsep_answer,
                    )
                    .map_err(Error::from)
                })
            })
            .unwrap_or_else(|err| janus_err!("[CONFERENCE] Error sending response: {}", err));
    }

    /// Determines the operation by router method, builds a request object and pushes it
    /// to the message handling queue.
    pub fn schedule_request(
        &self,
        session: Arc<Session>,
        transaction: &str,
        payload: &JanssonValue,
        jsep_offer: Option<JanssonValue>,
    ) -> Result<(), Error> {
        janus_info!("[CONFERENCE] Scheduling request");
        let request = Request::new(session, &transaction);

        match utils::jansson_to_serde::<Method>(payload) {
            Ok(method) => {
                janus_info!("[CONFERENCE] Pushing request to queue");
                let request = request.set_operation(method.into());

                let request = match jsep_offer {
                    None => request,
                    Some(jansson_value) => {
                        let jsep = utils::jansson_to_serde::<Jsep>(&jansson_value)?;
                        let json_value = serde_json::to_value(jsep)?;
                        request.set_jsep_offer(json_value)
                    }
                };

                self.tx
                    .send(Message::Request(request))
                    .map_err(|err| format_err!("Failed to schedule request: {}", err))
            }
            Err(err) => {
                janus_info!("[CONFERENCE] Bad request. Couldn't determine method");

                let err = SvcError::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .detail(&err.to_string())
                    .build();

                self.schedule_response(request, err.into(), None);
                Ok(())
            }
        }
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

    /// Parses SDP offer, gets the answer, sets the offer to the request's session.
    /// Returns the answer which is intended to send in the response.
    fn handle_jsep(request: &Request) -> Result<Option<JsonValue>, SvcError> {
        let error = |status: StatusCode, err: Error| {
            SvcError::builder()
                .status(status)
                .detail(&format!("Failed to handle JSEP: {}", err))
                .build()
        };

        let operation = match request.operation() {
            Some(operation) => operation,
            None => return Ok(None),
        };

        if !operation.is_handle_jsep() {
            return Ok(None);
        }

        let negotiation_result = match &request.jsep_offer() {
            Some(jsep_offer) => Jsep::negotiate(jsep_offer),
            None => Err(err_msg("JSEP is empty")),
        };

        match negotiation_result {
            Ok(None) => Ok(None),
            Ok(Some((offer, answer))) => {
                if let Err(err) = request.session().set_offer(offer) {
                    return Err(error(StatusCode::INTERNAL_SERVER_ERROR, err));
                }

                match serde_json::to_value(answer) {
                    Ok(jsep) => Ok(Some(jsep)),
                    Err(err) => {
                        return Err(error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format_err!("Failed to serialize JSEP answer: {}", err),
                        ));
                    }
                }
            }
            Err(err) => {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    format_err!("Failed to negotiate JSEP: {}", err),
                ));
            }
        }
    }
}
