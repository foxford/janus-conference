use anyhow::anyhow;
use axum::{extract::Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::switchboard::{AgentId, StreamId};

use super::client::{create_handle::CreateHandleRequest, JanusClient};

pub mod agent_leave;
pub mod reader_config_update;
pub mod stream_upload;
pub mod writer_config_update;
pub mod init;

async fn poll();
async fn proxy_request(
    client: Extension<JanusClient>,
    request: Json<Value>,
) -> anyhow::Result<Json<Value>> {
    // client.proxy_request(request, )
}

async fn stream_upload();

async fn agent_leave();
