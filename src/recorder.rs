use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use failure::{err_msg, Error};
use glib;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_base::BaseSrcExt;

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

    pub fn new_parse_elem(&self) -> gst::Element {
        match self {
            VideoCodec::H264 => GstElement::H264Parse.new(),
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

    pub fn new_parse_elem(&self) -> gst::Element {
        match self {
            AudioCodec::OPUS => GstElement::OpusParse.new(),
        }
    }
}

const MKV: &'static str = "mkv";

#[derive(Debug)]
struct RecorderMsg {
    buf: gst::buffer::Buffer,
    is_video: bool,
}

#[derive(Debug)]
pub struct Recorder {
    sender: mpsc::Sender<RecorderMsg>,
    room_id: String,
    video_codec: VideoCodec,
    audio_codec: AudioCodec,
}

unsafe impl Sync for Recorder {}

impl Recorder {
    pub fn new(room_id: &str, video_codec: VideoCodec, audio_codec: AudioCodec) -> Self {
        let (sender, recv): (mpsc::Sender<RecorderMsg>, _) = mpsc::channel();

        // Self::setup_recording(room_id, video_codec, audio_codec, recv);

        Self {
            sender,
            room_id: String::from(room_id),
            video_codec,
            audio_codec,
        }
    }

    pub fn record_packet(&self, buf: &[u8], is_video: bool) -> Result<(), Error> {
        let buf = Self::wrap_buf(buf)?;
        let msg = RecorderMsg { buf, is_video };

        self.sender.send(msg).map_err(Error::from)
    }

    pub fn finish_record(&self) -> Result<(), Error> {
        /*
        GStreamer pipeline we creating here:

            filesrc location=1545122937.mkv ! matroskademux name=demux0
            demux0.video_0 ! h264parse ! v.
            demux0.audio_0 ! opusparse ! a.

            ...

            concat name=v ! queue ! matroskamux name=mux
            concat name=a ! queue ! mux.audio_0

            mux. ! filesink location=concat.mkv
        */

        let mux = GstElement::MatroskaMux.new();

        let filesink = GstElement::Filesink.new();
        let location = Self::generate_record_path(&self.room_id, Some(String::from("full")), MKV);
        let location = location.to_string_lossy();

        janus_info!("[CONFERENCE] Saving full record to {}", location);

        filesink.set_property("location", &location.to_value())?;

        let concat_video = GstElement::Concat.new();
        let queue_video = GstElement::Queue.new();

        let concat_audio = GstElement::Concat.new();
        let queue_audio = GstElement::Queue.new();

        let pipeline = gst::Pipeline::new(None);

        {
            pipeline.add_many(&[
                &mux,
                &filesink,
                &concat_video,
                &queue_video,
                &concat_audio,
                &queue_audio,
            ])?;
        }

        mux.link(&filesink)?;

        concat_video.link(&queue_video)?;
        concat_audio.link(&queue_audio)?;

        let video_src_pad = queue_video
            .get_static_pad("src")
            .expect("Failed to get src pad for video");
        let video_sink_pad = mux
            .get_request_pad("video_%u")
            .expect("Failed to request video pad from mux");
        let res = video_src_pad.link(&video_sink_pad);
        assert_eq!(res, gst::PadLinkReturn::Ok);

        let audio_src_pad = queue_audio
            .get_static_pad("src")
            .expect("Failed to get src pad for audio");
        let audio_sink_pad = mux
            .get_request_pad("audio_%u")
            .expect("Failed to request audio pad from mux");
        let res = audio_src_pad.link(&audio_sink_pad);
        assert_eq!(res, gst::PadLinkReturn::Ok);

        let parts = fs::read_dir(&self.room_id)?;

        for file in parts {
            let filesrc = GstElement::Filesrc.new();
            filesrc.set_property("location", &file?.path().to_string_lossy().to_value())?;
            pipeline.add(&filesrc)?;

            let demux = GstElement::MatroskaDemux.new();
            pipeline.add(&demux)?;

            filesrc.link(&demux)?;

            let video_parse = self.video_codec.new_parse_elem();
            pipeline.add(&video_parse)?;

            // let video_src_pad = demux
            //     .get_static_pad("video_0")
            //     .expect("Failed to get src pad for video");
            // let video_sink_pad = video_parse
            //     .get_static_pad("sink")
            //     .expect("Failed to request video pad sink");
            // let res = video_src_pad.link(&video_sink_pad);
            // assert_eq!(res, gst::PadLinkReturn::Ok);

            video_parse.link(&concat_video)?;

            let audio_parse = self.audio_codec.new_parse_elem();
            pipeline.add(&audio_parse)?;

            // let audio_src_pad = demux
            //     .get_static_pad("audio_0")
            //     .expect("Failed to get src pad for audio");
            // let audio_sink_pad = video_parse
            //     .get_static_pad("sink")
            //     .expect("Failed to request audio pad sink");
            // let res = audio_src_pad.link(&audio_sink_pad);
            // assert_eq!(res, gst::PadLinkReturn::Ok);

            audio_parse.link(&concat_audio)?;
        }

        let res = pipeline.set_state(gst::State::Playing);
        assert_ne!(res, gst::StateChangeReturn::Failure);

        let bus = pipeline.get_bus().unwrap();

        let main_loop = glib::MainLoop::new(None, false);
        let main_loop_clone = main_loop.clone();
        bus.add_watch(move |_bus, msg| {
            if let gst::MessageView::Eos(..) = msg.view() {
                main_loop_clone.quit();
            }

            glib::Continue(true)
        });

        main_loop.run();

        let res = pipeline.set_state(gst::State::Null);
        assert_ne!(res, gst::StateChangeReturn::Failure);

        Ok(())
    }

    fn wrap_buf(buf: &[u8]) -> Result<gst::Buffer, Error> {
        let mut gbuf = gst::buffer::Buffer::with_size(buf.len())
            .ok_or_else(|| err_msg("Failed to init GBuffer"))?;

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

    fn setup_recording(
        room_id: &str,
        video_codec: VideoCodec,
        audio_codec: AudioCodec,
        recv: mpsc::Receiver<RecorderMsg>,
    ) {
        let pipeline = gst::Pipeline::new(None);
        let matroskamux = GstElement::MatroskaMux.new();

        let filesink = GstElement::Filesink.new();
        let path = Self::generate_record_path(room_id, None, MKV);
        let path = path.to_string_lossy();

        janus_info!("[CONFERENCE] Saving video to {}", path);

        filesink
            .set_property("location", &path.to_value())
            .expect("failed to set location prop on filesink?!");

        let (video_src, video_rtpdepay, video_codec) = Self::setup_video_elements(video_codec);
        let video_queue = GstElement::Queue.new();

        let (audio_src, audio_rtpdepay, audio_codec) = Self::setup_audio_elements(audio_codec);
        let audio_queue = GstElement::Queue.new();

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
                &matroskamux,
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

        matroskamux
            .link(&filesink)
            .expect("Failed to link matroskamux to filesink");

        let video_src_pad = video_queue
            .get_static_pad("src")
            .expect("Failed to get src pad on video src");
        let video_sink_pad = matroskamux
            .get_request_pad("video_%u")
            .expect("Failed to request video pad");
        let res = video_src_pad.link(&video_sink_pad);
        assert_eq!(res, gst::PadLinkReturn::Ok);

        let audio_src_pad = audio_queue
            .get_static_pad("src")
            .expect("Failed to get src pad on audio src");
        let audio_sink_pad = matroskamux
            .get_request_pad("audio_%u")
            .expect("Failed to request audio pad");
        let res = audio_src_pad.link(&audio_sink_pad);
        assert_eq!(res, gst::PadLinkReturn::Ok);

        let res = pipeline.set_state(gst::State::Playing);
        assert_ne!(res, gst::StateChangeReturn::Failure);

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

            let main_loop = glib::MainLoop::new(None, false);

            let eos_ev = gst::Event::new_eos().build();
            pipeline.send_event(eos_ev);

            let bus = pipeline.get_bus().unwrap();
            let main_loop_clone = main_loop.clone();
            bus.add_watch(move |_bus, msg| {
                if let gst::MessageView::Eos(..) = msg.view() {
                    main_loop_clone.quit();
                }

                glib::Continue(true)
            });

            main_loop.run();

            matroskamux.release_request_pad(&audio_sink_pad);
            matroskamux.release_request_pad(&video_sink_pad);

            let res = pipeline.set_state(gst::State::Null);
            assert_ne!(res, gst::StateChangeReturn::Failure);

            bus.remove_watch();

            janus_info!("[CONFERENCE] End of record");
        });
    }

    fn generate_record_path(room_id: &str, filename: Option<String>, extension: &str) -> PathBuf {
        let mut path = PathBuf::new();
        path.push(room_id);

        let filename = match filename {
            Some(filename) => filename,
            None => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .to_string(),
        };

        path.push(filename);
        path.set_extension(extension);

        path
    }

    fn setup_video_elements(codec: VideoCodec) -> (gst_app::AppSrc, gst::Element, gst::Element) {
        let src = GstElement::AppSrc.new();

        let rtpdepay = match codec {
            VideoCodec::H264 => GstElement::RTPH264Depay.new(),
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

        (src, rtpdepay, codec.new_parse_elem())
    }

    fn setup_audio_elements(codec: AudioCodec) -> (gst_app::AppSrc, gst::Element, gst::Element) {
        let src = GstElement::AppSrc.new();

        let rtpdepay = match codec {
            AudioCodec::OPUS => GstElement::RTPOpusDepay.new(),
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

        (src, rtpdepay, codec.new_parse_elem())
    }
}

enum GstElement {
    Queue,
    Filesrc,
    Filesink,
    AppSrc,
    Concat,
    MatroskaMux,
    MatroskaDemux,
    OpusParse,
    RTPOpusDepay,
    H264Parse,
    RTPH264Depay,
}

impl GstElement {
    pub fn name(&self) -> &str {
        match self {
            GstElement::Queue => "queue",
            GstElement::Filesrc => "filesrc",
            GstElement::Filesink => "filesink",
            GstElement::AppSrc => "appsrc",
            GstElement::Concat => "concat",
            GstElement::MatroskaMux => "matroskamux",
            GstElement::MatroskaDemux => "matroskademux",
            GstElement::OpusParse => "opusparse",
            GstElement::RTPOpusDepay => "rtpopusdepay",
            GstElement::H264Parse => "h264parse",
            GstElement::RTPH264Depay => "rtph264depay",
        }
    }

    pub fn new(&self) -> gst::Element {
        match gst::ElementFactory::make(self.name(), None) {
            Some(elem) => elem,
            None => panic!("Failed to create GStreamer element {}", self.name()),
        }
    }
}
