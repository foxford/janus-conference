use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};

use crate::{switchboard::SessionId, utils::infinite_retry};

use self::{
    create_handle::{CreateHandleRequest, CreateHandleResponse},
    create_session::CreateSessionResponse,
};
use anyhow::Context;

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

#[derive(Clone, Debug)]
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
        Ok(Self {
            http: Client::new(),
            janus_url,
            requests: tx,
            session,
        })
    }

    pub async fn get_events(&self, max_events: usize) -> Vec<Value> {
        let (tx, mut rx) = oneshot::channel();
        self.requests.send(Message::GetEvents {
            max_events,
            waiter: tx,
        });
        Ok(rx.await?)
    }

    pub async fn proxy_request<T: Serialize>(&self, request: T) -> anyhow::Result<()> {
        let transaction = Uuid::new_v4();
        let (tx, mut rx) = oneshot::channel();
        self.requests.send(Message::GetResponse {
            transaction,
            waiter: tx,
        });
        let _ack: AckResponse = send_post(
            &self.http,
            &self.janus_url,
            JanusRequest {
                transaction,
                janus: "message",
                plugin: None,
                data: request,
            },
        )
        .await?;
        Ok(rx.await?)
    }

    pub fn session(&self) -> &Session {
        &self.session
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

struct Session {
    session_id: u64,
    handle_id: u64,
}

async fn create_session(client: &Client, url: &Url) -> Session {
    let create_session = || async {
        let app = app!()?;
        let session: JanusResponse<CreateSessionResponse> = send_post(
            client,
            url,
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
            url,
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
            switchboard.touch_session(SessionId::new(handle.id));
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

async fn send_post(
    client: &Client,
    url: &Url,
    body: &impl Serialize,
) -> reqwest::Result<impl DeserializeOwned> {
    client.post(url).json(body).send().await?.json().await?
}

#[derive(Debug)]
enum Message {
    GetResponse {
        transaction: Uuid,
        waiter: Sender<Value>,
    },
    GetEvents {
        max_events: usize,
        waiter: Sender<Value>,
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
    tokio::task::spawn(polling(
        &client,
        responses_tx,
        events_tx,
        session_id,
        skip_events,
    ));
    loop {
        tokio::select! {
            Some(message) = requests.recv() => {
                match message {
                    Message::GetResponse { transaction, waiter} => waiting_requests.insert(transaction, waiter),
                    Message::GetEvents { max_events, waiter } => events_requests.push((max_events, waiter)),
                };
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
            Some((id, event)) = events_rx.recv() => {
                if let Some(req) = waiting_requests.remove(&id) {
                    let _ = req.send(event);
                }
            }
        }
    }
}

async fn polling(
    client: &Client,
    url: &Url,
    events_sink: UnboundedSender<Value>,
    responses_sink: UnboundedSender<(Uuid, Value)>,
    session_id: u64,
    skip_events: Vec<String>,
) {
    let send_request = || client.get(format!("{}/{}?maxev=5", url, session_id)).send();
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
