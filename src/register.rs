use std::time::Duration;

use anyhow::anyhow;

use fure::{
    backoff::fixed,
    policies::{backoff, cond},
};
use http::StatusCode;
use reqwest::Client;

use crate::{conf::Description, utils::retry_failed};

pub async fn register(
    http_client: &Client,
    description: &Description,
    conference_url: &str,
    token: &str,
) {
    let register = || async move {
        let response = http_client
            .post(conference_url)
            .header("Authorization", token)
            .json(description)
            .send()
            .await?;
        match response.status() {
            StatusCode::OK => Ok(()),
            StatusCode::UNAUTHORIZED => Err(anyhow!("Bad token")),
            _ => Err(anyhow!("Not registered")),
        }
    };
    fure::retry(
        register,
        cond(backoff(fixed(Duration::from_secs(1))), retry_failed),
    )
    .await
    .expect("Must be sucess");
}
