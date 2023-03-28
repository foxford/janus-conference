use std::ffi::CString;
use std::os::raw::c_int;

use anyhow::{bail, Context, Result};
use janus::sdp::{AudioCodec, MediaDirection, MediaType, OfferAnswerParameters, Sdp, VideoCodec};
use serde_json::Value as JsonValue;

use crate::{
    janus_rtp::{janus_rtp_extmap_audio_level, JANUS_RTP_EXTMAP_AUDIO_LEVEL},
    switchboard::StreamId,
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum Jsep {
    Offer { sdp: Sdp },
    Answer { sdp: Sdp },
}

impl Jsep {
    pub fn find_audio_ext_id(jsep: &JsonValue) -> Option<u32> {
        let ext_map_pat = "a=extmap:";
        jsep.get("sdp").and_then(|x| x.as_str()).and_then(|sdp| {
            sdp.lines()
                .find(|x| x.starts_with(ext_map_pat) && x.contains(JANUS_RTP_EXTMAP_AUDIO_LEVEL))
                .and_then(|extmap| extmap.split_once(' '))
                .and_then(|(ext_map, _)| ext_map[ext_map_pat.len()..].parse().ok())
        })
    }

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
            janus_rtp_extmap_audio_level().as_ptr(),
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

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::Jsep;

    #[test]
    fn test_get_audio_ext() {
        let jsep = "{
            \"type\": \"offer\",
            \"sdp\": \"v=0\\r\\no=- 6033301702498781727 2 IN IP4 127.0.0.1\\r\\ns=-\\r\\nt=0 0\\r\\na=extmap-allow-mixed\\r\\na=msid-semantic: WMS Gzu32nZNPO16h5KQkCFAVIfGTVR2Ob7HOzdP\\r\\na=group:BUNDLE 0 1\\r\\nm=audio 9 UDP/TLS/RTP/SAVPF 109\\r\\nc=IN IP4 0.0.0.0\\r\\na=rtpmap:109 opus/48000/2\\r\\na=fmtp:109 minptime=10;useinbandfec=1\\r\\na=rtcp:9 IN IP4 0.0.0.0\\r\\na=rtcp-fb:109 transport-cc\\r\\na=extmap:1 urn:ietf:params:rtp-hdrext:ssrc-audio-level\\r\\na=extmap:2 http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time\\r\\na=extmap:3 http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01\\r\\na=extmap:4 urn:ietf:params:rtp-hdrext:sdes:mid\\r\\na=extmap:5 urn:ietf:params:rtp-hdrext:sdes:rtp-stream-id\\r\\na=extmap:6 urn:ietf:params:rtp-hdrext:sdes:repaired-rtp-stream-id\\r\\na=setup:actpass\\r\\na=mid:0\\r\\na=msid:Gzu32nZNPO16h5KQkCFAVIfGTVR2Ob7HOzdP ef94afa9-cefc-477b-9569-0d7a18bc85d3\\r\\na=sendonly\\r\\na=ice-ufrag:X4HN\\r\\na=ice-pwd:hX24HQQimRinGUdmMPBectRw\\r\\na=fingerprint:sha-256 E2:69:0F:BA:EB:B9:8A:EB:B3:57:2A:DA:7E:54:E9:05:95:09:7C:B1:EE:46:7B:0A:65:6D:07:9E:6B:D9:B5:B4\\r\\na=ice-options:trickle\\r\\na=ssrc:2408778299 cname:Ga31llMWheOe/+Mo\\r\\na=ssrc:2408778299 msid:Gzu32nZNPO16h5KQkCFAVIfGTVR2Ob7HOzdP ef94afa9-cefc-477b-9569-0d7a18bc85d3\\r\\na=ssrc:2408778299 mslabel:Gzu32nZNPO16h5KQkCFAVIfGTVR2Ob7HOzdP\\r\\na=ssrc:2408778299 label:ef94afa9-cefc-477b-9569-0d7a18bc85d3\\r\\na=rtcp-mux\\r\\nm=video 9 UDP/TLS/RTP/SAVPF 120\\r\\nc=IN IP4 0.0.0.0\\r\\na=rtpmap:120 VP8/90000\\r\\na=rtcp:9 IN IP4 0.0.0.0\\r\\na=rtcp-fb:120 goog-remb\\r\\na=rtcp-fb:120 transport-cc\\r\\na=rtcp-fb:120 ccm fir\\r\\na=rtcp-fb:120 nack\\r\\na=rtcp-fb:120 nack pli\\r\\na=extmap:14 urn:ietf:params:rtp-hdrext:toffset\\r\\na=extmap:2 http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time\\r\\na=extmap:13 urn:3gpp:video-orientation\\r\\na=extmap:3 http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01\\r\\na=extmap:12 http://www.webrtc.org/experiments/rtp-hdrext/playout-delay\\r\\na=extmap:11 http://www.webrtc.org/experiments/rtp-hdrext/video-content-type\\r\\na=extmap:7 http://www.webrtc.org/experiments/rtp-hdrext/video-timing\\r\\na=extmap:8 http://www.webrtc.org/experiments/rtp-hdrext/color-space\\r\\na=extmap:4 urn:ietf:params:rtp-hdrext:sdes:mid\\r\\na=extmap:5 urn:ietf:params:rtp-hdrext:sdes:rtp-stream-id\\r\\na=extmap:6 urn:ietf:params:rtp-hdrext:sdes:repaired-rtp-stream-id\\r\\na=setup:actpass\\r\\na=mid:1\\r\\na=msid:Gzu32nZNPO16h5KQkCFAVIfGTVR2Ob7HOzdP fdcae036-4e04-48ae-a76f-0236f990e418\\r\\na=sendonly\\r\\na=ice-ufrag:X4HN\\r\\na=ice-pwd:hX24HQQimRinGUdmMPBectRw\\r\\na=fingerprint:sha-256 E2:69:0F:BA:EB:B9:8A:EB:B3:57:2A:DA:7E:54:E9:05:95:09:7C:B1:EE:46:7B:0A:65:6D:07:9E:6B:D9:B5:B4\\r\\na=ice-options:trickle\\r\\na=ssrc:4184122499 cname:Ga31llMWheOe/+Mo\\r\\na=ssrc:4184122499 msid:Gzu32nZNPO16h5KQkCFAVIfGTVR2Ob7HOzdP fdcae036-4e04-48ae-a76f-0236f990e418\\r\\na=ssrc:4184122499 mslabel:Gzu32nZNPO16h5KQkCFAVIfGTVR2Ob7HOzdP\\r\\na=ssrc:4184122499 label:fdcae036-4e04-48ae-a76f-0236f990e418\\r\\na=ssrc:2418400294 cname:Ga31llMWheOe/+Mo\\r\\na=ssrc:2418400294 msid:Gzu32nZNPO16h5KQkCFAVIfGTVR2Ob7HOzdP fdcae036-4e04-48ae-a76f-0236f990e418\\r\\na=ssrc:2418400294 mslabel:Gzu32nZNPO16h5KQkCFAVIfGTVR2Ob7HOzdP\\r\\na=ssrc:2418400294 label:fdcae036-4e04-48ae-a76f-0236f990e418\\r\\na=ssrc-group:FID 4184122499 2418400294\\r\\na=rtcp-mux\\r\\na=rtcp-rsize\\r\\n\"
        }";
        let value: Value = serde_json::from_str(jsep).unwrap();

        assert_eq!(Jsep::find_audio_ext_id(&value), Some(1));
    }
}
