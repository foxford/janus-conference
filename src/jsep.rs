use std::ffi::CString;
use std::os::raw::c_int;

use anyhow::{bail, Context, Result};
use janus::sdp::{AudioCodec, MediaDirection, MediaType, OfferAnswerParameters, Sdp, VideoCodec};
use serde_json::Value as JsonValue;

use crate::switchboard::StreamId;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum Jsep {
    Offer { sdp: Sdp },
    Answer { sdp: Sdp },
}

impl Jsep {
    /// Parses JSEP SDP offer and returns the answer.
    pub fn negotiate(jsep_offer: &JsonValue, stream_id: StreamId) -> Result<Option<Self>> {
        let offer = serde_json::from_value::<Jsep>(jsep_offer.clone())
            .context("Failed to deserialize JSEP")?;

        let offer_sdp = match offer {
            Jsep::Offer { ref sdp } => sdp,
            Jsep::Answer { .. } => bail!("Expected JSEP offer, got answer"),
        };

        verb!("SDP offer: {:?}", offer_sdp);

        let answer_sdp = answer_sdp!(
            offer_sdp,
            OfferAnswerParameters::AudioCodec,
            AudioCodec::Opus.to_cstr().as_ptr(),
            OfferAnswerParameters::VideoCodec,
            VideoCodec::Vp8.to_cstr().as_ptr(),
            OfferAnswerParameters::AcceptExtmap,
            CString::new("urn:ietf:params:rtp-hdrext:ssrc-audio-level")?.as_ptr(),
        );

        // Set video bitrate.
        let app = app!()?;

        let video_bitrate = app.switchboard.with_read_lock(|switchboard| {
            let writer_config = switchboard.writer_config(stream_id);
            Ok(writer_config.video_remb())
        })?;

        Self::set_publisher_bitrate_constraints(
            jsep_offer,
            &answer_sdp,
            video_bitrate,
            app.config.constraint.writer.audio_bitrate,
        )?;

        verb!("SDP answer: {:?}", answer_sdp);
        let answer = Jsep::Answer { sdp: answer_sdp };
        Ok(Some(answer))
    }

    fn set_publisher_bitrate_constraints(
        jsep_offer: &JsonValue,
        answer_sdp: &Sdp,
        video_bitrate: u32,
        audio_bitrate: u32,
    ) -> Result<()> {
        let mut m_lines = answer_sdp.get_mlines();

        let is_firefox = {
            let serialized_offer = jsep_offer.to_string();
            serialized_offer.contains("mozilla") || serialized_offer.contains("Mozilla")
        };

        let media_types_with_bitrates = [
            (MediaType::JANUS_SDP_VIDEO, video_bitrate),
            (MediaType::JANUS_SDP_AUDIO, audio_bitrate),
        ];

        for (media_type, bitrate) in &media_types_with_bitrates {
            if let Some(m_lines) = m_lines.get_mut(media_type) {
                let (b_name, b_value) = if is_firefox {
                    // Use TIAS (bps) instead of AS (kbps) for the b= attribute, as explained here:
                    // https://github.com/meetecho/janus-gateway/issues/1277#issuecomment-397677746
                    // (taken from videoroom plugin)
                    ("TIAS", *bitrate)
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
            }
        }

        Ok(())
    }
}
