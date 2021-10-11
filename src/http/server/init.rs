use anyhow::Result;
use axum::{extract::Extension, Json};
use serde::Serialize;

use crate::http::client::{create_handle::CreateHandleRequest, JanusClient};

#[derive(Serialize)]
struct Response {
    session_id: u64,
    handle_id: u64,
}

async fn init(client: Extension<JanusClient>) -> Result<Json<Response>> {
    let session = client.create_session().await?;
    let handle = client
        .create_handle(CreateHandleRequest {
            session_id: session.id,
        })
        .await?;
    let app = app!()?;

    app.switchboard.with_write_lock(|mut switchboard| {
        switchboard.touch_session(request.session_id());
        Ok(())
    })?;

    Ok(Response {
        session_id: session.id,
        handle_id: handle.id,
    })
}
