use std::{collections::HashMap, str::FromStr};

use serde_json::Value;
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    oneshot::Sender,
};
use uuid::Uuid;

use super::JanusClient;

async fn start_polling(
    janus_client: JanusClient,
    events_sink: UnboundedSender<Value>,
    mut requests: UnboundedReceiver<(Uuid, Sender)>,
    skip_events: Vec<String>,
    session_id: u64,
) {
    let (tx, mut rx) = unbounded_channel();
    tokio::task::spawn(polling(client, events_sink, tx, session_id, skip_events));

    let waiting_requests = HashMap::new();
    loop {
        tokio::select! {
            Some((id, event)) = rx.recv() => {
                if let Some(req) = waiting_requests.remove(&id) {
                    req.send(event);
                }
            }
            Some((id, waiter)) = requests.recv() => {
                waiting_requests.insert(id, waiter);
            }
        }
    }
}

async fn polling(
    client: JanusClient,
    events_sink: UnboundedSender<Value>,
    requests_sink: UnboundedSender<(Uuid, Value)>,
    session_id: u64,
    skip_events: Vec<String>,
) {
    loop {
        match client.poll(session_id).await {
            Ok(events) => {
                for event in events {
                    if let Some(event_kind) = result.get("janus").and_then(|x| x.as_str()) {
                        if skip_events.contains(event_kind) {
                            continue;
                        }
                        if event_kind == "event" {
                            if let Some(Ok(tran)) = result.get("transaction").map(Uuid::from_str) {
                                requests_sink.send((tran, event))
                            }
                        } else {
                            events_sink.send(event)
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
