use std::ffi::CString;
use std::os::raw::c_int;

use anyhow::{bail, Context, Result};
use janus::sdp::{AudioCodec, MediaDirection, MediaType, OfferAnswerParameters, Sdp, VideoCodec};
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

        // Set video bitrate
        if let Some(bitrate) = app!()?.config.constraint.publisher.bitrate {
            Self::set_publisher_bitrate_constraint(jsep_offer, &answer_sdp, bitrate)?;
        }

        janus_verb!("[CONFERENCE] answer: {:?}", answer_sdp);
        let answer = Jsep::Answer { sdp: answer_sdp };
        Ok(Some(answer))
    }

    fn set_publisher_bitrate_constraint(
        jsep_offer: &JsonValue,
        answer_sdp: &Sdp,
        bitrate: u32,
    ) -> Result<()> {
        let m_lines = match answer_sdp.get_mlines().get_mut(&MediaType::JANUS_SDP_VIDEO) {
            None => return Ok(()),
            Some(m_lines) => m_lines,
        };

        let is_firefox = {
            let serialized_offer = jsep_offer.to_string();
            serialized_offer.contains("mozilla") || serialized_offer.contains("Mozilla")
        };

        let (b_name, b_value) = if is_firefox {
            // Use TIAS (bps) instead of AS (kbps) for the b= attribute, as explained here:
            // https://github.com/meetecho/janus-gateway/issues/1277#issuecomment-397677746
            // (taken from videoroom plugin)
            ("TIAS", bitrate)
        } else {
            ("AS", bitrate / 1000)
        };

        for m_line in m_lines {
            if m_line.direction == MediaDirection::JANUS_SDP_SENDRECV
                || m_line.direction == MediaDirection::JANUS_SDP_SENDONLY
            {
                m_line.b_name = CString::new(b_name.to_string())?.into_raw();
                m_line.b_value = b_value as c_int;
            }
        }

        Ok(())
    }
}
