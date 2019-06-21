// Publisher stream recording functional test.
//
// 1. Setup GStreamer pipeline that uses webrtcbin element to stream a test video.
// 2. Make an SDP offer and send it with the `stream.create` request with MQTT client.
// 3. Set the SDP answer from JSEP part of the response from Janus as the remote description.
// 4. Send local trickle ICE candidates to Janus. After that RTP packets should start streaming.
// 5. Wait for some time for the pipeline to stream then shut it down.
// 6. Check out the recordings folder: there should be a valid MKV file.
// 7. Cleanup: delete the test recording.

use std::fs;
use std::thread;
use std::time::Duration;

use failure::Error;
use serde_json::json;

use crate::support::conference_plugin_api_responses::CreateResponse;
use crate::support::janus_client::JanusClient;
use crate::support::publisher_pipeline::{Message as PublisherMessage, PublisherPipeline};
use crate::support::test_stream::TestStream;

#[test]
fn it_records_video_from_publisher() -> Result<(), Error> {
    // Setup.
    gst::init()?;
    let stream = TestStream::new();
    let (pipeline, rx) = PublisherPipeline::new()?;
    let mut client = JanusClient::new()?;

    // Handle messages from publisher pipeline.
    loop {
        match rx.try_iter().next() {
            None => break,
            Some(PublisherMessage::Error(err)) => return Err(err),
            Some(PublisherMessage::LocalSessionDescription(sdp_offer)) => {
                let response: CreateResponse = client.request_message(
                    json!({"method": "stream.create", "id": stream.id()}),
                    Some(json!({"type": "offer", "sdp": sdp_offer})),
                    Duration::from_secs(5),
                )?;

                assert_eq!(response.janus, "event");
                assert_eq!(response.plugindata.data.status, 200);
                assert_eq!(response.jsep.r#type, "answer");

                // Set SDP answer and send local ICE candidates.
                pipeline.set_sdp_answer(&response.jsep.sdp)?;
            }
            Some(PublisherMessage::LocalIceCandidate {
                sdp_mline_index: _,
                candidate,
            }) => {
                // Substitute actual SDP m-line index with 0 because Janus ignores candidates
                // with m-line index > 0 sent after the offer processing when bundling.
                // https://github.com/meetecho/janus-gateway/blob/553c2526ad7616b016f7f8a0a2a541b235d27c96/ice.c#L745-L749
                client.trickle_ice_candidate(0, &candidate)?;
            }
        }
    }

    // Wait some time to stream video.
    thread::sleep(Duration::from_secs(5));
    drop(pipeline);
    drop(client);

    // Check out the presence of the recorded video.
    let recordings = stream.recordings()?;
    assert_eq!(recordings.len(), 1);

    // Empty MKV file with just a header takes 366 bytes. Our recording should be bigger.
    let metadata = fs::metadata(&recordings[0])?;
    assert!(metadata.len() > 366);

    Ok(())
}
