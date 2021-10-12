use anyhow::anyhow;
use axum::{extract::Extension, handler::post, routing::BoxRoute, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::switchboard::{AgentId, StreamId};

use self::proxy::proxy;

use super::client::{create_handle::CreateHandleRequest, JanusClient};

pub mod proxy;
pub mod reader_config_update;
pub mod stream_upload;
pub mod writer_config_update;

pub fn router(janus_client: JanusClient) -> Router<BoxRoute> {
    Router::new().route("/proxy", post(proxy)).boxed()
}
