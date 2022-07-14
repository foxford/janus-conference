use std::convert::TryFrom;

use anyhow::{Context, Error, Result};
use http::StatusCode;
use janus::JanssonValue;
use serde_json::Value as JsonValue;
use svc_error::Error as SvcError;

use super::request::Request;
use crate::switchboard::SessionId;
use crate::utils;

#[derive(Debug)]
pub struct Response {
    request: Request,
    payload: Payload,
    jsep_answer: Option<JsonValue>,
}

impl Response {
    pub fn new(request: Request, payload: Payload) -> Self {
        Self {
            request,
            payload,
            jsep_answer: None,
        }
    }

    pub fn set_jsep_answer(self, jsep_answer: JsonValue) -> Self {
        Self {
            jsep_answer: Some(jsep_answer),
            ..self
        }
    }

    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    pub fn jsep_answer(&self) -> Option<&JsonValue> {
        self.jsep_answer.as_ref()
    }

    pub fn session_id(&self) -> SessionId {
        self.request.session_id()
    }

    pub fn transaction(&self) -> &str {
        self.request.transaction()
    }
}

#[derive(Serialize)]
pub struct SyncResponse {
    payload: Payload,
    jsep_answer: Option<JsonValue>,
}

impl SyncResponse {
    pub fn new(payload: Payload, jsep_answer: Option<JsonValue>) -> Self {
        Self {
            payload,
            jsep_answer,
        }
    }

    pub fn payload(&self) -> &Payload {
        &self.payload
    }
}

impl TryFrom<SyncResponse> for JanssonValue {
    type Error = Error;

    fn try_from(response: SyncResponse) -> Result<Self, Self::Error> {
        serde_json::to_value(response)
            .map_err(Error::from)
            .and_then(|ref json_value| utils::serde_to_jansson(json_value))
            .context("Failed to serialize response")
    }
}

#[derive(Debug, Serialize)]
pub struct Payload {
    #[serde(with = "crate::serde::HttpStatusCodeRef")]
    status: StatusCode,
    #[serde(flatten)]
    response: Option<JsonValue>,
    #[serde(flatten)]
    error: Option<SvcError>,
}

impl Payload {
    pub fn new(status: StatusCode) -> Self {
        Self {
            status,
            response: None,
            error: None,
        }
    }

    pub fn set_response(self, response: JsonValue) -> Self {
        Self {
            response: Some(response),
            ..self
        }
    }

    pub fn set_error(self, error: SvcError) -> Self {
        Self {
            error: Some(error),
            ..self
        }
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl From<JsonValue> for Payload {
    fn from(json_value: JsonValue) -> Self {
        Self::new(StatusCode::OK).set_response(json_value)
    }
}

impl From<SvcError> for Payload {
    fn from(err: SvcError) -> Self {
        Self::new(err.status_code()).set_error(err)
    }
}

impl TryFrom<&Payload> for JanssonValue {
    type Error = Error;

    fn try_from(payload: &Payload) -> Result<Self, Self::Error> {
        serde_json::to_value(payload)
            .map_err(Error::from)
            .and_then(|ref json_value| utils::serde_to_jansson(json_value))
            .context("Failed to serialize response")
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use janus::JanssonEncodingFlags;
    use serde_json::{json, Value as JsonValue};
    use svc_error::Error as SvcError;

    use super::*;

    #[test]
    fn serialize_ok_payload() -> Result<()> {
        let payload = Payload::from(json!({"result": "ok"}));
        let jansson_value = JanssonValue::try_from(&payload)?;
        let json_str = jansson_value.to_libcstring(JanssonEncodingFlags::empty());
        let parsed_json = serde_json::from_str::<JsonValue>(&json_str.to_string_lossy())?;
        assert_eq!(parsed_json, json!({"result": "ok", "status": "200"}));
        Ok(())
    }

    #[test]
    fn serialize_error_payload() -> Result<()> {
        let error = SvcError::builder()
            .status(StatusCode::NOT_FOUND)
            .detail("Not Found")
            .kind("some_operation_error", "Some operation error")
            .build();

        let payload = Payload::from(error);
        let jansson_value = JanssonValue::try_from(&payload)?;
        let json_str = jansson_value.to_libcstring(JanssonEncodingFlags::empty());
        let parsed_json = serde_json::from_str::<JsonValue>(&json_str.to_string_lossy())?;

        assert_eq!(
            parsed_json,
            json!({
                "detail": "Not Found",
                "status": "404",
                "title": "Some operation error",
                "type": "some_operation_error",
            })
        );

        Ok(())
    }
}
