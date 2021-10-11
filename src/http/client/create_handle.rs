use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct CreateHandleRequest {
    pub session_id: u64,
}

#[derive(Debug, Deserialize)]
pub struct CreateHandleResponse {
    pub id: u64,
}
