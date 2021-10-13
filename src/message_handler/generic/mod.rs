mod operation;
mod request;
mod response;

use std::convert::TryFrom;

use anyhow::{format_err, Context, Result};
use http::StatusCode;
use janus_plugin::JanssonValue;
use serde_json::Value as JsonValue;

use self::response::Response;
use crate::utils;
use crate::{jsep::Jsep, message_handler::Method};
use crate::{
    message_handler::generic::response::Payload,
    switchboard::{AgentId, SessionId, StreamId},
};

pub use self::request::Request;

use super::JanusSender;

pub struct PreparedRequest {
    request: Request,
    operation: Method,
}
pub fn prepare_request(
    session_id: SessionId,
    transaction: &str,
    payload: &JanssonValue,
    jsep_offer: Option<JanssonValue>,
) -> anyhow::Result<PreparedRequest> {
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

pub fn handle_request(PreparedRequest { request, operation }: PreparedRequest) -> Response {
    let handle_request = || -> Result<_, anyhow::Error> {
        let jsep = handle_jsep(&request, operation.stream_id())?;
        match operation {
            Method::StreamCreate(x) => x.stream_create(&request).context("StreamCreate")?,
            Method::StreamRead(x) => x.stream_read(&request).context("StreamRead")?,
        };
        Ok(jsep)
    };
    match handle_request() {
        Ok(jsep) => Response::new(request, Payload::new(StatusCode::OK)).set_jsep_answer(jsep),
        Err(err) => {
            let error = svc_error::Error::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .detail(&format!("Error occured: {:?}", err))
                .build();
            Response::new(request, error.into())
        }
    }
}

pub fn send_response(sender: impl Sender, response: Response) {
    huge!(
        "Handling response";
        {"handle_id": response.session_id(), "transaction": response.transaction()}
    );
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

#[allow(clippy::ptr_arg)]
pub fn send_speaking_notification(
    sender: &JanusSender,
    session_id: SessionId,
    agent_id: &AgentId,
    is_speaking: bool,
) -> anyhow::Result<()> {
    let notification = serde_json::json!({
        "agent_id": agent_id,
        "speaking": is_speaking
    });
    let response = Some(JanssonValue::try_from(
        &Payload::new(StatusCode::OK).set_response(notification),
    )?);
    sender.send(session_id, "speaking", response, None)?;
    Ok(())
}

fn handle_jsep(request: &Request, stream_id: StreamId) -> Result<JsonValue> {
    let negotiation_result = match &request.jsep_offer() {
        Some(jsep_offer) => Jsep::negotiate(jsep_offer, stream_id),
        None => Err(format_err!("JSEP is empty")),
    };

    match negotiation_result {
        Ok(answer) => match serde_json::to_value(answer) {
            Ok(jsep) => Ok(jsep),
            Err(err) => Err(format_err!("Failed to serialize JSEP answer: {}", err)),
        },
        Err(err) => Err(format_err!("Failed to negotiate JSEP: {}", err)),
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

// #[cfg(test)]
// mod tests {
//     use std::sync::{mpsc, Arc, Mutex};
//     use std::time::Duration;

//     use anyhow::{bail, Result};
//     use async_trait::async_trait;
//     use serde_json::{json, Value as JsonValue};

//     use super::{Operation, OperationResult, Request};
//     use crate::{
//         janus::{JanssonEncodingFlags, JanssonValue},
//         message_handler::{send_response, PreparedRequest},
//     };
//     use crate::{
//         message_handler::handle_request,
//         switchboard::{SessionId, StreamId},
//     };

//     #[derive(Clone, Debug, Deserialize)]
//     struct PingRequest {}

//     #[derive(Serialize)]
//     struct PingResponse {
//         message: String,
//         session_id: SessionId,
//     }

//     #[async_trait]
//     impl Operation for PingRequest {
//         async fn call(&self, request: &Request) -> OperationResult {
//             Ok(PingResponse {
//                 message: String::from("pong"),
//                 session_id: request.session_id(),
//             }
//             .into())
//         }

//         fn stream_id(&self) -> Option<StreamId> {
//             None
//         }

//         fn method_kind(&self) -> Option<super::MethodKind> {
//             None
//         }
//     }

//     struct TestResponse {
//         session_id: SessionId,
//         transaction: String,
//         payload: Option<JsonValue>,
//         jsep_answer: Option<JsonValue>,
//     }

//     #[derive(Clone)]
//     struct TestSender {
//         tx: Arc<Mutex<mpsc::Sender<TestResponse>>>,
//     }

//     impl TestSender {
//         fn new() -> (Self, mpsc::Receiver<TestResponse>) {
//             let (tx, rx) = mpsc::channel();

//             let object = Self {
//                 tx: Arc::new(Mutex::new(tx)),
//             };

//             (object, rx)
//         }
//     }

//     impl super::Sender for TestSender {
//         fn send(
//             &self,
//             session_id: SessionId,
//             transaction: &str,
//             payload: Option<JanssonValue>,
//             jsep_answer: Option<JanssonValue>,
//         ) -> Result<()> {
//             let payload = payload.map(|json| {
//                 let json = json.to_libcstring(JanssonEncodingFlags::empty());
//                 let json = json.to_string_lossy();
//                 serde_json::from_str(&json).unwrap()
//             });

//             let jsep_answer = jsep_answer.map(|json| {
//                 let json = json.to_libcstring(JanssonEncodingFlags::empty());
//                 let json = json.to_string_lossy();
//                 serde_json::from_str(&json).unwrap()
//             });

//             self.tx
//                 .lock()
//                 .map_err(|err| anyhow!("Failed to obtain test sender lock: {}", err))?
//                 .send(TestResponse {
//                     session_id,
//                     transaction: transaction.to_owned(),
//                     payload,
//                     jsep_answer,
//                 })
//                 .map_err(|err| anyhow!("Failed to send test response: {}", err))
//         }
//     }

//     #[test]
//     fn handle_message() -> Result<()> {
//         let (sender, rx) = TestSender::new();
//         let session_id = SessionId::new(123);
//         let request = PreparedRequest {
//             request: Request::new(session_id, "txn"),
//             operation: PingRequest {},
//         };
//         let response = async_std::task::block_on(handle_request(request));
//         send_response(sender, response);
//         let response = rx.recv_timeout(Duration::from_secs(1))?;
//         assert_eq!(response.session_id, session_id);
//         assert_eq!(response.transaction, "txn");
//         assert!(response.jsep_answer.is_none());

//         match response.payload {
//             None => bail!("Missing payload"),
//             Some(json) => assert_eq!(
//                 json,
//                 json!({
//                     "message": "pong",
//                     "session_id": session_id,
//                     "status": "200",
//                 })
//             ),
//         }

//         Ok(())
//     }
// }
