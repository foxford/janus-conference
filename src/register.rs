use anyhow::anyhow;
use fure::{
    backoff::fixed,
    policies::{backoff, cond},
    Policy,
};
use http::StatusCode;
use reqwest::Client;
use serde::Serialize;
use std::time::Duration;

use crate::{conf::Description, http::client::{create_handle::CreateHandleRequest, JanusClient}, switchboard::SessionId, utils::infinite_retry};

#[derive(Serialize, Clone, Copy)]
struct SessionAndHandle {
    session_id: u64,
    handle_id: u64,
}

#[derive(Serialize)]
struct InitRequest {
    session_and_handle: SessionAndHandle,
    description: Description,
}

pub async fn register(
    http_client: &Client,
    janus_client: &JanusClient,
    description: &Description,
    conference_url: &str,
    token: &str,
) {
    let create_session = || async {
        let app = app!()?;
        let session = janus_client.create_session().await?;
        let handle = janus_client
            .create_handle(CreateHandleRequest {
                session_id: session.id,
            })
            .await?;
        let desc = serde_json::to_vec(&description)?;
        app.switchboard.with_write_lock(|mut switchboard| {
            switchboard.touch_session(SessionId::new(handle.id));
            Ok(SessionAndHandle {
                session_id: session.id,
                handle_id: handle.id,
            })
        })
    };
    let session = fure::retry(create_session, infinite_retry())
        .await
        .expect("Must be success");

    let register = || async {
        let response = http_client
            .post(conference_url)
            .header("Authorization", token)
            .json(&InitRequest {
                session_and_handle: session,
                description: description.clone(),
            })
            .send()
            .await?;
        match response.status() {
            StatusCode::OK => Ok(()),
            StatusCode::UNAUTHORIZED => Err(anyhow!("Bad token")),
            _ => Err(anyhow!("Not registered")),
        }
    };
    fure::retry(register, infinite_retry())
        .await
        .expect("Must be sucess");
}
