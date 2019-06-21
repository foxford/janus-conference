// Full record upload functional test.
//
// 1. Copy test videos from ./tests/files/recording to ./recordings/<ID> where <ID> is random number.
// 2. Send `stream.upload` request for that <ID> with MQTT, get ack response.
// 3. Janus concatenates videos and uploads it to S3 then sends the second response.
// 4. When this response arrives ensure that it's successful.
// 5. Download the full video from S3 to temporary file. Delete the original from S3 to keep it clean.
// 6. Check the duration of the downloaded full video to make sure that it's really concatenated.
// 7. The response also contains start/stop timestamps of the original parts. Ensure that they're OK.
// 8. Cleanup: delete ./recordings/<ID> and the downloaded full record.

use std::time::Duration;

use failure::{err_msg, Error};
use serde_json::json;
use tempfile::TempDir;

use crate::support::conference_plugin_api_responses::UploadResponse;
use crate::support::janus_client::JanusClient;
use crate::support::s3_client_wrapper::S3ClientWrapper as S3Client;
use crate::support::test_recording::TestRecording;

const BUCKET: &str = "origin.webinar.beta.netology.ru";
const DISCOVERER_TIMEOUT: u64 = 15;

#[test]
fn it_uploads_full_record() -> Result<(), Error> {
    // Setup.
    gst::init()?;
    let mut janus_client = JanusClient::new()?;
    let s3_client = S3Client::new()?;
    let recording = TestRecording::new()?;
    let object_id = format!("{}.test.mp4", recording.id());

    // Send `stream.upload` request.
    let payload = json!({
        "method": "stream.upload",
        "id": recording.id(),
        "bucket": BUCKET,
        "object": object_id,
    });

    let response: UploadResponse = janus_client.request_message(
        payload,
        None::<serde_json::Value>,
        Duration::from_secs(30),
    )?;

    assert_eq!(response.janus, "event");
    assert_eq!(response.plugindata.data.status, 200);
    drop(janus_client);

    // Download the full record file from S3 and delete it from there.
    let temp_dir = TempDir::new()?;
    let record_path = temp_dir.into_path().join(&object_id);
    s3_client.get_object(BUCKET, &object_id, &record_path)?;
    s3_client.delete_object(BUCKET, &object_id)?;

    // Assert downloaded file duration to ensure that it's really concatenated video.
    let duration = gst_pbutils::Discoverer::new(gst::ClockTime::from_seconds(DISCOVERER_TIMEOUT))?
        .discover_uri(&format!(
            "file://{}",
            record_path.as_path().to_string_lossy()
        ))?
        .get_duration()
        .mseconds()
        .ok_or_else(|| err_msg("Empty duration"))?;

    assert_eq!(duration, 3633);

    // Assert part timestamps from the response after removing the file from S3 to keep it clean
    // even in case of failure.
    assert_eq!(
        response.plugindata.data.time,
        vec![
            (1560489452218, 1560489453851),
            (1560489460900, 1560489462900),
        ]
    );

    Ok(())
}
