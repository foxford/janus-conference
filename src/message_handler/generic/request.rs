use serde_json::Value as JsonValue;

use crate::switchboard::SessionId;

#[derive(Debug)]
pub struct Request {
    session_id: SessionId,
    transaction: String,
    jsep_offer: Option<JsonValue>,
}

impl Request {
    pub fn new(session_id: SessionId, transaction: &str) -> Self {
        Self {
            session_id,
            transaction: transaction.to_owned(),
            jsep_offer: None,
        }
    }

    pub fn set_jsep_offer(self, jsep_offer: JsonValue) -> Self {
        Self {
            jsep_offer: Some(jsep_offer),
            ..self
        }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn transaction(&self) -> &str {
        &self.transaction
    }

    pub fn jsep_offer(&self) -> Option<&JsonValue> {
        self.jsep_offer.as_ref()
    }
}
