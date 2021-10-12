use std::{
    fs::File,
    io::{BufRead, BufReader},
    process::Command,
    thread,
};

use crate::recorder::RecorderHandle;
use crate::switchboard::StreamId;
use anyhow::{anyhow, format_err, Context, Error, Result};
use axum::{extract::Extension, Json};
use crossbeam_channel::{Receiver, Sender};
use http::StatusCode;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use svc_error::Error as SvcError;
use tokio::sync::oneshot;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    id: StreamId,
    backend: String,
    bucket: String,
}

#[derive(Serialize)]
struct Response {
    id: StreamId,
    mjr_dumps_uris: Vec<String>,
}

pub async fn stream_upload(Json(request): Json<Request>) -> Result<Json<Value>> {
    let app = app!()?;
    if !app.config.upload.backends.contains(&request.backend) {
        let err = anyhow!("Unknown backend '{}'", request.backend);
        err!("Unknown backend: {:?}", err; {"rtc_id": request.id});
        return Err(anyhow!("Unknown backend"));
    }

    app.switchboard
            .with_write_lock(|mut switchboard| {
                // The stream still may be ongoing and we must stop it gracefully.
                if let Some(publisher) = switchboard.publisher_of(request.id) {
                    warn!(
                        "Stream upload has been called while still ongoing; stopping it and disconnecting everyone";
                        {"rtc_id": request.id}
                    );

                    let subscribers = switchboard.subscribers_to(publisher).to_owned();

                    // At first we synchronously stop the stream and hence the recording
                    // ensuring that it finishes correctly.
                    switchboard.remove_stream(request.id)?;

                    // Then we disconnect the publisher to close its PeerConnection and notify
                    // the frontend. Disconnection also implies stream removal but it's being
                    // performed asynchronously through a janus callback and to avoid race condition
                    // we have preliminary removed the stream in a synchronous way.
                    switchboard.disconnect(publisher)?;

                    // Disconnect subscribers also to avoid memory leak.
                    for subscriber in subscribers {
                        switchboard.disconnect(subscriber)?;
                    }
                }

                Ok(())
            })?;
    let recorder = app.recorders_creator.new_handle(request.id);
    recorder.wait_stop().await?;

    recorder.check_existence()?;

    match app.uploader.upload_record(request.clone()).await? {
        UploadStatus::AlreadyRunning => Ok(Json(
            serde_json::json!({"id": request.id, "state": "already_running"}),
        )),
        UploadStatus::Done => {
            let dumps = get_dump_uris(&recorder)?;
            recorder.delete_record()?;

            Ok(Json(
                serde_json::json!({"id": request.id, "mjr_dumps_uris": dumps}),
            ))
        }
    }
}

///////////////////////////////////////////////////////////////////////////////

pub enum UploadStatus {
    AlreadyRunning,
    Done,
}

const LOCKFILE_EARLY_EXIT_STATUS: i32 = 251;

pub struct Uploader {
    requests: Sender<(Request, oneshot::Sender<Result<UploadStatus>>)>,
}

impl Uploader {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        thread::spawn(|| uploader(rx));
        Self { requests: tx }
    }

    async fn upload_record(&self, request: Request) -> Result<UploadStatus> {
        let (tx, mut rx) = oneshot::channel();
        self.requests.send((request, tx)).expect("Must be alive");
        rx.await?
    }
}

fn uploader(requests: Receiver<(Request, oneshot::Sender<Result<UploadStatus>>)>) {
    loop {
        let (request, waiter) = requests.recv().expect("Sender must be alive");
        let result = upload_record(&request);
        let _ = waiter.send(result);
    }
}

fn upload_record(request: &Request) -> Result<UploadStatus> {
    info!("Preparing & uploading record"; {"rtc_id": request.id});

    let mut script_path = std::env::current_exe()
        .context("Failed to get current executable path")?
        .parent()
        .ok_or_else(|| format_err!("Missing current executable dir"))?
        .to_path_buf();

    script_path.push("upload_record.sh");
    let mut command = Command::new(&script_path);
    let stream_id = request.id.to_string();

    command.args(&[&stream_id, &request.backend, &request.bucket]);

    huge!("Running stream upload shell command: {:?}", command);

    command
        .status()
        .map_err(|err| format_err!("Failed to run upload_record.sh, return code = '{}'", err))
        .and_then(|status| {
            if status.success() {
                info!(
                    "Dumps successfully uploaded to {} bucket", request.bucket;
                    {"rtc_id": request.id}
                );

                Ok(UploadStatus::Done)
            } else {
                match status.code() {
                    Some(LOCKFILE_EARLY_EXIT_STATUS) => Ok(UploadStatus::AlreadyRunning),
                    _ => Err(format_err!("Failed to prepare & upload record: {}", status)),
                }
            }
        })
}

fn get_dump_uris(recorder: &RecorderHandle) -> Result<Vec<String>> {
    let mut path = recorder.get_records_dir();
    path.push("dumps.txt");
    Ok(BufReader::new(File::open(path)?)
        .lines()
        .collect::<Result<Vec<_>, _>>()?)
}
