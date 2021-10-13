use anyhow::anyhow;

use http::StatusCode;
use reqwest::Client;

use crate::{conf::Description, utils::infinite_retry};

pub async fn register(
    http_client: &Client,
    description: &Description,
    conference_url: &str,
    token: &str,
) {
    let register = || async {
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
    fure::retry(register, infinite_retry())
        .await
        .expect("Must be sucess");
}
