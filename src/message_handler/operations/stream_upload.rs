use std::fs::File;
use std::io::{BufRead, BufReader};

use anyhow::{bail, format_err, Context, Error, Result};
use async_trait::async_trait;
use http::StatusCode;
use svc_error::Error as SvcError;
use tokio::process::Command; // No async-std equivalent yet.

use crate::recorder::Recorder;
use crate::switchboard::StreamId;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    id: StreamId,
    bucket: String,
    object: String,
}

#[derive(Serialize)]
struct Response {
    id: StreamId,
    started_at: u64,
    time: Vec<(u64, u64)>,
}

#[async_trait]
impl super::Operation for Request {
    async fn call(&self, _request: &super::Request) -> super::OperationResult {
        janus_info!(
            "[CONFERENCE] Calling stream.upload operation with id {}",
            self.id
        );

        app!()
            .map_err(internal_error)?
            .switchboard
            .with_write_lock(|mut switchboard| {
                // The stream still may be ongoing and we must stop it gracefully.
                if let Some(publisher) = switchboard.publisher_of(self.id) {
                    // At first we synchronously stop the stream and hence the recording
                    // ensuring that it finishes correctly.
                    switchboard.remove_stream(self.id)?;

                    // Then we disconnect the publisher to close its PeerConnection and notify
                    // the frontend. Disconnection also implies stream removal but it's being
                    // performed asynchronously through a janus callback and to avoid race condition
                    // we have preliminary removed the stream in a synchronous way.
                    switchboard.disconnect(publisher)?;
                }

                Ok(())
            })
            .map_err(internal_error)?;

        upload_record(&self).await.map_err(internal_error)?;

        let recorder_config = app!().map_err(internal_error)?.config.recordings.clone();
        let recorder = Recorder::new(&recorder_config, self.id);
        let (started_at, segments) = parse_segments(&recorder).map_err(internal_error)?;

        recorder.delete_record().map_err(internal_error)?;

        Ok(Response {
            id: self.id,
            started_at,
            time: segments,
        }
        .into())
    }

    fn is_handle_jsep(&self) -> bool {
        false
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

async fn upload_record(request: &Request) -> Result<()> {
    janus_info!("[CONFERENCE] Preparing & uploading record");

    let mut script_path = std::env::current_exe()
        .context("Failed to get current executable path")?
        .parent()
        .ok_or_else(|| format_err!("Missing current executable dir"))?
        .to_path_buf();

    script_path.push("upload_record.sh");
    let mut command = Command::new(&script_path);
    let stream_id = request.id.to_string();
    command.args(&[&stream_id, &request.bucket, &request.object]);
    janus_verb!("[CONFERENCE] {:?}", command);

    command
        .status()
        .await
        .map_err(|err| format_err!("Failed to run upload_record.sh, return code = '{}'", err))
        .and_then(|status| {
            if status.success() {
                janus_info!(
                    "[CONFERENCE] Record {} successfully uploaded to {}/{}",
                    request.id,
                    request.bucket,
                    request.object
                );

                Ok(())
            } else {
                Err(format_err!("Failed to prepare & upload record: {}", status))
            }
        })
}

fn parse_segments(recorder: &Recorder) -> Result<(u64, Vec<(u64, u64)>)> {
    let mut path = recorder.get_records_dir();
    path.push("segments.csv");

    let file = File::open(&path)?;
    let mut segments = vec![];

    for read_result in BufReader::new(file).lines() {
        let line = match read_result {
            Ok(line) => line,
            Err(err) => bail!(err),
        };

        // "123456789,123.45" => (123456789, 123.45)
        match line.splitn(2, ',').collect::<Vec<&str>>().as_slice() {
            [started_at, duration] => {
                let parsed_started_at = started_at
                    .parse::<u64>()
                    .context("Failed to parse started_at")?;

                let parsed_duration = duration
                    .parse::<f32>()
                    .context("Failed to parse duration")?;

                segments.push((parsed_started_at, parsed_duration))
            }
            _ => bail!("Failed to split line: {}", line),
        }
    }

    let absolute_started_at = match segments.first() {
        None => bail!("No segments parsed"),
        Some((started_at, _)) => started_at.to_owned(),
    };

    // [(123456789, 123.45), (123470134, 456.78)] => [(0, 12345), (13345, 59023)]
    let relative_segments = segments
        .into_iter()
        .map(|(started_at, duration_sec)| {
            let relative_started_at = started_at - absolute_started_at;
            let duration_ms = (duration_sec * 1000.0) as u64;
            (relative_started_at, relative_started_at + duration_ms)
        })
        .collect();

    Ok((absolute_started_at, relative_segments))
}
