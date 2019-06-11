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
        }
    }

    pub fn make(&self) -> gst::Element {
        match gst::ElementFactory::make(self.name(), None) {
            Some(elem) => elem,
            None => panic!("Failed to create GStreamer element {}", self.name()),
        }
    }
}
