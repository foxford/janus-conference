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

use crate::{
    conf::Description,
    http::client::{create_handle::CreateHandleRequest, JanusClient, Session},
    switchboard::SessionId,
    utils::infinite_retry,
};

#[derive(Serialize)]
struct InitRequest {
    session: Session,
    description: Description,
}

pub async fn register(
    http_client: &Client,
    session: Session,
    description: &Description,
    conference_url: &str,
    token: &str,
) {
    let register = || async {
        let response = http_client
            .post(conference_url)
            .header("Authorization", token)
            .json(&InitRequest {
                session,
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
