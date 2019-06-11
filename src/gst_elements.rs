use gstreamer as gst;

pub enum GstElement {
    Queue,
    Filesink,
    AppSrc,
    MatroskaMux,
    OpusParse,
    RTPOpusDepay,
    H264Parse,
    RTPH264Depay,
    AVDecH264,
    VideoScale,
    VideoRate,
    VideoConvert,
    CapsFilter,
    X264Enc,
}

impl GstElement {
    pub fn name(&self) -> &str {
        match self {
            GstElement::Queue => "queue",
            GstElement::Filesink => "filesink",
            GstElement::AppSrc => "appsrc",
            GstElement::MatroskaMux => "matroskamux",
            GstElement::OpusParse => "opusparse",
            GstElement::RTPOpusDepay => "rtpopusdepay",
            GstElement::H264Parse => "h264parse",
            GstElement::RTPH264Depay => "rtph264depay",
            GstElement::AVDecH264 => "avdec_h264",
            GstElement::VideoScale => "videoscale",
            GstElement::VideoRate => "videorate",
            GstElement::VideoConvert => "videoconvert",
            GstElement::CapsFilter => "capsfilter",
            GstElement::X264Enc => "x264enc",
        }
    }

    pub fn make(&self) -> gst::Element {
        match gst::ElementFactory::make(self.name(), None) {
            Some(elem) => elem,
            None => panic!("Failed to create GStreamer element {}", self.name()),
        }
    }
}
