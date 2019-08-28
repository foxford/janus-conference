use std::convert::TryFrom;

use failure::Error;
use http::StatusCode;
use janus::JanssonValue;
use serde_json::Value as JsonValue;
use svc_error::Error as SvcError;

use super::request::Request;
use crate::utils;

#[derive(Debug)]
pub struct Response<C> {
    request: Request<C>,
    payload: Payload,
    jsep_answer: Option<JsonValue>,
}

impl<C> Response<C> {
    pub fn new(request: Request<C>, payload: Payload) -> Self {
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

    pub fn context(&self) -> &C {
        self.request.context()
    }

    pub fn transaction(&self) -> &str {
        self.request.transaction()
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
            .map_err(|err| format_err!("Failed to serialize response: {}", err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use janus::JanssonEncodingFlags;
    use serde_json::json;
    use svc_error::Error as SvcError;

    #[test]
    fn serialize_ok_payload() -> Result<(), Error> {
        let payload = Payload::from(json!({"result": "ok"}));
        let jansson_value = JanssonValue::try_from(&payload)?;
        let json_str = jansson_value.to_libcstring(JanssonEncodingFlags::empty());
        assert_eq!(
            json_str.to_string_lossy(),
            "{\"result\": \"ok\", \"status\": \"200\"}"
        );
        Ok(())
    }

    #[test]
    fn serialize_error_payload() -> Result<(), Error> {
        let error = SvcError::builder()
            .status(StatusCode::NOT_FOUND)
            .detail("Not Found")
            .kind("some_operation_error", "Some operation error")
            .build();

        let payload = Payload::from(error);
        let jansson_value = JanssonValue::try_from(&payload)?;
        let json_str = jansson_value.to_libcstring(JanssonEncodingFlags::empty());

        assert_eq!(
            json_str.to_string_lossy(),
            "{\"detail\": \"Not Found\", \"status\": \"404\", \"title\": \"Some operation error\", \"type\": \"some_operation_error\"}"
        );

        Ok(())
    }

    #[derive(Serialize)]
    struct BadResponse;

    #[test]
    fn serialize_bad_response() {
        let json_value = serde_json::to_value(BadResponse {}).unwrap();
        let payload = Payload::from(json_value);

        match JanssonValue::try_from(&payload) {
            Ok(_) => panic!("Expected serialization to fail"),
            Err(err) => assert!(err.to_string().starts_with("Failed to serialize response")),
        }
    }
}
