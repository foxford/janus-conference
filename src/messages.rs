use failure;
use http::StatusCode;

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

#[derive(Serialize)]
#[serde(untagged)]
pub enum StreamResponse {
    Create { offer: JsepKind },
    Read {},
}

pub type ErrorStatus = StatusCode;

#[derive(Serialize)]
#[serde(remote = "http::StatusCode")]
struct Status(#[serde(getter = "http::StatusCode::as_u16")] u16);

#[derive(Serialize)]
pub struct Response {
    #[serde(with = "Status")]
    status: ErrorStatus,
    #[serde(flatten)]
    response: Option<StreamResponse>,
    #[serde(flatten)]
    error: Option<APIError>,
}

impl Response {
    pub fn new(response: Option<StreamResponse>, error: Option<APIError>) -> Self {
        let status = match &error {
            None => StatusCode::OK,
            Some(err) => err.status,
        };

        Self {
            status,
            response,
            error,
        }
    }
}

#[derive(Debug, Fail, Serialize)]
#[fail(display = "[{} - {}] {}: {}", ty, status, title, detail)]
pub struct APIError {
    #[serde(rename = "type")]
    ty: String,
    title: String,
    #[serde(skip)]
    pub status: ErrorStatus,
    detail: String,
}

impl APIError {
    pub fn new(
        status: StatusCode,
        detail: failure::Error,
        operation: Option<&StreamOperation>,
    ) -> Self {
        let operation = match operation {
            None => OperationErrorDescription::unknown(status),
            Some(op) => OperationErrorDescription::new(op),
        };

        Self {
            ty: operation.ty,
            title: operation.title,
            status,
            detail: detail.to_string(),
        }
    }
}

struct OperationErrorDescription {
    ty: String,
    title: String,
}

const UNKNOWN_ERROR: &str = "about::blank";

const CREATE_ERROR: &str = "stream_create_error";
const CREATE_ERROR_TITLE: &str = "Error creating a stream";
const READ_ERROR: &str = "stream_read_error";
const READ_ERROR_TITLE: &str = "Error reading a stream";

impl OperationErrorDescription {
    fn new(operation: &StreamOperation) -> Self {
        let (ty, title) = match operation {
            StreamOperation::Create { .. } => (CREATE_ERROR, CREATE_ERROR_TITLE),
            StreamOperation::Read { .. } => (READ_ERROR, READ_ERROR_TITLE),
        };

        Self {
            ty: ty.to_string(),
            title: title.to_string(),
        }
    }

    fn unknown(status: StatusCode) -> Self {
        Self {
            ty: UNKNOWN_ERROR.to_string(),
            title: status.canonical_reason().unwrap_or("").to_string(),
        }
    }
}
