use anyhow::{bail, Context, Result};
use janus::sdp::{AudioCodec, OfferAnswerParameters, Sdp, VideoCodec};
use serde_json::Value as JsonValue;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum Jsep {
    Offer { sdp: Sdp },
    Answer { sdp: Sdp },
}

impl Jsep {
    /// Parses JSEP SDP offer and returns the answer.
    pub fn negotiate(jsep_offer: &JsonValue) -> Result<Option<Self>> {
        let offer = serde_json::from_value::<Jsep>(jsep_offer.clone())
            .context("Failed to deserialize JSEP")?;

        let offer_sdp = match offer {
            Jsep::Offer { ref sdp } => sdp,
            Jsep::Answer { .. } => bail!("Expected JSEP offer, got answer"),
        };

        janus_verb!("[CONFERENCE] offer: {:?}", offer_sdp);

        let answer_sdp = answer_sdp!(
            offer_sdp,
            OfferAnswerParameters::AudioCodec,
            AudioCodec::Opus.to_cstr().as_ptr(),
            OfferAnswerParameters::VideoCodec,
            VideoCodec::H264.to_cstr().as_ptr(),
        );

        janus_verb!("[CONFERENCE] answer: {:?}", answer_sdp);
        let answer = Jsep::Answer { sdp: answer_sdp };
        Ok(Some(answer))
    }
}
