use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct CreateSessionResponse {
    pub id: u64,
}
