use std::{collections::HashMap, sync::Mutex};

use self::{
    create_handle::{CreateHandleRequest, CreateHandleResponse},
    create_session::CreateSessionResponse,
};
use anyhow::Context;

use reqwest::{Client, StatusCode, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use tokio::sync::{self, oneshot::Sender};
use uuid::Uuid;

pub mod create_handle;
pub mod create_session;
pub mod polling;

#[derive(Clone, Debug)]
pub struct JanusClient {
    http: Client,
    janus_url: Url,
}

impl JanusClient {
    pub fn new(janus_url: &str) -> anyhow::Result<Self> {
        Ok(Self {
            http: Client::new(),
            janus_url: janus_url.parse()?,
        })
    }

    pub async fn poll(&self, session_id: u64) -> anyhow::Result<Vec<Value>> {
        let response = self
            .http
            .get(format!("{}/{}?maxev=5", self.janus_url, session_id))
            .send()
            .await?;
        Ok(response.json().await?)
    }

    pub async fn create_handle(
        &self,
        request: CreateHandleRequest,
    ) -> anyhow::Result<CreateHandleResponse> {
        let response: JanusResponse<CreateHandleResponse> =
            self.send_request(create_handle(request)).await?;
        Ok(response.data)
    }

    pub async fn create_session(&self) -> anyhow::Result<CreateSessionResponse> {
        let response: JanusResponse<CreateSessionResponse> =
            self.send_request(create_session()).await?;
        Ok(response.data)
    }

    pub async fn proxy_request<T: Serialize>(
        &self,
        request: T,
        transaction: Uuid,
    ) -> anyhow::Result<()> {
        let _ack: AckResponse = self
            .send_request(JanusRequest {
                transaction,
                janus: "message",
                plugin: None,
                data: request,
            })
            .await?;
        Ok(())
    }

    async fn send_request<R: DeserializeOwned>(&self, body: impl Serialize) -> anyhow::Result<R> {
        let body = serde_json::to_vec(&body)?;
        let response = self
            .http
            .post(self.janus_url.clone())
            .body(body)
            .send()
            .await?
            .text()
            .await?;
        Ok(serde_json::from_str(&response).context(response)?)
    }
}

#[derive(Deserialize, Debug)]
enum Ack {
    #[serde(rename = "ack")]
    Ack,
}

#[derive(Deserialize, Debug)]
struct AckResponse {
    janus: Ack,
}

#[derive(Deserialize, Debug)]
enum Success {
    #[serde(rename = "success")]
    Success,
}

#[derive(Deserialize, Debug)]
struct JanusResponse<T> {
    data: T,
    janus: Success,
}

#[derive(Serialize, Debug)]
struct JanusRequest<T> {
    #[serde(with = "serialize_as_str")]
    transaction: Uuid,
    janus: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin: Option<&'static str>,
    #[serde(flatten)]
    data: T,
}

fn create_session() -> JanusRequest<()> {
    JanusRequest {
        transaction: Transaction::only_id(),
        plugin: None,
        janus: "create",
        data: (),
    }
}

fn create_handle(request: CreateHandleRequest) -> JanusRequest<CreateHandleRequest> {
    JanusRequest {
        transaction: Transaction::only_id(),
        janus: "attach",
        plugin: Some("janus.plugin.conference"),
        data: request,
    }
}
