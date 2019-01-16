use failure;

pub type StreamId = String;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum JsepKind {
    Offer { sdp: String },
    Answer { sdp: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum StreamOperation {
    #[serde(rename = "stream.create")]
    Create { id: StreamId },
    #[serde(rename = "stream.read")]
    Read { id: StreamId },
}

#[derive(Debug, Fail, Serialize)]
#[fail(display = "{}: {}", kind, detail)]
pub struct APIError {
    pub kind: ErrorKind,
    detail: String,
}

#[derive(Debug, Fail, Serialize)]
pub enum ErrorKind {
    #[fail(display = "Internal error")]
    Internal,
    #[fail(display = "Bad request ({})", reason)]
    BadRequest { reason: String },
    #[fail(display = "Stream {} does not exist", id)]
    NonExistentStream { id: StreamId },
}

pub trait ToAPIError {
    fn to_internal(&self) -> APIError;
    fn to_bad_request(&self, reason: &'static str) -> APIError;
    fn to_non_existent_stream(&self, id: StreamId) -> APIError;
}

impl ToAPIError for failure::Error {
    fn to_internal(&self) -> APIError {
        APIError {
            kind: ErrorKind::Internal,
            detail: self.to_string(),
        }
    }

    fn to_bad_request(&self, title: &'static str) -> APIError {
        APIError {
            kind: ErrorKind::BadRequest {
                reason: self.to_string(),
            },
            detail: String::from(title),
        }
    }

    fn to_non_existent_stream(&self, id: StreamId) -> APIError {
        APIError {
            kind: ErrorKind::NonExistentStream { id },
            detail: self.to_string(),
        }
    }
}
