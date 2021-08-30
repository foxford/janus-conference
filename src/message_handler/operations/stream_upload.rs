use std::fs::File;
use std::io::{BufRead, BufReader};

use anyhow::{format_err, Context, Error, Result};
use async_std::process::Command;
use async_trait::async_trait;
use http::StatusCode;
use svc_error::Error as SvcError;

use crate::switchboard::StreamId;
use crate::{message_handler::generic::MethodKind, recorder::RecorderHandle};

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

#[async_trait]
impl super::Operation for Request {
    async fn call(&self, _request: &super::Request) -> super::OperationResult {
        verb!("Calling stream.upload operation"; {"rtc_id": self.id});

        {
            let app = app!().map_err(internal_error)?;

            if !app.config.upload.backends.contains(&self.backend) {
                let err = anyhow!("Unknown backend '{}'", self.backend);
                err!("{}", err; {"rtc_id": self.id});
                return Err(error(StatusCode::BAD_REQUEST, err));
            }
        }

        app!()
            .map_err(internal_error)?
            .switchboard
            .with_write_lock(|mut switchboard| {
                // The stream still may be ongoing and we must stop it gracefully.
                if let Some(publisher) = switchboard.publisher_of(self.id) {
                    warn!(
                        "Stream upload has been called while still ongoing; stopping it and disconnecting everyone";
                        {"rtc_id": self.id}
                    );

                    let subscribers = switchboard.subscribers_to(publisher).to_owned();

                    // At first we synchronously stop the stream and hence the recording
                    // ensuring that it finishes correctly.
                    switchboard.remove_stream(self.id)?;

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
            })
            .map_err(internal_error)?;
        let recorder = app!()
            .map_err(internal_error)?
            .recorders_creator
            .new_handle(self.id);
        recorder.wait_stop().await.map_err(internal_error)?;

        recorder
            .check_existence()
            .map_err(|err| error(StatusCode::NOT_FOUND, err))?;

        match upload_record(self).await.map_err(internal_error)? {
            UploadStatus::AlreadyRunning => {
                Ok(serde_json::json!({"id": self.id, "state": "already_running"}).into())
            }
            UploadStatus::Done => {
                let dumps = get_dump_uris(&recorder).map_err(internal_error)?;
                recorder.delete_record().map_err(internal_error)?;

                Ok(Response {
                    id: self.id,
                    mjr_dumps_uris: dumps,
                }
                .into())
            }
        }
    }

    fn stream_id(&self) -> Option<StreamId> {
        None
    }

    fn method_kind(&self) -> Option<MethodKind> {
        Some(MethodKind::StreamUpload)
    }
}

fn error(status: StatusCode, err: Error) -> SvcError {
    SvcError::builder()
        .kind(
            "stream_upload_error",
            "Error uploading a recording of stream",
        )
        .status(status)
        .detail(&err.to_string())
        .build()
}

fn internal_error(err: Error) -> SvcError {
    error(StatusCode::INTERNAL_SERVER_ERROR, err)
}

///////////////////////////////////////////////////////////////////////////////

pub enum UploadStatus {
    AlreadyRunning,
    Done,
}

const LOCKFILE_EARLY_EXIT_STATUS: i32 = 251;

async fn upload_record(request: &Request) -> Result<UploadStatus> {
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
        .await
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
