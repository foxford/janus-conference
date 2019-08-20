use std::sync::Arc;

use serde_json::Value as JsonValue;

use super::operation::Operation;
use crate::session::Session;

#[derive(Debug)]
pub struct Request {
    session: Arc<Session>,
    transaction: String,
    operation: Option<Box<dyn Operation>>,
    jsep_offer: Option<JsonValue>,
}

impl Request {
    pub fn new(session: Arc<Session>, transaction: &str) -> Self {
        Self {
            session,
            transaction: transaction.to_owned(),
            operation: None,
            jsep_offer: None,
        }
    }

    pub fn set_operation(self, operation: Box<dyn Operation>) -> Self {
        Self {
            operation: Some(operation),
            ..self
        }
    }

    pub fn set_jsep_offer(self, jsep_offer: JsonValue) -> Self {
        Self {
            jsep_offer: Some(jsep_offer),
            ..self
        }
    }

    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }

    pub fn transaction(&self) -> &str {
        &self.transaction
    }

    pub fn operation(&self) -> Option<&Box<dyn Operation>> {
        self.operation.as_ref()
    }

    pub fn jsep_offer(&self) -> Option<&JsonValue> {
        self.jsep_offer.as_ref()
    }
}
