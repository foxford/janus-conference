use std::{sync::Arc, time::Duration};
use svc_error::extension::sentry;

use axum::{
    extract::{Extension, Query},
    handler::{get, post},
    routing::BoxRoute,
    AddExtensionLayer, Json, Router,
};
use http::StatusCode;
use serde::Deserialize;
use tokio::time::timeout;

use crate::metrics::Metrics;

use self::stream_upload::stream_upload;

use super::client::JanusClient;

pub mod reader_config_update;
pub mod stream_upload;
pub mod writer_config_update;

fn map_result<T>(
    result: anyhow::Result<T>,
) -> Result<Json<T>, (StatusCode, Json<svc_error::Error>)> {
    result
        .map(|x| {
            Metrics::observe_success_response();
            Json(x)
        })
        .map_err(|err| {
            Metrics::observe_failed_response();
            let error = svc_error::Error::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .detail(&format!("Error occured: {:?}", err))
                .build();
            sentry::send(error.clone()).unwrap_or_else(|err| {
                warn!("Failed to send error to Sentry: {}", err);
            });
            (error.status_code(), Json(error))
        })
}

#[derive(Deserialize)]
struct MaxEvents {
    #[serde(default)]
    max_events: Option<usize>,
}

pub fn router(janus_client: JanusClient) -> Router<BoxRoute> {
    Router::new()
        .route(
            "/proxy",
            post(
                |janus_client: Extension<Arc<JanusClient>>, Json(request)| async move {
                    let _timer = Metrics::start_proxy();
                    map_result(janus_client.proxy_request(request).await)
                },
            ),
        )
        .route(
            "/create-handle",
            post(
                |janus_client: Extension<Arc<JanusClient>>, Json(body)| async move {
                    map_result(janus_client.create_handle(body).await)
                },
            ),
        )
        .route(
            "/poll",
            get(
                |janus_client: Extension<Arc<JanusClient>>,
                 max_events: Query<MaxEvents>| async move {
                    map_result(
                        timeout(
                            Duration::from_secs(30),
                            janus_client.get_events(max_events.max_events.unwrap_or(5)),
                        )
                        .await
                        .unwrap_or_else(|_| Ok(Vec::new())),
                    )
                },
            ),
        )
        .route(
            "/stream-upload",
            post(|Json(r)| async move {
                let _timer = Metrics::start_upload();
                map_result(stream_upload(r).await)
            }),
        )
        .route(
            "/writer-config-update",
            post(|Json(request)| async move {
                let _timer = Metrics::start_writer_config();
                map_result(writer_config_update::writer_config_update(request))
            }),
        )
        .route(
            "/reader-config-update",
            post(|Json(request)| async move {
                let _timer = Metrics::start_reader_config();
                map_result(reader_config_update::reader_config_update(request))
            }),
        )
        .layer(AddExtensionLayer::new(Arc::new(janus_client)))
        .boxed()
}
