use serde_json::Value as JsonValue;

#[derive(Debug)]
pub struct Request<C> {
    context: C,
    transaction: String,
    jsep_offer: Option<JsonValue>,
}

impl<C> Request<C> {
    pub fn new(context: C, transaction: &str) -> Self {
        Self {
            context,
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

    pub fn context(&self) -> &C {
        &self.context
    }

    pub fn transaction(&self) -> &str {
        &self.transaction
    }

    pub fn jsep_offer(&self) -> Option<&JsonValue> {
        self.jsep_offer.as_ref()
    }
}
