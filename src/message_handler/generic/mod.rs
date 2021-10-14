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
            err!("Message handler error occured: {:?}", err);
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
