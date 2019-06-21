use std::sync::{mpsc, Arc, Mutex};

use failure::{err_msg, Error};
use gst::prelude::*;

/// A GStreamer pipeline that streams test video & audio to WebRTC.
const PUBLISHER_PIPELINE: &str = r#"
    webrtcbin name=webrtcbin bundle-policy=max-bundle

    videotestsrc is-live=true pattern=ball !
        videoconvert !
        queue !
        x264enc tune=zerolatency speed-preset=ultrafast !
        rtph264pay !
        queue !
        application/x-rtp, media=video, encoding-name=H264, payload=97 !
        webrtcbin.
    
    audiotestsrc is-live=true wave=red-noise !
        audioconvert !
        audioresample !
        queue !
        opusenc !
        rtpopuspay !
        queue !
        application/x-rtp, media=audio, encoding-name=OPUS, payload=96 !
        webrtcbin.
"#;

pub enum Message {
    Error(Error),
    LocalSessionDescription(String),
    LocalIceCandidate {
        sdp_mline_index: u32,
        candidate: String,
    },
}

pub struct PublisherPipeline {
    pipeline: gst::Pipeline,
}

impl PublisherPipeline {
    pub fn new() -> Result<(Self, mpsc::Receiver<Message>), Error> {
        let pipeline = gst::parse_launch(&PUBLISHER_PIPELINE)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| err_msg("Failed to cast pipeline"))?;

        let this = Self { pipeline };
        let webrtcbin = this.webrtcbin()?;

        // Messages channel to talk to the caller.
        let (tx, rx) = mpsc::channel();
        let tx = Arc::new(Mutex::new(tx));
        let tx_clone = tx.clone();

        // On local ICE candidate discovery send it to the messages channel so the caller
        // could send it to Janus.
        webrtcbin.connect("on-ice-candidate", false, move |values| {
            Self::handle_gst_callback_result(&tx, move || {
                Ok(Some(Message::LocalIceCandidate {
                    sdp_mline_index: Self::cast_value::<u32>(values, 1)?,
                    candidate: Self::cast_value::<String>(values, 2)?,
                }))
            })
        })?;

        // When webrtcbin asks for SDP negotiation ask it to create the SDP offer
        // and send it to the messages channel.
        webrtcbin.connect("on-negotiation-needed", false, move |values| {
            Self::handle_gst_callback_result(&tx_clone, || {
                let webrtcbin = Self::cast_value::<gst::Element>(values, 0)?;
                let webrtcbin_clone = webrtcbin.clone();
                let tx_clone = tx_clone.clone();

                // Called when the offer is created by webrtcbin.
                let create_offer_promise = gst::Promise::new_with_change_func(move |promise| {
                    Self::handle_gst_callback_result(&tx_clone, move || {
                        // Get the offer from the promise result and cast it.
                        if promise.wait() != gst::PromiseResult::Replied {
                            return Err(format_err!("Bad create-offer promise result"));
                        };

                        let offer = promise
                            .get_reply()
                            .ok_or_else(|| err_msg("Empty reply"))?
                            .get_value("offer")
                            .ok_or_else(|| err_msg("Failed to get offer from promise"))?
                            .get::<gst_webrtc::WebRTCSessionDescription>()
                            .ok_or_else(|| err_msg("Failed to cast SDP offer"))?;

                        // Set the offer as the local session description.
                        webrtcbin_clone
                            .emit("set-local-description", &[&offer, &None::<gst::Promise>])?;

                        let sdp = offer
                            .get_sdp()
                            .as_text()
                            .ok_or_else(|| err_msg("Failed to get SDP offer"))?;

                        // Send it to the messages channel so the caller could send it to Janus.
                        Ok(Some(Message::LocalSessionDescription(sdp)))
                    });
                });

                webrtcbin.emit(
                    "create-offer",
                    &[&None::<gst::Structure>, &create_offer_promise],
                )?;

                Ok(None)
            })
        })?;

        this.pipeline.set_state(gst::State::Playing)?;
        Ok((this, rx))
    }

    fn cast_value<'a, R>(values: &'a [gst::Value], index: usize) -> Result<R, Error>
    where
        for<'b> R: 'a + gst::glib::value::FromValueOptional<'b>,
    {
        values
            .get(index)
            .ok_or_else(|| format_err!("Failed to get value {}", index))?
            .get::<R>()
            .ok_or_else(|| format_err!("Failed to cast value {}", index))
    }

    // Convenience wrapper for error handling inside GStreamer callbacks.
    fn handle_gst_callback_result<F>(
        tx: &Mutex<mpsc::Sender<Message>>,
        callback: F,
    ) -> Option<gst::Value>
    where
        F: FnOnce() -> Result<Option<Message>, Error>,
    {
        // Extract the message from the callback result if present.
        // If an error happened inside the callback, convert it to the Error message to send it
        // to the caller so it could fail the test properly.
        let maybe_message = match callback() {
            Ok(None) => None,
            Ok(Some(message)) => Some(message),
            Err(err) => Some(Message::Error(err)),
        };

        if let Some(message) = maybe_message {
            let send_result = tx
                .lock()
                .map_err(|_| err_msg("Failed to get message channel lock"))
                .and_then(|tx| {
                    tx.send(message)
                        .map_err(|_| err_msg("Failed to send message"))
                });

            if let Err(_err) = send_result {
                eprintln!("Failed to send message from publisher pipeline callback");
            }
        }

        None
    }

    /// Returns webrtcbin element.
    fn webrtcbin(&self) -> Result<gst::Element, Error> {
        self.pipeline
            .get_by_name("webrtcbin")
            .ok_or_else(|| err_msg("Missing webrtcbin element in the pipeline"))
    }

    /// Set SDP answer from the remote peer and add ICE candidates from it.
    pub fn set_sdp_answer(&self, sdp_answer: &str) -> Result<(), Error> {
        let webrtcbin = self.webrtcbin()?;

        // Parse SDP string and build the answer.
        let sdp_message = gst_sdp::SDPMessage::parse_buffer(sdp_answer.as_bytes())
            .map_err(|_| err_msg("Failed to parse SDP answer"))?;

        let answer = gst_webrtc::WebRTCSessionDescription::new(
            gst_webrtc::WebRTCSDPType::Answer,
            sdp_message.to_owned(),
        );

        // Set the answer as the remote description.
        let promise = gst::Promise::new();
        webrtcbin.emit("set-remote-description", &[&answer, &promise])?;
        promise.wait();

        // Add ICE candidates from the SDP answer: webrtcbin doesn't do it automatically when
        // setting the remote description.
        for (sdp_mline_index, media) in (0..sdp_message.medias_len()).zip(sdp_message.medias()) {
            for attribute in media.attributes() {
                if attribute.key() == "candidate" {
                    if let Some(candidate) = attribute.value() {
                        let candidate = format!("a=candidate:{}", &candidate);
                        webrtcbin.emit("add-ice-candidate", &[&sdp_mline_index, &candidate])?;
                    }
                }
            }
        }

        Ok(())
    }
}

impl Drop for PublisherPipeline {
    fn drop(&mut self) {
        if let Err(err) = self.pipeline.set_state(gst::State::Null) {
            eprintln!("Failed to stop publisher pipeline: {}", err);
        }
    }
}
