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

include!("./janus_client.rs");

extern crate gstreamer;
extern crate gstreamer_pbutils;
extern crate rusoto_s3;
extern crate rusoto_signature;
extern crate tempfile;

use std::path::{Path, PathBuf};
use std::{env, fs, io};

use gstreamer as gst;
use gstreamer_pbutils::prelude::*;
use rusoto_core::request::HttpClient;
use rusoto_credential::StaticProvider;
use rusoto_s3::{DeleteObjectRequest, GetObjectRequest, S3Client, S3};
use rusoto_signature::Region;
use tempfile::TempDir;

const BUCKET: &str = "origin.webinar.example.org";
const TEST_RECORDING_PATH: &str = "./tests/files/recording";
const RECORDINGS_DIR: &str = "./recordings";
const DISCOVERER_TIMEOUT: u64 = 15;

#[test]
fn it_uploads_full_record() {
    // Setup
    gst::init().expect("Failed to initialize GStreamer");
    let mut janus_client = JanusClient::new().expect("Failed to initialize Janus client");
    let s3_client = S3ClientWrapper::new().expect("Failed to build S3 client");
    let recording = TestRecording::new().expect("Failed to initialize test recording");
    let object_id = format!("{}.test.mp4", recording.id);

    // Send `stream.upload` request and expect ack response.
    let ack_response: UploadAckResponse = janus_client
        .request_message(json!({
            "method": "stream.upload",
            "id": recording.id,
            "bucket": BUCKET,
            "object": object_id,
        }))
        .expect("Failed `stream.upload` request");

    assert_eq!(ack_response.janus, "ack");

    // When upload finishes expect the second response.
    let response: UploadResponse = janus_client
        .wait_for_response(&ack_response.transaction, Duration::from_secs(30))
        .expect("Failed to wait for upload response");

    assert_eq!(response.janus, "event");
    assert_eq!(response.plugindata.data.status, 200);
    drop(janus_client);

    // Download the full record file from S3 and delete it from there.
    let temp_dir = TempDir::new().expect("Failed to create temp file");
    let record_path = temp_dir.into_path().join(&object_id);

    s3_client
        .get_object(BUCKET, &object_id, &record_path)
        .expect("Failed to download record from S3");

    s3_client
        .delete_object(BUCKET, &object_id)
        .expect("Failed to delete record from S3");

    // Assert downloaded file duration to ensure that it's really concatenated video.
    let duration = discover_duration(&record_path).expect("Failed to get video duration");
    assert_eq!(duration, 3633);

    // Assert part timestamps from the response after removing the file from S3 to keep it clean
    // even in case of failure.
    assert_eq!(response.plugindata.data.id, recording.id);
    assert_eq!(response.plugindata.data.started_at, 1560489452218);

    assert_eq!(
        response.plugindata.data.time,
        vec![(0, 1633), (8682, 10682)]
    );
}

// Test recording directory with some video files. Gets deleted after the test.
struct TestRecording {
    id: String,
    path: PathBuf,
}

impl TestRecording {
    fn new() -> Result<Self> {
        let mut rng = rand::thread_rng();
        let id = rng.gen::<u64>().to_string();
        let path = Path::new(RECORDINGS_DIR).join(&id);
        fs::create_dir(&path)?;
        Self::copy_test_files(&path)?;
        Ok(Self { id, path })
    }

    fn copy_test_files(destination_path: &PathBuf) -> Result<()> {
        for entry in fs::read_dir(TEST_RECORDING_PATH)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("mkv") {
                let name = path
                    .file_name()
                    .ok_or_else(|| format_err!("Failed to get file name"))?;

                fs::copy(&path, &destination_path.join(&name))?;
            }
        }

        Ok(())
    }
}

impl Drop for TestRecording {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_dir_all(&self.path) {
            panic!("Failed to cleanup test recording: {}", err);
        }
    }
}

// A wrapper for S3 client with more concise API for readability.
struct S3ClientWrapper {
    client: S3Client,
}

impl S3ClientWrapper {
    fn new() -> Result<Self> {
        let region = Region::Custom {
            name: env::var("APP_UPLOADING__REGION")?,
            endpoint: env::var("APP_UPLOADING__ENDPOINT")?,
        };

        let access_key_id = env::var("APP_UPLOADING__ACCESS_KEY_ID")?;
        let secret_access_key = env::var("APP_UPLOADING__SECRET_ACCESS_KEY")?;

        let client = S3Client::new_with(
            HttpClient::new()?,
            StaticProvider::new_minimal(access_key_id, secret_access_key),
            region,
        );

        Ok(Self { client })
    }

    fn get_object<P>(&self, bucket: &str, object: &str, destination: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let request = GetObjectRequest {
            bucket: bucket.to_owned(),
            key: object.to_owned(),
            ..Default::default()
        };

        let mut resp = self.client.get_object(request).sync()?;
        let body = resp.body.take().context("Missing response body")?;

        let mut target = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .context("Failed to open destination file")?;

        io::copy(&mut body.into_blocking_read(), &mut target)
            .context("Failed to write downloaded file")?;

        Ok(())
    }

    fn delete_object(&self, bucket: &str, object: &str) -> Result<()> {
        let request = DeleteObjectRequest {
            bucket: bucket.to_owned(),
            key: object.to_owned(),
            ..Default::default()
        };

        self.client.delete_object(request).sync()?;
        Ok(())
    }
}

// Helper function for discovering video file duration using gstreamer discoverer.
fn discover_duration(path: &PathBuf) -> Result<u64> {
    gstreamer_pbutils::Discoverer::new(gst::ClockTime::from_seconds(DISCOVERER_TIMEOUT))?
        .discover_uri(&format!("file://{}", path.as_path().to_string_lossy()))?
        .get_duration()
        .mseconds()
        .ok_or_else(|| format_err!("Empty duration"))
}

// JSON responses
#[derive(Deserialize)]
struct UploadAckResponse {
    janus: String,
    transaction: Transaction,
}

#[derive(Deserialize)]
struct UploadResponse {
    janus: String,
    plugindata: UploadResponsePluginData,
}

#[derive(Deserialize)]
struct UploadResponsePluginData {
    data: UploadResponsePluginDataData,
}

#[derive(Deserialize)]
struct UploadResponsePluginDataData {
    id: String,
    status: usize,
    started_at: u64,
    time: Vec<(u64, u64)>,
}
