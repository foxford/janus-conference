use failure;

pub type RoomId = String;

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
    Create { id: RoomId },
    #[serde(rename = "stream.read")]
    Read { id: RoomId },
}

#[derive(Debug, Fail, Serialize)]
#[fail(display = "[{}] {}: {}", ty, status, title)]
pub struct APIError {
    #[serde(rename = "type")]
    ty: String,
    title: String,
    pub status: ErrorStatus,
    detail: String,
}

impl APIError {
    pub fn new(status: ErrorStatus, detail: failure::Error, operation: OperationError) -> Self {
        Self {
            ty: operation.ty,
            title: operation.title,
            status,
            detail: detail.to_string(),
        }
    }
}

#[derive(Debug, Fail, Serialize)]
pub enum ErrorStatus {
    #[fail(display = "Internal error")]
    #[serde(rename = "500")]
    Internal,
    #[fail(display = "Bad request")]
    #[serde(rename = "400")]
    BadRequest,
    #[fail(display = "Room does not exist")]
    #[serde(rename = "404")]
    NonExistentRoom,
}

pub struct OperationError {
    ty: String,
    title: String,
}

const UNKNOWN_ERROR: &str = "unknown_error";
const UNKNOWN_ERROR_TITLE: &str = "An error occured during unknown operation";
const CREATE_ERROR: &str = "stream_create_error";
const CREATE_ERROR_TITLE: &str = "Error creating a stream";
const READ_ERROR: &str = "stream_read_error";
const READ_ERROR_TITLE: &str = "Error reading a stream";

impl OperationError {
    pub fn new(operation: &StreamOperation) -> Self {
        let (ty, title) = match operation {
            StreamOperation::Create { .. } => (CREATE_ERROR, CREATE_ERROR_TITLE),
            StreamOperation::Read { .. } => (READ_ERROR, READ_ERROR_TITLE),
        };

        Self {
            ty: ty.to_string(),
            title: title.to_string(),
        }
    }

    pub fn unknown() -> Self {
        Self {
            ty: UNKNOWN_ERROR.to_string(),
            title: UNKNOWN_ERROR_TITLE.to_string(),
        }
    }
}
