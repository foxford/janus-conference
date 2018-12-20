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

    pub fn new_parse_elem(self) -> gst::Element {
        match self {
            VideoCodec::H264 => GstElement::H264Parse.make(),
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

    pub fn new_parse_elem(self) -> gst::Element {
        match self {
            AudioCodec::OPUS => GstElement::OpusParse.make(),
        }
    }
}

const MKV: &str = "mkv";

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

        Self::setup_recording(room_id, video_codec, audio_codec, recv);

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

        let mux = GstElement::MatroskaMux.make();

        let filesink = GstElement::Filesink.make();
        let location = Self::generate_record_path(&self.room_id, Some(String::from("full")), MKV);
        let location = location.to_string_lossy();

        janus_info!("[CONFERENCE] Saving full record to {}", location);

        filesink.set_property("location", &location.to_value())?;

        let concat_video = GstElement::Concat.make();
        let queue_video = GstElement::Queue.make();

        let concat_audio = GstElement::Concat.make();
        let queue_audio = GstElement::Queue.make();

        let pipeline = gst::Pipeline::new(None);

        pipeline.add_many(&[
            &mux,
            &filesink,
            &concat_video,
            &queue_video,
            &concat_audio,
            &queue_audio,
        ])?;

        mux.link(&filesink)?;
        concat_video.link(&queue_video)?;
        concat_audio.link(&queue_audio)?;

        let video_sink_pad =
            Self::link_static_and_request_pads((&queue_video, "src"), (&mux, "video_%u"))?;

        let audio_sink_pad =
            Self::link_static_and_request_pads((&queue_audio, "src"), (&mux, "audio_%u"))?;

        let parts = fs::read_dir(&self.room_id)?;

        for file in parts {
            let filesrc = GstElement::Filesrc.make();
            filesrc.set_property("location", &file?.path().to_string_lossy().to_value())?;

            let demux = GstElement::MatroskaDemux.make();
            let video_parse = self.video_codec.new_parse_elem();
            let audio_parse = self.audio_codec.new_parse_elem();

            pipeline.add_many(&[&filesrc, &demux, &video_parse, &audio_parse])?;

            filesrc.link(&demux)?;
            video_parse.link(&concat_video)?;
            audio_parse.link(&concat_audio)?;

            demux.connect("pad-added", true, move |args| {
                let pad = args[1]
                    .get::<gst::Pad>()
                    .expect("Second argument is not a Pad");

                let sink = match pad.get_name().as_ref() {
                    "video_0" => Some(&video_parse),
                    "audio_0" => Some(&audio_parse),
                    _ => None,
                };

                if let Some(sink) = sink {
                    let sink_pad = sink
                        .get_static_pad("sink")
                        .expect("Failed to obtain pad sink");
                    let res = pad.link(&sink_pad);
                    assert!(res == gst::PadLinkReturn::Ok or res == gst::PadLinkReturn::WasLinked);
                }

                None
            })?;
        }

        let res = pipeline.set_state(gst::State::Playing);
        assert_ne!(res, gst::StateChangeReturn::Failure);

        Self::run_pipeline_to_completion(&pipeline);

        mux.release_request_pad(&video_sink_pad);
        mux.release_request_pad(&audio_sink_pad);

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
        let matroskamux = GstElement::MatroskaMux.make();

        let filesink = GstElement::Filesink.make();
        let path = Self::generate_record_path(room_id, None, MKV);
        let path = path.to_string_lossy();

        janus_info!("[CONFERENCE] Saving video to {}", path);

        filesink
            .set_property("location", &path.to_value())
            .expect("failed to set location prop on filesink?!");

        pipeline
            .add_many(&[&matroskamux, &filesink])
            .expect("Failed to add elems to pipeline");

        let (video_src, video_rtpdepay, video_codec) = Self::setup_video_elements(video_codec);
        let video_queue = GstElement::Queue.make();

        let (audio_src, audio_rtpdepay, audio_codec) = Self::setup_audio_elements(audio_codec);
        let audio_queue = GstElement::Queue.make();

        {
            let streams = [
                [
                    &video_src.upcast_ref(),
                    &video_rtpdepay,
                    &video_codec,
                    &video_queue,
                ],
                [
                    &audio_src.upcast_ref(),
                    &audio_rtpdepay,
                    &audio_codec,
                    &audio_queue,
                ],
            ];

            for elems in streams.iter() {
                pipeline
                    .add_many(elems)
                    .expect("Failed to add elems to pipeline");
                gst::Element::link_many(elems).expect("Failed to link elements in pipeline");
            }
        }

        matroskamux
            .link(&filesink)
            .expect("Failed to link matroskamux -> filesink");

        let video_sink_pad =
            Self::link_static_and_request_pads((&video_queue, "src"), (&matroskamux, "video_%u"))
                .expect("Failed to link video -> mux");

        let audio_sink_pad =
            Self::link_static_and_request_pads((&audio_queue, "src"), (&matroskamux, "audio_%u"))
                .expect("Failed to link audio -> mux");

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

            let eos_ev = gst::Event::new_eos().build();
            pipeline.send_event(eos_ev);

            Self::run_pipeline_to_completion(&pipeline);

            matroskamux.release_request_pad(&audio_sink_pad);
            matroskamux.release_request_pad(&video_sink_pad);

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

    fn init_app_src(caps: gst::Caps) -> gst_app::AppSrc {
        let src = GstElement::AppSrc
            .make()
            .downcast::<gst_app::AppSrc>()
            .expect("Failed downcast: Element -> AppSrc");

        src.set_caps(Some(&caps));
        src.set_stream_type(gst_app::AppStreamType::Stream);
        src.set_format(gst::Format::Time);
        src.set_live(true);
        src.set_do_timestamp(true);

        src
    }

    fn setup_video_elements(codec: VideoCodec) -> (gst_app::AppSrc, gst::Element, gst::Element) {
        let rtpdepay = match codec {
            VideoCodec::H264 => GstElement::RTPH264Depay.make(),
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

        let src = Self::init_app_src(caps);

        (src, rtpdepay, codec.new_parse_elem())
    }

    fn setup_audio_elements(codec: AudioCodec) -> (gst_app::AppSrc, gst::Element, gst::Element) {
        let rtpdepay = match codec {
            AudioCodec::OPUS => GstElement::RTPOpusDepay.make(),
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

        let src = Self::init_app_src(caps);

        (src, rtpdepay, codec.new_parse_elem())
    }

    fn run_pipeline_to_completion(pipeline: &gst::Pipeline) {
        let main_loop = glib::MainLoop::new(None, false);

        let bus = pipeline.get_bus().unwrap();
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

        bus.remove_watch();
    }

    fn link_static_and_request_pads(
        (static_elem, static_pad): (&gst::Element, &str),
        (request_elem, request_pad): (&gst::Element, &str),
    ) -> Result<gst::Pad, Error> {
        let src_pad = static_elem.get_static_pad(static_pad).ok_or_else(|| {
            format_err!(
                "Failed to obtain static pad {} for elem {}",
                static_pad,
                static_elem.get_name()
            )
        })?;

        let sink_pad = request_elem.get_request_pad(request_pad).ok_or_else(|| {
            format_err!(
                "Failed to request pad {} for elem {}",
                request_pad,
                request_elem.get_name()
            )
        })?;

        match src_pad.link(&sink_pad) {
            gst::PadLinkReturn::Ok => Ok(sink_pad),
            other_res => Err(format_err!("Failed to link pads: {:?}", other_res)),
        }
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

    pub fn make(&self) -> gst::Element {
        match gst::ElementFactory::make(self.name(), None) {
            Some(elem) => elem,
            None => panic!("Failed to create GStreamer element {}", self.name()),
        }
    }
}
