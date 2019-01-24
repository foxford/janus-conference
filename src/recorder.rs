use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use failure::{err_msg, Error};
use glib;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_base::BaseSrcExt;

use messages::StreamId;

#[derive(Deserialize, Debug)]
pub struct RecordingConfig {
    pub recordings_directory: String,
    pub enabled: bool,
}

impl RecordingConfig {
    pub fn check(&mut self) -> Result<(), Error> {
        if !self.enabled {
            return Ok(());
        }

        if !Path::new(&self.recordings_directory).exists() {
            return Err(format_err!(
                "Recordings: recordings directory {} does not exist",
                self.recordings_directory
            ));
        }

        Ok(())
    }
}

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

    pub fn new_depay_elem(self) -> gst::Element {
        match self {
            VideoCodec::H264 => GstElement::RTPH264Depay.make(),
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

    pub fn new_depay_elem(self) -> gst::Element {
        match self {
            AudioCodec::OPUS => GstElement::RTPOpusDepay.make(),
        }
    }
}

const MKV: &str = "mkv";
const MP4: &str = "mp4";
const FULL_RECORD_FILENAME: &str = "full";

#[derive(Debug)]
enum RecorderMsg {
    Stop,
    Packet {
        buf: gst::buffer::Buffer,
        is_video: bool,
    },
}

/// Records video from RTP stream identified by StreamId.
///
/// StreamId is used as a directory for parts of a record.
/// In case of Janus restart stream newly created recorder
/// for old stream resumes recording but writes to new file
/// in that directory. Filename for record part is generated
/// by the following rule: `unix_timestamp.extension`.
///
/// Look at `VideoCodec` and `AudioCodec` enums to find out
/// which codecs are supported. It's up to you to determine
/// exact codecs during signaling.
///
/// GStreamer recording pipeline runs in separate thread.
/// You're able to write buffers using `record_packet` method.
///
/// It's possible to make a full concatenated record
/// (e.g. stream is over and you need to pass full record
/// to some external service). Use method `finish_record`
/// for that.
#[derive(Debug)]
pub struct Recorder {
    sender: mpsc::Sender<RecorderMsg>,
    stream_id: StreamId,
    save_root_dir: String,
    video_codec: VideoCodec,
    audio_codec: AudioCodec,
    recorder_thread_handle: Option<thread::JoinHandle<Result<(), Error>>>,
}

unsafe impl Sync for Recorder {}

impl Recorder {
    pub fn new(
        recording_config: &RecordingConfig,
        stream_id: &str,
        video_codec: VideoCodec,
        audio_codec: AudioCodec,
    ) -> Self {
        let (sender, recv): (mpsc::Sender<RecorderMsg>, _) = mpsc::channel();

        let mut rec = Self {
            sender,
            stream_id: stream_id.to_owned(),
            save_root_dir: recording_config.recordings_directory.clone(),
            video_codec,
            audio_codec,
            recorder_thread_handle: None,
        };

        let handle = rec.setup_recording(recv);
        rec.recorder_thread_handle = Some(handle);

        rec
    }

    pub fn record_packet(&self, buf: &[u8], is_video: bool) -> Result<(), Error> {
        let buf = Self::wrap_buf(buf)?;
        let msg = RecorderMsg::Packet { buf, is_video };

        self.sender.send(msg).map_err(Error::from)
    }

    pub fn finish_record(&mut self) -> Result<(), Error> {
        /*
        GStreamer pipeline we create here:

            filesrc location=1545122937.mkv ! matroskademux name=demux0
            demux0.video_0 ! queue ! h264parse ! v.
            demux0.audio_0 ! queue ! opusparse ! a.

            ...

            concat name=v ! queue ! mp4mux name=mux
            concat name=a ! queue ! mux.audio_0

            mux. ! filesink location=full.mp4
        */

        self.sender.send(RecorderMsg::Stop)?;

        let _res = self
            .recorder_thread_handle
            .take()
            .ok_or_else(|| err_msg("Missing thread handle?!"))?
            .join()
            .map_err(|err| {
                format_err!(
                    "Error during finalization of current record part: {:?}",
                    err
                )
            })?;

        let mux = GstElement::MP4Mux.make();

        let filesink = GstElement::Filesink.make();
        let location = self.get_full_record_path();
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

        let records_dir = self.get_records_dir();
        let mut parts: Vec<fs::DirEntry> =
            fs::read_dir(&records_dir)?.filter_map(|r| r.ok()).collect();

        parts.sort_by_key(|f| f.path());

        for file in parts {
            let metadata = file.metadata()?;

            if metadata.is_dir() {
                continue;
            }

            match file.path().as_path().file_stem() {
                None => {
                    continue;
                }
                Some(stem) => {
                    if stem.to_string_lossy().starts_with(".") || stem == FULL_RECORD_FILENAME {
                        continue;
                    }
                }
            }

            let filesrc = GstElement::Filesrc.make();
            filesrc.set_property("location", &file.path().to_string_lossy().to_value())?;

            let demux = GstElement::MatroskaDemux.make();

            let video_parse = self.video_codec.new_parse_elem();
            let video_queue = GstElement::Queue.make();

            let audio_parse = self.audio_codec.new_parse_elem();
            let audio_queue = GstElement::Queue.make();

            pipeline.add_many(&[
                &filesrc,
                &demux,
                &video_parse,
                &audio_parse,
                &video_queue,
                &audio_queue,
            ])?;

            filesrc.link(&demux)?;
            video_queue.link(&video_parse)?;
            video_parse.link(&concat_video)?;

            audio_queue.link(&audio_parse)?;
            audio_parse.link(&concat_audio)?;

            demux.connect("pad-added", true, move |args| {
                let pad = args[1]
                    .get::<gst::Pad>()
                    .expect("Second argument is not a Pad");

                let sink = match pad.get_name().as_ref() {
                    "video_0" => Some(&video_queue),
                    "audio_0" => Some(&audio_queue),
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

        janus_info!("[CONFERENCE] End of full record");

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
        &self,
        recv: mpsc::Receiver<RecorderMsg>,
    ) -> thread::JoinHandle<Result<(), Error>> {
        /*
        GStreamer pipeline we create here:

            appsrc ! rtph264depay ! h264parse ! queue name=v
            appsrc ! rtpopusdepay ! opusparse ! queue name=a

            v. ! mux.video_0
            a. ! mux.audio_0

            matroskamux name=mux ! filesink location=${STREAM_ID}/${CURRENT_UNIX_TIMESTAMP}.mkv
        */
        let pipeline = gst::Pipeline::new(None);
        let mux = GstElement::MatroskaMux.make();

        let filesink = GstElement::Filesink.make();
        let path = self.generate_record_path(None, MKV);
        let path = path.to_string_lossy();

        janus_info!("[CONFERENCE] Saving video to {}", path);

        filesink
            .set_property("location", &path.to_value())
            .expect("failed to set location prop on filesink?!");

        pipeline
            .add_many(&[&mux, &filesink])
            .expect("Failed to add elems to pipeline");

        let (video_src, video_rtpdepay, video_codec) = Self::setup_video_elements(self.video_codec);
        let video_queue = GstElement::Queue.make();

        let (audio_src, audio_rtpdepay, audio_codec) = Self::setup_audio_elements(self.audio_codec);
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

        mux.link(&filesink)
            .expect("Failed to link matroskamux -> filesink");

        let video_sink_pad =
            Self::link_static_and_request_pads((&video_queue, "src"), (&mux, "video_%u"))
                .expect("Failed to link video -> mux");

        let audio_sink_pad =
            Self::link_static_and_request_pads((&audio_queue, "src"), (&mux, "audio_%u"))
                .expect("Failed to link audio -> mux");

        let res = pipeline.set_state(gst::State::Playing);
        assert_ne!(res, gst::StateChangeReturn::Failure);

        thread::spawn(move || {
            for msg in recv.iter() {
                match msg {
                    RecorderMsg::Packet { is_video, buf } => {
                        let res = if is_video {
                            video_src.push_buffer(buf)
                        } else {
                            audio_src.push_buffer(buf)
                        };
                        if res != gst::FlowReturn::Ok {
                            let err = format_err!("Error pushing buffer to AppSrc: {:?}", res);
                            return Err(err);
                        };
                    }
                    RecorderMsg::Stop => {
                        break;
                    }
                }
            }

            let res = video_src.end_of_stream();
            if res != gst::FlowReturn::Ok {
                let err = format_err!("Error trying to finish video stream: {:?}", res);
                return Err(err);
            }

            let res = audio_src.end_of_stream();
            if res != gst::FlowReturn::Ok {
                let err = format_err!("Error trying to finish audio stream: {:?}", res);
                return Err(err);
            }

            let eos_ev = gst::Event::new_eos().build();
            pipeline.send_event(eos_ev);

            Self::run_pipeline_to_completion(&pipeline);

            mux.release_request_pad(&audio_sink_pad);
            mux.release_request_pad(&video_sink_pad);

            janus_info!("[CONFERENCE] End of record");

            Ok(())
        })
    }

    fn get_records_dir(&self) -> PathBuf {
        let mut path = PathBuf::new();
        path.push(&self.save_root_dir);
        path.push(&self.stream_id);

        path
    }

    fn generate_record_path(&self, filename: Option<String>, extension: &str) -> PathBuf {
        let mut path = self.get_records_dir();

        if let Err(err) = fs::create_dir(&path) {
            match err.kind() {
                ::std::io::ErrorKind::AlreadyExists => {}
                err => {
                    panic!("Failed to create directory for record: {:?}", err);
                }
            }
        }

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

    pub fn get_full_record_path(&self) -> PathBuf {
        self.generate_record_path(Some(FULL_RECORD_FILENAME.to_owned()), MP4)
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

        (src, codec.new_depay_elem(), codec.new_parse_elem())
    }

    fn setup_audio_elements(codec: AudioCodec) -> (gst_app::AppSrc, gst::Element, gst::Element) {
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

        (src, codec.new_depay_elem(), codec.new_parse_elem())
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
    MP4Mux,
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
            GstElement::MP4Mux => "mp4mux",
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
