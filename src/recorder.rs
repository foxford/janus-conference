use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use failure::{err_msg, Error};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_base::BaseSrcExt;

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
struct RecorderMsg {
    buf: gst::buffer::Buffer,
    is_video: bool,
}

#[derive(Debug)]
pub struct Recorder {
    sender: mpsc::Sender<RecorderMsg>,
}

unsafe impl Sync for Recorder {}

impl Recorder {
    pub fn new(save_directory: &Path, video_codec: VideoCodec, audio_codec: AudioCodec) -> Self {
        let (sender, recv): (mpsc::Sender<RecorderMsg>, _) = mpsc::channel();

        let pipeline = gst::Pipeline::new(None);
        let mp4mux =
            gst::ElementFactory::make("mp4mux", None).expect("Failed to create GStreamer mp4mux");
        let filesink = gst::ElementFactory::make("filesink", None)
            .expect("Failed to create GStreamer filesink");

        let (video_src, video_rtpdepay, video_codec) = Self::setup_video_elements(video_codec);
        let video_queue =
            gst::ElementFactory::make("queue", None).expect("Failed to create queue for video");

        let (audio_src, audio_rtpdepay, audio_codec) = Self::setup_audio_elements(audio_codec);
        let audio_queue =
            gst::ElementFactory::make("queue", None).expect("Failed to create queue for audio");

        {
            let elems = [
                &video_src.upcast_ref(),
                &video_rtpdepay,
                &video_codec,
                &video_queue,
                &audio_src.upcast_ref(),
                &audio_rtpdepay,
                &audio_codec,
                &audio_queue,
                &mp4mux,
                &filesink,
            ];

            pipeline
                .add_many(&elems)
                .expect("Failed to add elems to pipeline");

            let video_link = [
                &video_src.upcast_ref(),
                &video_rtpdepay,
                &video_codec,
                &video_queue,
            ];
            gst::Element::link_many(&video_link)
                .expect("Failed to link video elements in pipeline");

            let audio_link = [
                &audio_src.upcast_ref(),
                &audio_rtpdepay,
                &audio_codec,
                &audio_queue,
            ];
            gst::Element::link_many(&audio_link)
                .expect("Failed to link audio elements in pipeline");
        }

        mp4mux
            .link(&filesink)
            .expect("Failed to link mp4mux to filesink");

        let video_src_pad = video_queue
            .get_static_pad("src")
            .expect("Failed to get src pad on video src");
        let video_sink_pad = mp4mux
            .get_request_pad("video_%u")
            .expect("Failed to request video pad");
        let res = video_src_pad.link(&video_sink_pad);
        janus_info!("video - {:?}", res);

        let audio_src_pad = audio_queue
            .get_static_pad("src")
            .expect("Failed to get src pad on audio src");
        let audio_sink_pad = mp4mux
            .get_request_pad("audio_%u")
            .expect("Failed to request audio pad");
        let res = audio_src_pad.link(&audio_sink_pad);
        janus_info!("audio - {:?}", res);

        let mut path = Self::generate_record_path(save_directory);
        path.set_extension("mp4");
        let path = path.to_string_lossy();

        janus_info!("[CONFERENCE] Saving video to {}", path);

        filesink
            .set_property("location", &path.to_value())
            .expect("failed to set location prop on filesink?!");

        pipeline.set_state(gst::State::Playing);

        thread::spawn(move || {
            for msg in recv.iter() {
                let res = if msg.is_video {
                    video_src.push_buffer(msg.buf)
                } else {
                    audio_src.push_buffer(msg.buf)
                };
                if res != gst::FlowReturn::Ok {
                    janus_err!("[CONFERENCE] Error pushing buffer to AppSrc: {:?}", res);
                };
            }

            let res = video_src.end_of_stream();
            if res != gst::FlowReturn::Ok {
                janus_err!(
                    "[CONFERENCE] Error trying to finish video stream: {:?}",
                    res
                );
            }

            let res = audio_src.end_of_stream();
            if res != gst::FlowReturn::Ok {
                janus_err!(
                    "[CONFERENCE] Error trying to finish audio stream: {:?}",
                    res
                );
            }

            let eos_ev = gst::Event::new_eos().build();
            pipeline.send_event(eos_ev);

            mp4mux.release_request_pad(&audio_sink_pad);
            mp4mux.release_request_pad(&video_sink_pad);

            let bus = pipeline.get_bus().unwrap();
            bus.set_sync_handler(|_bus, _msg| gst::BusSyncReply::Pass);
            thread::sleep(::std::time::Duration::from_secs(5));

            let res = pipeline.set_state(gst::State::Null);
            assert_ne!(res, gst::StateChangeReturn::Failure);

            janus_info!("[CONFERENCE] End of record");
        });

        Self { sender }
    }

    pub fn record_packet(&self, buf: &[u8], is_video: bool) -> Result<(), Error> {
        let buf = Self::wrap_buf(buf)?;
        let msg = RecorderMsg { buf, is_video };

        self.sender.send(msg).map_err(|err| Error::from(err))
    }

    fn wrap_buf(buf: &[u8]) -> Result<gst::Buffer, Error> {
        let mut gbuf =
            gst::buffer::Buffer::with_size(buf.len()).ok_or(err_msg("Failed to init GBuffer"))?;

        {
            let gbuf = gbuf.get_mut().unwrap();
            gbuf.copy_from_slice(0, buf).map_err(|copied| {
                format_err!(
                    "Failed to copy buf into GBuffer: copied {} out of {} bytes",
                    copied,
                    buf.len()
                )
            })?;
        }

        Ok(gbuf)
    }

    fn generate_record_path(dir: &Path) -> PathBuf {
        let filename = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string();
        let mut path = dir.to_path_buf();
        path.push(filename);

        path
    }

    fn setup_video_elements(codec: VideoCodec) -> (gst_app::AppSrc, gst::Element, gst::Element) {
        let src =
            gst::ElementFactory::make("appsrc", None).expect("Failed to create GStreamer AppSrc");

        let rtpdepay = match codec {
            VideoCodec::H264 => gst::ElementFactory::make("rtph264depay", None)
                .expect("Failed to create GStreamer rtph264depay"),
        };

        let codec_elem = match codec {
            VideoCodec::H264 => gst::ElementFactory::make("h264parse", None)
                .expect("Failed to create GStreamer h264parse"),
        };

        let caps = gst::Caps::new_simple(
            "application/x-rtp",
            &[
                ("media", &"video"),
                ("encoding-name", &codec.name()),
                ("payload", &96),
                ("clock-rate", &90000),
            ],
        );

        let src = src
            .downcast::<gst_app::AppSrc>()
            .expect("Failed downcast: Element -> AppSrc");

        src.set_caps(Some(&caps));
        src.set_stream_type(gst_app::AppStreamType::Stream);
        src.set_format(gst::Format::Time);
        src.set_live(true);
        src.set_do_timestamp(true);

        (src, rtpdepay, codec_elem)
    }

    fn setup_audio_elements(codec: AudioCodec) -> (gst_app::AppSrc, gst::Element, gst::Element) {
        let src =
            gst::ElementFactory::make("appsrc", None).expect("Failed to create GStreamer AppSrc");

        let rtpdepay = match codec {
            AudioCodec::OPUS => gst::ElementFactory::make("rtpopusdepay", None)
                .expect("Failed to create GStreamer rtpopusdepay"),
        };

        let codec_elem = match codec {
            AudioCodec::OPUS => gst::ElementFactory::make("opusdec", None)
                .expect("Failed to create GStreamer opusdec"),
        };

        let caps = gst::Caps::new_simple(
            "application/x-rtp",
            &[
                ("media", &"audio"),
                ("encoding-name", &codec.name()),
                ("payload", &111),
                ("clock-rate", &48000),
            ],
        );

        let src = src
            .downcast::<gst_app::AppSrc>()
            .expect("Failed downcast: Element -> AppSrc");

        src.set_caps(Some(&caps));
        src.set_stream_type(gst_app::AppStreamType::Stream);
        src.set_format(gst::Format::Time);
        src.set_live(true);
        src.set_do_timestamp(true);

        (src, rtpdepay, codec_elem)
    }
}
