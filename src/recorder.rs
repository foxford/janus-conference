use std::fs;
use std::marker::PhantomData;
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
use gstreamer_pbutils::prelude::*;

use gst_elements::GstElement;
use messages::StreamId;

#[derive(Clone, Deserialize, Debug)]
pub struct Config {
    pub directory: String,
    pub enabled: bool,
}

impl Config {
    pub fn check(&mut self) -> Result<(), Error> {
        if !self.enabled {
            return Ok(());
        }

        if !Path::new(&self.directory).exists() {
            return Err(format_err!(
                "Recordings: recordings directory {} does not exist",
                self.directory
            ));
        }

        Ok(())
    }
}

const MKV_EXTENSION: &str = "mkv";
const MP4_EXTENSION: &str = "mp4";
const DISCOVERER_TIMEOUT: u64 = 15;
const FULL_RECORD_FILENAME: &str = "full";
const FULL_RECORD_CAPS: &str =
    "video/x-raw,width=1280,height=720,pixel-aspect-ratio=1/1,framerate=30/1";

#[derive(Debug)]
enum RecorderMsg {
    Stop,
    Packet {
        buf: gst::buffer::Buffer,
        is_video: bool,
    },
}

#[derive(Debug)]
pub struct RecorderImpl<V, A> {
    sender: mpsc::Sender<RecorderMsg>,
    receiver_for_recorder_thread: Option<mpsc::Receiver<RecorderMsg>>,
    recorder_thread_handle: Option<thread::JoinHandle<Result<(), Error>>>,
    stream_id: StreamId,
    filename: Option<String>,
    save_root_dir: String,
    video_codec: PhantomData<V>,
    audio_codec: PhantomData<A>,
}

/// Records video from RTP stream identified by StreamId.
///
/// StreamId is used as a directory for parts of a record.
/// In case of Janus restart stream newly created recorder
/// for old stream resumes recording but writes to new file
/// in that directory. Filename for record part is generated
/// by the following rule: `unix_timestamp.extension`.
///
/// Look at `codecs` module to find out
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
pub trait Recorder {
    type VideoCodec;
    type AudioCodec;

    fn new(config: &Config, stream_id: &str) -> Self;
    fn start_recording(&mut self) -> Result<(), Error>;
    fn record_packet(&self, buf: &[u8], is_video: bool) -> Result<(), Error>;
    fn stop_recording(&self) -> Result<(), Error>;
    fn finish_record(&mut self) -> Result<Vec<(u64, u64)>, Error>;
    fn get_full_record_path(&self) -> PathBuf;
}

impl<V: crate::codecs::VideoCodec, A: crate::codecs::AudioCodec> Recorder for RecorderImpl<V, A> {
    type VideoCodec = V;
    type AudioCodec = A;

    fn new(config: &Config, stream_id: &str) -> Self {
        let (sender, recv): (mpsc::Sender<RecorderMsg>, _) = mpsc::channel();

        Self {
            sender,
            receiver_for_recorder_thread: Some(recv),
            recorder_thread_handle: None,
            stream_id: stream_id.to_owned(),
            save_root_dir: config.directory.clone(),
            filename: None,
            video_codec: PhantomData,
            audio_codec: PhantomData,
        }
    }

    fn record_packet(&self, buf: &[u8], is_video: bool) -> Result<(), Error> {
        let buf = Self::wrap_buf(buf)?;
        let msg = RecorderMsg::Packet { buf, is_video };

        self.sender.send(msg).map_err(Error::from)
    }

    fn finish_record(&mut self) -> Result<Vec<(u64, u64)>, Error> {
        /*
        GStreamer pipeline we create here:

            filesrc location=1545122937.mkv ! matroskademux name=demux0
            demux0.video_0 ! queue ! h264parse ! v.
            demux0.audio_0 ! queue ! opusparse ! a.

            ...

            mp4mux name=mux
            concat name=v ! queue ! mux.video_0
            concat name=a ! queue ! mux.audio_0

            mux. ! filesink location=full.mp4
        */

        match self.stop_recording() {
            Ok(()) => {}
            Err(err) => {
                janus_err!("[CONFERENCE] Error during recording stop: {}", err);
            }
        }

        if let Some(handle) = self.recorder_thread_handle.take() {
            match handle.join() {
                Ok(_) => {}
                Err(err) => janus_err!(
                    "Error during finalization of current record part: {:?}",
                    err
                ),
            }
        }

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

        let mut start_stop_timestamps: Vec<(u64, u64)> = Vec::new();
        let timeout: gst::ClockTime = gst::ClockTime::from_seconds(DISCOVERER_TIMEOUT);
        let discoverer = gstreamer_pbutils::Discoverer::new(timeout)?;

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

            let path = file.path();
            let filename = path.to_string_lossy();

            let start = file
                .path()
                .as_path()
                .file_stem()
                .ok_or_else(|| format_err!("Bad filename {}.", filename))?
                .to_string_lossy()
                .parse::<u64>()
                .map_err(|_| format_err!("Bad filename {}. Expected timestamp.", filename))?;

            let duration = discoverer
                .discover_uri(&format!("file://{}", filename))?
                .get_duration()
                .mseconds()
                .ok_or_else(|| err_msg("Fail to get duration"))?;

            let stop = start + duration;
            start_stop_timestamps.push((start, stop));

            let filesrc = GstElement::Filesrc.make();
            filesrc.set_property("location", &filename.to_value())?;

            let demux = GstElement::MatroskaDemux.make();

            let video_parse = Self::VideoCodec::new_parse_elem();
            let video_queue = GstElement::Queue.make();

            let audio_parse = Self::AudioCodec::new_parse_elem();
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

        start_stop_timestamps.sort();
        Ok(start_stop_timestamps)
    }

    fn start_recording(&mut self) -> Result<(), Error> {
        /*
        GStreamer pipeline we create here:

            appsrc ! rtph264depay ! h264parse ! avdec_h264 ! videoscale ! videorate ! capsfilter caps=video/x-raw,width=1280,height=720,pixel-aspect-ratio=1/1,framerate=30/1 ! x264enc tune=zerolatency ! queue name=v
            appsrc ! rtpopusdepay ! opusparse ! queue name=a

            v. ! mux.video_0
            a. ! mux.audio_0

            matroskamux name=mux ! filesink location=${STREAM_ID}/${CURRENT_UNIX_TIMESTAMP}.mkv
        */

        janus_info!("[CONFERENCE] Initialize recording pipeline");

        let pipeline = gst::Pipeline::new(None);
        let mux = GstElement::MatroskaMux.make();
        let filesink = GstElement::Filesink.make();

        let start = unix_time_ms();
        let basename = start.to_string();

        let path = self.generate_record_path(&basename, MKV_EXTENSION);
        let path = path.to_string_lossy();

        self.filename = Some(basename);

        janus_info!("[CONFERENCE] Start recording to {}", path);

        filesink
            .set_property("location", &path.to_value())
            .expect("failed to set location prop on filesink?!");

        pipeline.add_many(&[&mux, &filesink])?;

        let (video_src, video_rtpdepay, video_parse) = Self::setup_video_elements();
        let video_queue = GstElement::Queue.make();

        let decode_video = Self::VideoCodec::new_decode_elem();
        let scale_video = GstElement::VideoScale.make();
        let rate_video = GstElement::VideoRate.make();

        let capsfilter_video = GstElement::CapsFilter.make();
        capsfilter_video.set_property_from_str("caps", FULL_RECORD_CAPS);

        let encode_video = Self::VideoCodec::new_encode_elem();
        encode_video.set_property_from_str("tune", "zerolatency");

        let (audio_src, audio_rtpdepay, audio_parse) = Self::setup_audio_elements();
        let audio_queue = GstElement::Queue.make();

        {
            let video_elems = [
                &video_src.upcast_ref(),
                &video_rtpdepay,
                &video_parse,
                &decode_video,
                &scale_video,
                &rate_video,
                &capsfilter_video,
                &encode_video,
                &video_queue,
            ];

            pipeline.add_many(&video_elems)?;
            gst::Element::link_many(&video_elems)?;

            let audio_elems = [
                &audio_src.upcast_ref(),
                &audio_rtpdepay,
                &audio_parse,
                &audio_queue,
            ];

            pipeline.add_many(&audio_elems)?;
            gst::Element::link_many(&audio_elems)?;
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

        let recv = self
            .receiver_for_recorder_thread
            .take()
            .expect("Empty receiver in recorder?!");

        let handle = thread::spawn(move || {
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

            janus_info!("[CONFERENCE] Stop recording");

            Ok(())
        });

        self.recorder_thread_handle = Some(handle);

        Ok(())
    }

    fn stop_recording(&self) -> Result<(), Error> {
        self.sender.send(RecorderMsg::Stop)?;
        Ok(())
    }

    fn get_full_record_path(&self) -> PathBuf {
        self.generate_record_path(FULL_RECORD_FILENAME, MP4_EXTENSION)
    }
}

// Associated types are not yet supported in inherent impls (see #8995)
// so we define private methods through this particular trait.
trait RecorderPrivate {
    type VideoCodec;
    type AudioCodec;

    fn init_app_src(caps: gst::Caps) -> gst_app::AppSrc;
    fn setup_video_elements() -> (gst_app::AppSrc, gst::Element, gst::Element);
    fn setup_audio_elements() -> (gst_app::AppSrc, gst::Element, gst::Element);
    fn wrap_buf(buf: &[u8]) -> Result<gst::Buffer, Error>;
    fn get_records_dir(&self) -> PathBuf;
    fn generate_record_path(&self, filename: &str, extension: &str) -> PathBuf;
    fn run_pipeline_to_completion(pipeline: &gst::Pipeline);

    fn link_static_and_request_pads(
        static_elem_and_pad: (&gst::Element, &str),
        request_elem_and_pad: (&gst::Element, &str),
    ) -> Result<gst::Pad, Error>;
}

impl<V: crate::codecs::VideoCodec, A: crate::codecs::AudioCodec> RecorderPrivate
    for RecorderImpl<V, A>
{
    type VideoCodec = V;
    type AudioCodec = A;

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

    fn setup_video_elements() -> (gst_app::AppSrc, gst::Element, gst::Element) {
        let caps = gst::Caps::new_simple(
            "application/x-rtp",
            &[
                ("media", &"video"),
                ("encoding-name", &Self::VideoCodec::NAME),
                ("payload", &96),
                ("clock-rate", &90000),
            ],
        );

        let src = Self::init_app_src(caps);
        (src, V::new_depay_elem(), V::new_parse_elem())
    }

    fn setup_audio_elements() -> (gst_app::AppSrc, gst::Element, gst::Element) {
        let caps = gst::Caps::new_simple(
            "application/x-rtp",
            &[
                ("media", &"audio"),
                ("encoding-name", &Self::AudioCodec::NAME),
                ("payload", &111),
                ("clock-rate", &48000),
            ],
        );

        let src = Self::init_app_src(caps);
        (src, A::new_depay_elem(), A::new_parse_elem())
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

    fn get_records_dir(&self) -> PathBuf {
        let mut path = PathBuf::new();
        path.push(&self.save_root_dir);
        path.push(&self.stream_id);

        path
    }

    fn generate_record_path(&self, filename: &str, extension: &str) -> PathBuf {
        let mut path = self.get_records_dir();

        if let Err(err) = fs::create_dir(&path) {
            match err.kind() {
                ::std::io::ErrorKind::AlreadyExists => {}
                err => {
                    panic!("Failed to create directory for record: {:?}", err);
                }
            }
        }

        path.push(filename);
        path.set_extension(extension);

        path
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

unsafe impl<V, A> Sync for RecorderImpl<V, A> {}

fn unix_time_ms() -> u64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

    now.as_secs() * 1000 + now.subsec_millis() as u64
}
