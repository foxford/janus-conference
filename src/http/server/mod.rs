use std::sync::Arc;

use anyhow::anyhow;
use axum::{extract::Extension, handler::post, routing::BoxRoute, AddExtensionLayer, Json, Router};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::switchboard::{AgentId, StreamId};

use self::stream_upload::stream_upload;

use super::client::{create_handle::CreateHandleRequest, JanusClient};

pub mod reader_config_update;
pub mod stream_upload;
pub mod writer_config_update;

fn map_result<T>(
    result: anyhow::Result<T>,
) -> Result<Json<T>, (StatusCode, Json<svc_error::Error>)> {
    result.map(Json).map_err(|err| {
        let error = svc_error::Error::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .detail(&format!("Error occured: {:?}", err))
            .build();
        (error.status_code(), Json(error))
    })
}

pub fn router(janus_client: JanusClient) -> Router<BoxRoute> {
    Router::new()
        .route(
            "/proxy",
            post(
                |janus_client: Extension<Arc<JanusClient>>, Json(request)| async {
                    map_result(janus_client.proxy_request(request).await)
                },
            ),
        )
        .route(
            "/stream-upload",
            post(|Json(r)| async { map_result(stream_upload(r).await) }),
        )
        .route(
            "/writer-config-update",
            post(|Json(request)| async {
                map_result(writer_config_update::writer_config_update(request))
            }),
        )
        .route(
            "/reader-config-update",
            post(|Json(request)| async {
                map_result(reader_config_update::reader_config_update(request))
            }),
        )
        .layer(AddExtensionLayer::new(Arc::new(janus_client)))
        .boxed()
}
