#[macro_use]
extern crate failure;
extern crate gstreamer as gst;
extern crate gstreamer_pbutils as gst_pbutils;
extern crate gstreamer_sdp as gst_sdp;
extern crate gstreamer_webrtc as gst_webrtc;
extern crate rand;
extern crate rumqtt;
extern crate rusoto_core;
extern crate rusoto_s3;
extern crate s4;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate svc_agent;
extern crate tempfile;

mod support;

// Tests
mod recording_test;
mod upload_test;
