#[macro_use]
extern crate failure;
extern crate rand;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate rumqtt;
extern crate serde_json;
extern crate svc_agent;

use std::time::Duration;

use failure::{err_msg, Error};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;

use svc_agent::mqtt::compat::IntoEnvelope;
use svc_agent::mqtt::{
    Agent, AgentBuilder, AgentConfig, ConnectionMode, Notification, OutgoingRequest,
    OutgoingRequestProperties, QoS,
};
use svc_agent::{AccountId, AgentId, Subscription};

const MQTT_BROKER_URL: &str = "localhost:1883";
const AGENT_VERSION: &str = "v1.mqtt3";
const AGENT_ID_LABEL: &str = "alpha";
const JANUS_ACCOUNT_LABEL: &str = "janus-gateway";
const CONFERENCE_ACCOUNT_LABEL: &str = "conference";
const AUDIENCE: &str = "example.org";
const PLUGIN: &str = "janus.plugin.conference";
const RESPONSE_TIMEOUT: u64 = 5;
const RESPONSE_SKIP_MAX: usize = 10;
const IGNORE: &str = "ignore";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionId(u64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HandleId(u64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transaction(String);

pub struct JanusClient {
    agent: Agent,
    receiver: rumqtt::Receiver<rumqtt::Notification>,
    janus_agent_id: AgentId,
    session_id: Option<SessionId>,
    handle_id: Option<HandleId>,
}

impl JanusClient {
    /// Initializes the client.
    /// Connects to the broker, subscribes to responses topic.
    /// Then obtains session id and handle id for `PLUGIN`.
    /// Returns the client that is set up for sending messages to the plugin handle.
    pub fn new() -> Result<Self, Error> {
        let agent_config: AgentConfig = serde_json::from_value(json!({
            "uri": MQTT_BROKER_URL,
            "clean_session": true,
        }))?;

        let account_id = AccountId::new(CONFERENCE_ACCOUNT_LABEL, AUDIENCE);
        let agent_id = AgentId::new(AGENT_ID_LABEL, account_id);

        let (mut agent, receiver) = AgentBuilder::new(agent_id)
            .version(AGENT_VERSION)
            .mode(ConnectionMode::Service)
            .start(&agent_config)?;

        let janus_account_id = AccountId::new(JANUS_ACCOUNT_LABEL, AUDIENCE);
        let janus_agent_id = AgentId::new(AGENT_ID_LABEL, janus_account_id);

        let subscription = Subscription::broadcast_events(&janus_agent_id, "responses");
        agent.subscribe(&subscription, QoS::AtLeastOnce, None)?;

        let mut janus_client = Self {
            agent,
            receiver,
            janus_agent_id: janus_agent_id.clone(),
            session_id: None,
            handle_id: None,
        };

        janus_client.session_id = Some(janus_client.init_session()?);
        janus_client.handle_id = Some(janus_client.init_handle()?);
        Ok(janus_client)
    }

    fn init_session(&mut self) -> Result<SessionId, Error> {
        let response: SessionOrHandleResponse = self.request(&json!({"janus": "create"}))?;

        if response.janus == "success" {
            Ok(SessionId(response.data.id))
        } else {
            Err(format_err!("Unsuccessful response: {}", response.janus))
        }
    }

    fn init_handle(&mut self) -> Result<HandleId, Error> {
        let session_id = self.session_id()?;

        let response: SessionOrHandleResponse = self.request(&json!({
            "janus": "attach",
            "session_id": session_id,
            "plugin": PLUGIN,
        }))?;

        if response.janus == "success" {
            Ok(HandleId(response.data.id))
        } else {
            Err(format_err!("Unsuccessful response: {}", response.janus))
        }
    }

    /// Returns session id if present.
    pub fn session_id(&self) -> Result<SessionId, Error> {
        self.session_id
            .clone()
            .ok_or_else(|| err_msg("Session is not initialized"))
    }

    /// Returns handle id for the `PLUGIN` if present.
    pub fn handle_id(&self) -> Result<HandleId, Error> {
        self.handle_id
            .clone()
            .ok_or_else(|| err_msg("Handle is not initialized"))
    }

    /// Publish a message to Janus.
    pub fn publish<T: Serialize>(&mut self, payload: &T) -> Result<(), Error> {
        let outgoing_request = OutgoingRequest::unicast(
            payload,
            OutgoingRequestProperties::new(IGNORE, IGNORE, IGNORE),
            &self.janus_agent_id,
        );

        self.agent
            .publish(&outgoing_request.into_envelope()?)
            .map_err(|err| format_err!("Failed to publish: {}", err))
    }

    /// Publish a message to Janus and wait for response on it.
    /// It adds `transaction` field to the `payload` with random number to match the response.
    /// Returns the response deserialized to `R` type.
    pub fn request<T, R>(&mut self, payload: &T) -> Result<R, Error>
    where
        T: Serialize,
        for<'de> R: Deserialize<'de>,
    {
        let mut payload = serde_json::to_value(payload)?;

        let mut rng = rand::thread_rng();
        let transaction = Transaction(rng.gen::<u64>().to_string());

        payload
            .as_object_mut()
            .ok_or_else(|| err_msg("Payload is not a JSON object"))?
            .insert(String::from("transaction"), json!(transaction));

        self.publish(&payload)?;
        self.wait_for_response(&transaction, Duration::from_secs(RESPONSE_TIMEOUT))
    }

    /// Wait for response for the given `transaction` and deserialize it to `R` type.
    /// Skips intermediate messages that are unrelated to the `transaction`.
    /// Returns deserialized response on success.
    /// Returns error on timeout or intermediate messagees limit excess – `RESPONSE_SKIP_MAX`.
    pub fn wait_for_response<R>(
        &self,
        transaction: &Transaction,
        timeout: Duration,
    ) -> Result<R, Error>
    where
        for<'de> R: Deserialize<'de>,
    {
        let mut skip_counter: usize = 0;

        loop {
            if skip_counter == RESPONSE_SKIP_MAX {
                let err = format_err!(
                    "Skipped {} messages, but no one is a response on {:?}",
                    RESPONSE_SKIP_MAX,
                    transaction,
                );

                return Err(err);
            }

            match self.receiver.recv_timeout(timeout) {
                Ok(Notification::Publish(publish)) => {
                    let payload = Self::parse_response(&publish.payload.as_slice())?;

                    if Self::is_expected_transaction(&payload, transaction) {
                        return serde_json::from_value::<R>(payload.to_owned())
                            .map_err(|err| format_err!("Failed to typify message: {}", err));
                    } else {
                        skip_counter += 1;
                    }
                }
                Ok(_) => (),
                Err(_) => {
                    let err =
                        format_err!("Timed out waiting for the response on {:?}", transaction);

                    return Err(err);
                }
            }
        }
    }

    fn parse_response(payload: &[u8]) -> Result<serde_json::Value, Error> {
        let json = serde_json::from_slice::<serde_json::Value>(payload)?;

        let payload_str = json
            .get("payload")
            .ok_or_else(|| err_msg("Missing payload in response"))?
            .as_str()
            .ok_or_else(|| err_msg("Response payload is not a string"))?;

        serde_json::from_str::<serde_json::Value>(payload_str)
            .map_err(|err| format_err!("Failed to parse message: {}", err))
    }

    fn is_expected_transaction(payload: &serde_json::Value, transaction: &Transaction) -> bool {
        payload
            .get("transaction")
            .and_then(|value| value.as_str())
            .map(|value| Transaction(String::from(value)))
            .filter(|value| *value == *transaction)
            .is_some()
    }

    /// Convenience wrapper around `request` to send send a message to the plugin handle.
    pub fn request_message<T, R>(&mut self, body: T) -> Result<R, Error>
    where
        T: Serialize,
        for<'de> R: Deserialize<'de>,
    {
        let session_id = self.session_id()?;
        let handle_id = self.handle_id()?;

        self.request(&json!({
          "janus": "message",
          "session_id": session_id,
          "handle_id": handle_id,
          "body": body,
        }))
    }

    fn graceful_disconnect(&mut self) -> Result<(), Error> {
        if let Some(session_id) = self.session_id.clone() {
            if let Some(handle_id) = self.handle_id.clone() {
                let _response: IgnoredResponse = self.request(&json!({
                    "janus": "detach",
                    "session_id": session_id,
                    "handle_id": handle_id,
                }))?;
            }

            let _response: IgnoredResponse = self.request(&json!({
                "janus": "destroy",
                "session_id": session_id,
            }))?;
        }

        Ok(())
    }
}

impl Drop for JanusClient {
    fn drop(&mut self) {
        if let Err(err) = self.graceful_disconnect() {
            eprintln!("Failed to disconnect MQTT client: {}", err);
        }
    }
}

// JSON responses
#[derive(Deserialize)]
struct SessionOrHandleResponse {
    janus: String,
    data: SessionOrHandleResponseData,
}

#[derive(Deserialize)]
struct SessionOrHandleResponseData {
    id: u64,
}

#[derive(Deserialize)]
struct IgnoredResponse;
