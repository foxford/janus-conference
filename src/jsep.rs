use failure::Error;
use janus::sdp::{AudioCodec, OfferAnswerParameters, Sdp, VideoCodec};
use janus::{JanssonDecodingFlags, JanssonValue};

use crate::utils;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum Jsep {
    Offer { sdp: Sdp },
    Answer { sdp: Sdp },
}

impl Jsep {
    pub fn negotiate(jsep: &str) -> Result<Option<(Self, Self)>, Error> {
        let jsep = JanssonValue::from_str(jsep, JanssonDecodingFlags::empty())
            .map_err(|err| format_err!("Failed to parse JSEP: {}", err))?;

        let offer = utils::jansson_to_serde::<Jsep>(&jsep)
            .map_err(|err| format_err!("Failed to deserialize JSEP: {}", err))?;

        let offer_sdp = match offer {
            Jsep::Offer { ref sdp } => sdp,
            Jsep::Answer { .. } => bail!("Expected JSEP offer, got answer"),
        };

        janus_verb!("[CONFERENCE] offer: {:?}", offer_sdp);

        let answer_sdp = answer_sdp!(
            offer_sdp,
            OfferAnswerParameters::AudioCodec,
            VideoCodec::H264.to_cstr().as_ptr(),
            OfferAnswerParameters::VideoCodec,
            AudioCodec::Opus.to_cstr().as_ptr()
        );

        janus_verb!("[CONFERENCE] answer: {:?}", answer_sdp);
        let answer = Jsep::Answer { sdp: answer_sdp };
        Ok(Some((offer, answer)))
    }
}
