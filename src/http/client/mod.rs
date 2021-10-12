use std::{
    collections::{HashMap, VecDeque},
    str::FromStr,
    sync::Mutex,
};

use crate::{switchboard::SessionId, utils::infinite_retry};

use self::{
    create_handle::{CreateHandleRequest, CreateHandleResponse},
    create_session::CreateSessionResponse,
};
use anyhow::{Context, Result};

use reqwest::{Client, StatusCode, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use tokio::sync::{
    self,
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    oneshot::{self, Sender},
};
use uuid::Uuid;

pub mod create_handle;
pub mod create_session;

#[derive(Debug)]
pub struct JanusClient {
    http: Client,
    janus_url: Url,
    session: Session,
    requests: UnboundedSender<Message>,
}

impl JanusClient {
    pub async fn new(janus_url: Url, skip_events: Vec<String>) -> Self {
        let client = Client::new();
        let session = create_session(&client, &janus_url).await;
        let (tx, mut rx) = unbounded_channel();
        tokio::spawn({
            let client = client.clone();
            let janus_url = janus_url.clone();
            let session_id = session.session_id;
            async move { start_polling(&client, &janus_url, rx, skip_events, session_id).await }
        });
        Self {
            http: Client::new(),
            janus_url,
            requests: tx,
            session,
        }
    }

    pub async fn get_events(&self, max_events: usize) -> Result<Vec<Value>> {
        let (tx, mut rx) = oneshot::channel();
        self.requests.send(Message::GetEvents {
            max_events,
            waiter: tx,
        });
        Ok(rx.await?)
    }

    pub async fn proxy_request(&self, request: Value) -> Result<Value> {
        let transaction = Uuid::new_v4();
        let (tx, mut rx) = oneshot::channel();
        self.requests.send(Message::GetResponse {
            transaction,
            waiter: tx,
        });
        let _ack: AckResponse = send_post(
            &self.http,
            self.janus_url.clone(),
            &JanusRequest {
                transaction,
                janus: "message",
                plugin: None,
                data: request,
            },
        )
        .await?;
        Ok(rx.await?)
    }

    pub fn session(&self) -> Session {
        self.session
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
    transaction: Uuid,
    janus: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin: Option<&'static str>,
    #[serde(flatten)]
    data: T,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Session {
    pub session_id: u64,
    pub handle_id: u64,
}

async fn create_session(client: &Client, url: &Url) -> Session {
    let create_session = || async {
        let app = app!()?;
        let session: JanusResponse<CreateSessionResponse> = send_post(
            client,
            url.clone(),
            &JanusRequest {
                transaction: Uuid::new_v4(),
                plugin: None,
                janus: "create",
                data: (),
            },
        )
        .await?;
        let handle: JanusResponse<CreateHandleResponse> = send_post(
            client,
            url.clone(),
            &JanusRequest {
                transaction: Uuid::new_v4(),
                janus: "attach",
                plugin: Some("janus.plugin.conference"),
                data: CreateHandleRequest {
                    session_id: session.data.id,
                },
            },
        )
        .await?;
        app.switchboard.with_write_lock(|mut switchboard| {
            switchboard.touch_session(SessionId::new(handle.data.id));
            Ok(Session {
                session_id: session.data.id,
                handle_id: handle.data.id,
            })
        })
    };
    fure::retry(create_session, infinite_retry())
        .await
        .expect("Must be success")
}

async fn send_post<R: DeserializeOwned>(
    client: &Client,
    url: Url,
    body: &impl Serialize,
) -> reqwest::Result<R> {
    Ok(client.post(url).json(body).send().await?.json().await?)
}

#[derive(Debug)]
enum Message {
    GetResponse {
        transaction: Uuid,
        waiter: Sender<Value>,
    },
    GetEvents {
        max_events: usize,
        waiter: Sender<Vec<Value>>,
    },
}

async fn start_polling(
    client: &Client,
    janus_url: &Url,
    mut requests: UnboundedReceiver<Message>,
    skip_events: Vec<String>,
    session_id: u64,
) {
    let (events_tx, mut events_rx) = unbounded_channel();
    let (responses_tx, mut responses_rx) = unbounded_channel();
    let mut waiting_requests = HashMap::new();
    let mut events_requests = VecDeque::new();
    tokio::task::spawn({
        let client = client.clone();
        let url = janus_url.clone();
        async move {
            polling(
                &client,
                &url,
                session_id,
                events_tx,
                responses_tx,
                skip_events,
            )
            .await
        }
    });
    loop {
        tokio::select! {
            Some(message) = requests.recv() => {
                match message {
                    Message::GetResponse { transaction, waiter } => { waiting_requests.insert(transaction, waiter); },
                    Message::GetEvents { max_events, waiter } => { events_requests.push_back((max_events, waiter)); },
                }
            }
            Some(event) = events_rx.recv(), if !events_requests.is_empty() => {
                let (max_capacity, waiter) = events_requests.pop_front().expect("Must have elements");
                let mut response = Vec::with_capacity(max_capacity);
                response.push(event);
                loop {
                    if response.len() == max_capacity {
                        break;
                    }
                    match events_rx.try_recv() {
                        Ok(event) => response.push(event),
                        Err(_) => break,
                    }
                }
                //todo maybe it is better to return events back in queue in case of receiver part of this waiter had been  dropped?
                let _ = waiter.send(response);
            }
            Some((id, event)) = responses_rx.recv() => {
                if let Some(waiter) = waiting_requests.remove(&id) {
                    let _ = waiter.send(event);
                }
            }
        }
    }
}

async fn polling(
    client: &Client,
    url: &Url,
    session_id: u64,
    events_sink: UnboundedSender<Value>,
    responses_sink: UnboundedSender<(Uuid, Value)>,
    skip_events: Vec<String>,
) {
    let send_request = || async {
        client
            .get(format!("{}/{}?maxev=5", url, session_id))
            .send()
            .await?
            .json::<Vec<Value>>()
            .await
    };
    loop {
        match send_request().await {
            Ok(events) => {
                for event in events {
                    if let Some(event_kind) = event.get("janus").and_then(|x| x.as_str()) {
                        if skip_events.iter().any(|e| e.as_str() == event_kind) {
                            continue;
                        }
                        if event_kind == "event" {
                            if let Some(Ok(tran)) = event
                                .get("transaction")
                                .and_then(|x| x.as_str())
                                .map(Uuid::from_str)
                            {
                                let _ = responses_sink.send((tran, event));
                            }
                        } else {
                            let _ = events_sink.send(event);
                        }
                    }
                }
            }
            rest => {
                err!("Something bad happened: {:?}", rest)
            }
        }
    }
}
