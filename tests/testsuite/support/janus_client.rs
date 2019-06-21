use std::collections::HashMap;
use std::sync::{mpsc, Arc, RwLock};
use std::thread;
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

use crate::support::conference_plugin_api_responses::{
    GenericResponse, HandleId, HandleResponse, SessionId, SessionResponse, Transaction,
};

const MQTT_BROKER_URL: &str = "localhost:1883";
const AGENT_VERSION: &str = "v1.mqtt3";
const AGENT_ID_LABEL: &str = "alpha";
const JANUS_ACCOUNT_LABEL: &str = "janus-gateway";
const CONFERENCE_ACCOUNT_LABEL: &str = "conference";
const AUDIENCE: &str = "example.org";
const PLUGIN: &str = "janus.plugin.conference";
const RESPONSE_TIMEOUT: u64 = 5;
const IGNORE: &str = "ignore";

#[derive(Clone)]
pub struct JanusClient {
    agent: Agent,
    janus_agent_id: AgentId,
    session_id: Option<SessionId>,
    handle_id: Option<HandleId>,
    response_senders: Arc<RwLock<HashMap<Transaction, mpsc::SyncSender<serde_json::Value>>>>,
    response_receivers: Arc<RwLock<HashMap<Transaction, mpsc::Receiver<serde_json::Value>>>>,
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
            janus_agent_id: janus_agent_id.clone(),
            session_id: None,
            handle_id: None,
            response_senders: Arc::new(RwLock::new(HashMap::new())),
            response_receivers: Arc::new(RwLock::new(HashMap::new())),
        };

        let response_senders = janus_client.response_senders.clone();

        thread::spawn(move || {
            for notification in receiver.iter() {
                if let Notification::Publish(published) = notification {
                    let payload_bytes = published.payload.as_slice();
                    let json = serde_json::from_slice::<serde_json::Value>(payload_bytes).unwrap();

                    let payload_str = json
                        .get("payload")
                        .expect("Missing payload in response")
                        .as_str()
                        .expect("Response payload is not a string");

                    let payload = serde_json::from_str::<serde_json::Value>(payload_str)
                        .map_err(|err| format_err!("Failed to parse message: {}", err))
                        .unwrap();

                    if let Some(value) = payload.get("transaction") {
                        let transaction = Transaction(String::from(value.as_str().unwrap()));
                        let response_senders = response_senders.read().unwrap();
                        let tx = response_senders.get(&transaction).unwrap();
                        tx.send(payload.to_owned()).unwrap();
                    }
                }
            }
        });

        janus_client.session_id = Some(janus_client.init_session()?);
        janus_client.handle_id = Some(janus_client.init_handle()?);
        Ok(janus_client)
    }

    fn init_session(&mut self) -> Result<SessionId, Error> {
        let response: SessionResponse = self.request(&json!({"janus": "create"}))?;

        if response.janus == "success" {
            Ok(response.data.id)
        } else {
            Err(format_err!("Unsuccessful response: {}", response.janus))
        }
    }

    fn init_handle(&mut self) -> Result<HandleId, Error> {
        let session_id = self.session_id()?;

        let response: HandleResponse = self.request(&json!({
            "janus": "attach",
            "session_id": session_id,
            "plugin": PLUGIN,
        }))?;

        if response.janus == "success" {
            Ok(response.data.id)
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

        let (tx, rx) = mpsc::sync_channel(100);

        self.response_senders
            .write()
            .unwrap()
            .insert(transaction.to_owned(), tx);

        self.response_receivers
            .write()
            .unwrap()
            .insert(transaction.to_owned(), rx);

        payload
            .as_object_mut()
            .ok_or_else(|| err_msg("Payload is not a JSON object"))?
            .insert(String::from("transaction"), json!(transaction));

        self.publish(&payload)?;
        self.wait_for_response(&transaction, Duration::from_secs(RESPONSE_TIMEOUT))
    }

    /// Wait for response for the given `transaction` and deserialize it to `R` type.
    /// Returns deserialized response on success.
    /// Returns error on timeout.
    pub fn wait_for_response<R>(
        &self,
        transaction: &Transaction,
        timeout: Duration,
    ) -> Result<R, Error>
    where
        for<'de> R: Deserialize<'de>,
    {
        let response_receivers = self
            .response_receivers
            .read()
            .map_err(|_err| err_msg("Failed to get response channels read lock"))?;

        let rx = response_receivers
            .get(transaction)
            .ok_or_else(|| format_err!("Transaction {:?} not registered", transaction))?;

        match rx.recv_timeout(timeout) {
            Ok(payload) => serde_json::from_value::<R>(payload.to_owned())
                .map_err(|err| format_err!("Failed to typify message: {}", err)),
            Err(_) => {
                let err = format_err!("Timed out waiting for the response on {:?}", transaction);
                Err(err)
            }
        }
    }

    /// Convenience wrapper around `request` to send send a message to the plugin handle.
    /// Builds the request from `body` and optional `jsep`, publishes the request,
    /// waits for ack response on it and then for the event response.
    pub fn request_message<B, J, R>(
        &mut self,
        body: B,
        jsep: Option<J>,
        timeout: Duration,
    ) -> Result<R, Error>
    where
        B: Serialize,
        J: Serialize,
        for<'de> R: Deserialize<'de>,
    {
        let session_id = self.session_id()?;
        let handle_id = self.handle_id()?;

        let mut payload = json!({
          "janus": "message",
          "session_id": session_id,
          "handle_id": handle_id,
          "body": body,
        });

        if let Some(jsep) = jsep {
            payload
                .as_object_mut()
                .ok_or_else(|| err_msg("Payload is not a JSON object"))?
                .insert(String::from("jsep"), json!(jsep));
        }

        let ack_response: GenericResponse = self.request(&payload)?;

        if ack_response.janus != "ack" {
            let err = format_err!("Expected `ack`, got `{}`", ack_response.janus);
            return Err(err);
        }

        self.wait_for_response(&ack_response.transaction, timeout)
    }

    /// Sends local ICE candidate to Janus.
    pub fn trickle_ice_candidate(
        &mut self,
        sdp_m_line_index: u32,
        candidate: &str,
    ) -> Result<(), Error> {
        let session_id = self.session_id()?;
        let handle_id = self.handle_id()?;

        let ack_response: GenericResponse = self.request(&json!({
            "janus": "trickle",
            "session_id": session_id,
            "handle_id": handle_id,
            "candidate": {
                "sdpMLineIndex": sdp_m_line_index,
                "candidate": candidate,
            }
        }))?;

        if ack_response.janus != "ack" {
            let err = format_err!("Expected `ack`, got `{}`", ack_response.janus);
            return Err(err);
        }

        Ok(())
    }

    fn graceful_disconnect(&mut self) -> Result<(), Error> {
        if let Some(session_id) = self.session_id.clone() {
            if let Some(handle_id) = self.handle_id.clone() {
                let _response: GenericResponse = self.request(&json!({
                    "janus": "detach",
                    "session_id": session_id,
                    "handle_id": handle_id,
                }))?;
            }

            let _response: GenericResponse = self.request(&json!({
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
