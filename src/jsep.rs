use std::ffi::CString;
use std::os::raw::c_int;

use anyhow::{bail, Context, Result};
use janus::sdp::{AudioCodec, MediaDirection, MediaType, OfferAnswerParameters, Sdp, VideoCodec};
use serde_json::Value as JsonValue;

const WRITER_DIRECTIONS: &[MediaDirection] = &[
    MediaDirection::JANUS_SDP_SENDRECV,
    MediaDirection::JANUS_SDP_SENDONLY,
];

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum Jsep {
    Offer { sdp: Sdp },
    Answer { sdp: Sdp },
}

impl Jsep {
    /// Parses JSEP SDP offer and returns the answer.
    pub fn negotiate(jsep_offer: &JsonValue) -> Result<Option<Self>> {
        let offer_sdp = Self::parse_offer_sdp(jsep_offer)?;
        verb!("SDP offer: {:?}", offer_sdp);

        let answer_sdp = answer_sdp!(
            offer_sdp,
            OfferAnswerParameters::AudioCodec,
            AudioCodec::Opus.to_cstr().as_ptr(),
            OfferAnswerParameters::VideoCodec,
            VideoCodec::Vp8.to_cstr().as_ptr(),
        );

        // Set video bitrate
        if let Some(bitrate) = app!()?.config.constraint.writer.bitrate {
            Self::set_writer_bitrate_constraint(jsep_offer, &answer_sdp, bitrate)?;
        }

        verb!("SDP answer: {:?}", answer_sdp);
        let answer = Jsep::Answer { sdp: answer_sdp };
        Ok(Some(answer))
    }

    fn parse_offer_sdp(jsep_offer: &JsonValue) -> Result<Sdp> {
        let offer = serde_json::from_value::<Jsep>(jsep_offer.clone())
            .context("Failed to deserialize JSEP")?;

        match offer {
            Jsep::Offer { sdp } => Ok(sdp),
            Jsep::Answer { .. } => bail!("Expected JSEP offer, got answer"),
        }
    }

    fn set_writer_bitrate_constraint(
        jsep_offer: &JsonValue,
        answer_sdp: &Sdp,
        bitrate: u32,
    ) -> Result<()> {
        let mut m_lines = answer_sdp.get_mlines();

        let video_m_lines = match m_lines.get_mut(&MediaType::JANUS_SDP_VIDEO) {
            None => return Ok(()),
            Some(video_m_lines) => video_m_lines,
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

        for m_line in video_m_lines {
            if WRITER_DIRECTIONS.contains(&m_line.direction) {
                m_line.b_name = CString::new(b_name.to_string())?.into_raw();
                m_line.b_value = b_value as c_int;
            }
        }

        Ok(())
    }
}
