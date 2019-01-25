use janus::sdp;

#[derive(Debug, Clone, Copy)]
pub enum VideoCodec {
    H264,
}

impl VideoCodec {
    pub fn name(&self) -> &str {
        match self {
            VideoCodec::H264 => "H264",
        }
    }
}

impl From<VideoCodec> for sdp::VideoCodec {
    fn from(codec: VideoCodec) -> Self {
        match codec {
            VideoCodec::H264 => sdp::VideoCodec::H264,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AudioCodec {
    OPUS,
}

impl AudioCodec {
    pub fn name(&self) -> &str {
        match self {
            AudioCodec::OPUS => "OPUS",
        }
    }
}

impl From<AudioCodec> for sdp::AudioCodec {
    fn from(codec: AudioCodec) -> Self {
        match codec {
            AudioCodec::OPUS => sdp::AudioCodec::Opus,
        }
    }
}
