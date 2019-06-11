use gstreamer as gst;
use janus::sdp;

use gst_elements::GstElement;

pub trait VideoCodec {
    const NAME: &'static str;
    const SDP_VIDEO_CODEC: sdp::VideoCodec;

    fn new_parse_elem() -> gst::Element;
    fn new_depay_elem() -> gst::Element;
    fn new_decode_elem() -> gst::Element;
    fn new_encode_elem() -> gst::Element;
}

#[derive(Debug, Clone, Copy)]
pub struct H264;

impl VideoCodec for H264 {
    const NAME: &'static str = "H264";
    const SDP_VIDEO_CODEC: sdp::VideoCodec = sdp::VideoCodec::H264;

    fn new_parse_elem() -> gst::Element {
        GstElement::H264Parse.make()
    }

    fn new_depay_elem() -> gst::Element {
        GstElement::RTPH264Depay.make()
    }

    fn new_decode_elem() -> gst::Element {
        GstElement::AVDecH264.make()
    }

    fn new_encode_elem() -> gst::Element {
        GstElement::X264Enc.make()
    }
}

pub trait AudioCodec {
    const NAME: &'static str;
    const SDP_AUDIO_CODEC: sdp::AudioCodec;

    fn new_parse_elem() -> gst::Element;
    fn new_depay_elem() -> gst::Element;
}

#[derive(Debug, Clone, Copy)]
pub struct OPUS;

impl AudioCodec for OPUS {
    const NAME: &'static str = "OPUS";
    const SDP_AUDIO_CODEC: sdp::AudioCodec = sdp::AudioCodec::Opus;

    fn new_parse_elem() -> gst::Element {
        GstElement::OpusParse.make()
    }

    fn new_depay_elem() -> gst::Element {
        GstElement::RTPOpusDepay.make()
    }
}
