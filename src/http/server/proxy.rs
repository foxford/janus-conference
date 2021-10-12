use anyhow::Result;
use axum::{extract::Extension, Json};
use serde_json::Value;

use crate::http::client::JanusClient;

pub async fn proxy(client: Extension<JanusClient>, Json(request): Json<Value>) -> Result<Value> {
    client.proxy_request(request).await
}
