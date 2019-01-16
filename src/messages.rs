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
#[fail(display = "{}: {}", status, title)]
pub struct APIError {
    pub status: ErrorStatus,
    title: String,
}

#[derive(Debug, Fail, Serialize)]
pub enum ErrorStatus {
    #[fail(display = "Internal error")]
    Internal,
    #[fail(display = "Bad request ({})", reason)]
    BadRequest { reason: String },
    #[fail(display = "Room {} does not exist", id)]
    NonExistentRoom { id: RoomId },
}

pub trait ToAPIError {
    fn to_internal(&self) -> APIError;
    fn to_bad_request(&self, reason: &'static str) -> APIError;
    fn to_non_existent_room(&self, id: RoomId) -> APIError;
}

impl ToAPIError for failure::Error {
    fn to_internal(&self) -> APIError {
        APIError {
            status: ErrorStatus::Internal,
            title: self.to_string(),
        }
    }

    fn to_bad_request(&self, title: &'static str) -> APIError {
        APIError {
            status: ErrorStatus::BadRequest {
                reason: self.to_string(),
            },
            title: String::from(title),
        }
    }

    fn to_non_existent_room(&self, id: RoomId) -> APIError {
        APIError {
            status: ErrorStatus::NonExistentRoom { id },
            title: self.to_string(),
        }
    }
}
