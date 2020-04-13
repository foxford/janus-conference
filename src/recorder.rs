use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io, thread};

use failure::{err_msg, Error, Fail};
use glib;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_pbutils::prelude::*;

use crate::switchboard::StreamId;

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
            bail!(
                "Recordings: recordings directory {} does not exist",
                self.directory
            );
        }

        Ok(())
    }
}

const MKV_EXTENSION: &str = "mkv";
const MP4_EXTENSION: &str = "mp4";
const DISCOVERER_TIMEOUT: u64 = 15;
const FULL_RECORD_FILENAME: &str = "full";

const RECORDING_PIPELINE: &str = r#"
    appsrc name=video_src stream-type=stream format=time is-live=true do-timestamp=true !
        application/x-rtp, media=video, encoding-name=H264, payload=(int)96, clock-rate=(int)90000 !
        rtph264depay !
        h264parse !
        avdec_h264 !
        videoscale !
        videorate !
        videoconvert !
        video/x-raw, width=1280, height=720, pixel-aspect-ratio=1/1, framerate=30/1, format=I420, profile=high !
        x264enc key-int-max=60 tune=zerolatency speed-preset=ultrafast !
        queue !
        mux.video_0

    appsrc name=audio_src stream-type=stream format=time is-live=true do-timestamp=true !
        application/x-rtp, media=audio, encoding-name=OPUS, payload=(int)111, clock-rate=(int)48000 !
        rtpopusdepay !
        opusparse !
        queue !
        mux.audio_0

    matroskamux name=mux !
        filesink name=out
"#;

#[derive(Debug)]
enum RecorderMsg {
    Stop,
    Packet {
        buf: gst::buffer::Buffer,
        is_video: bool,
    },
}

#[derive(Debug)]
pub struct Recorder {
    sender: mpsc::Sender<RecorderMsg>,
    receiver_for_recorder_thread: Option<mpsc::Receiver<RecorderMsg>>,
    recorder_thread_handle: Option<thread::JoinHandle<Result<(), Error>>>,
    stream_id: StreamId,
    filename: Option<String>,
    save_root_dir: String,
}

/// Records video from RTP stream identified by `stream_id`.
///
/// `stream_id` is used as a directory for parts of a record.
/// In case of Janus restart stream newly created recorder
/// for old stream resumes recording but writes to new file
/// in that directory. Filename for record part is generated
/// by the following rule: `unix_timestamp.extension`.
///
/// GStreamer recording pipeline runs in separate thread.
/// You're able to write buffers using `record_packet` method.
///
/// It's possible to make a full concatenated record
/// (e.g. stream is over and you need to pass full record
/// to some external service). Use method `finish_record`
/// for that.

impl Recorder {
    pub fn new(config: &Config, stream_id: StreamId) -> Self {
        let (sender, recv): (mpsc::Sender<RecorderMsg>, _) = mpsc::channel();

        Self {
            sender,
            receiver_for_recorder_thread: Some(recv),
            recorder_thread_handle: None,
            stream_id,
            save_root_dir: config.directory.clone(),
            filename: None,
        }
    }

    pub fn record_packet(&self, buf: &[u8], is_video: bool) -> Result<(), Error> {
        let buf = Self::wrap_buf(buf)?;
        let msg = RecorderMsg::Packet { buf, is_video };

        self.sender.send(msg).map_err(Error::from)
    }

    pub fn finish_record(&mut self) -> Result<(u64, Vec<(u64, u64)>), RecorderError> {
        let records_dir = self.get_records_dir();

        if !records_dir.is_dir() {
            return Err(RecorderError::RecordingMissing);
        }

        let mut parts: Vec<RecordPart> = fs::read_dir(&records_dir)?
            .filter_map(|maybe_dir_entry| {
                maybe_dir_entry
                    .ok()
                    .and_then(|dir_entry| RecordPart::from_path(dir_entry.path()))
            })
            .collect();

        if parts.is_empty() {
            return Err(RecorderError::RecordingMissing);
        }

        parts.sort_by_key(|part| part.start);

        let absolute_started_at = parts[0].start;
        let mut relative_timestamps: Vec<(u64, u64)> = Vec::with_capacity(parts.len());
        let files_list_path = records_dir.join("parts.txt");

        {
            let files_list = fs::File::create(files_list_path.as_path())?;
            let mut files_list_writer = BufWriter::new(&files_list);

            for part in parts {
                // file '/recordings/123/1234567890.mkv'
                let filename = part.path.as_path().to_string_lossy().into_owned();
                writeln!(&mut files_list_writer, "file '{}'", filename)?;

                let start = part.start - absolute_started_at;
                let stop = start + part.duration;
                relative_timestamps.push((start, stop));
            }
        }

        let full_record_path = self.get_full_record_path().to_string_lossy().into_owned();

        janus_info!(
            "[CONFERENCE] Concatenating full record to {}",
            full_record_path
        );

        // Use ffmpeg for concatenation instead of gstreamer because it doesn't hang on corrupted videos.
        // No transcoding is made here because it would create a peak load on the server.
        //
        // ffmpeg -f concat -safe 0 -i /recordings/123/parts.txt -c copy -y /recordings/123/full.mp4
        let mut command = Command::new("ffmpeg");

        command.args(&[
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            &files_list_path.to_string_lossy().into_owned(),
            "-c",
            "copy",
            "-y",
            "-strict",
            "-2",
            &full_record_path,
        ]);

        janus_info!("[CONFERENCE] {:?}", command);
        let status = command.status()?;

        if status.success() {
            janus_info!(
                "[CONFERENCE] Full record concatenated to {}",
                full_record_path
            );

            Ok((absolute_started_at, relative_timestamps))
        } else {
            let err = format_err!(
                "Failed to concatenate full record {} ({})",
                full_record_path,
                status
            );

            Err(err.into())
        }
    }

    pub fn start_recording(&mut self) -> Result<(), Error> {
        janus_info!("[CONFERENCE] Initialize recording pipeline");

        // Build pipeline by description and get necessary elements' and pads' handles.
        let pipeline = gst::parse_launch(RECORDING_PIPELINE)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| err_msg("Failed to downcast gst::Element to gst::Pipeline"))?;

        let video_src = pipeline
            .get_by_name("video_src")
            .ok_or_else(|| err_msg("Failed to get appsrc element named `video_src`"))?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| err_msg("Failed to downcast `video_src` element  to gst_app::AppSrc"))?;

        let audio_src = pipeline
            .get_by_name("audio_src")
            .ok_or_else(|| err_msg("Failed to get appsrc element named `audio_src`"))?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| err_msg("Failed to downcast `audio_src` element to gst_app::AppSrc"))?;

        let mux = pipeline
            .get_by_name("mux")
            .ok_or_else(|| err_msg("Failed to get matroskamux element named `mux`"))?;

        let video_sink_pad = mux
            .get_static_pad("video_0")
            .ok_or_else(|| err_msg("Failed to request `video_0` pad from `mux` element"))?;

        let audio_sink_pad = mux
            .get_static_pad("audio_0")
            .ok_or_else(|| err_msg("Failed to request `audio_0` pad from `mux` element"))?;

        let filesink = pipeline
            .get_by_name("out")
            .ok_or_else(|| err_msg("Failed to get filesink element named `out`"))?;

        // Set output filename to `./recordings/{STREAM_ID}/{CURRENT_TIMESTAMP}.mkv`.
        let start = unix_time_ms();
        let basename = start.to_string();

        let path = self.generate_record_path(&basename, MKV_EXTENSION);
        let path = path.to_string_lossy().into_owned();

        filesink.set_property("location", &path)?;

        // Start the pipeline.
        if pipeline.set_state(gst::State::Playing) == gst::StateChangeReturn::Failure {
            bail!("Failed to put pipeline to the `playing` state");
        }

        // Handle the pipeline in a separate thread.
        let recv = self
            .receiver_for_recorder_thread
            .take()
            .ok_or_else(|| err_msg("Empty receiver in recorder"))?;

        let handle = thread::spawn(move || {
            janus_info!("[CONFERENCE] Start recording to {}", path);

            // Push RTP packets into the pipeline until stop message.
            for msg in recv.iter() {
                match msg {
                    RecorderMsg::Stop => break,
                    RecorderMsg::Packet { is_video, buf } => {
                        let res = if is_video {
                            video_src.push_buffer(buf)
                        } else {
                            audio_src.push_buffer(buf)
                        };

                        if res != gst::FlowReturn::Ok {
                            bail!("Error pushing buffer to AppSrc: {:?}", res);
                        };
                    }
                }
            }

            // Notify the pipeline that there will be no more RTP packets and finish it.
            let res = video_src.end_of_stream();
            if res != gst::FlowReturn::Ok {
                bail!("Failed to finish video stream: {:?}", res);
            }

            let res = audio_src.end_of_stream();
            if res != gst::FlowReturn::Ok {
                bail!("Failed to finish audio stream: {:?}", res);
            }

            let eos_ev = gst::Event::new_eos().build();
            pipeline.send_event(eos_ev);

            Self::run_pipeline_to_completion(&pipeline)?;

            mux.release_request_pad(&audio_sink_pad);
            mux.release_request_pad(&video_sink_pad);

            janus_info!("[CONFERENCE] Stop recording");
            Ok(())
        });

        self.recorder_thread_handle = Some(handle);
        Ok(())
    }

    pub fn stop_recording(&mut self) -> Result<(), Error> {
        self.sender.send(RecorderMsg::Stop)?;

        if let Some(handle) = self.recorder_thread_handle.take() {
            if let Err(err) = handle.join() {
                janus_err!(
                    "Error during finalization of current record part: {:?}",
                    err
                );
            }
        }

        Ok(())
    }

    pub fn get_full_record_path(&self) -> PathBuf {
        // Use MP4 container instead of MKV because the video editor doesn't support MKV
        self.generate_record_path(FULL_RECORD_FILENAME, MP4_EXTENSION)
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
        path.push(&self.stream_id.to_string());

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

    fn run_pipeline_to_completion(pipeline: &gst::Pipeline) -> Result<(), Error> {
        let (tx, rx) = mpsc::channel();
        let main_loop = glib::MainLoop::new(None, false);
        let main_loop_clone = main_loop.clone();

        let bus = match pipeline.get_bus() {
            Some(bus) => bus,
            None => {
                Self::shutdown_pipeline(pipeline);
                bail!("Failed to get pipeline bus");
            }
        };

        bus.add_watch(move |_bus, msg| {
            let maybe_result = match msg.view() {
                gst::MessageView::Eos(..) => Some(Ok(())),
                gst::MessageView::Error(err) => Some(Err(format_err!("{}", err.get_error()))),
                _ => None,
            };

            if let Some(result) = maybe_result {
                tx.send(result)
                    .unwrap_or_else(|err| janus_err!("[CONFERENCE] {}", err));
                main_loop_clone.quit();
            }

            glib::Continue(true)
        });

        main_loop.run();
        Self::shutdown_pipeline(pipeline);
        bus.remove_watch();
        rx.recv().unwrap_or_else(|err| Err(format_err!("{}", err)))
    }

    fn shutdown_pipeline(pipeline: &gst::Pipeline) {
        if pipeline.set_state(gst::State::Null) == gst::StateChangeReturn::Failure {
            janus_err!("[CONFERENCE] Failed to set pipeline state to NULL");
        }
    }

    pub fn delete_record(&self) -> Result<(), RecorderError> {
        let records_dir = self.get_records_dir();

        fs::remove_dir_all(records_dir).map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => RecorderError::RecordingMissing,
            _ => RecorderError::IoError(err),
        })
    }
}

fn unix_time_ms() -> u64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

    now.as_secs() * 1000 + now.subsec_millis() as u64
}

struct RecordPart {
    path: PathBuf,
    start: u64,
    duration: u64,
}

impl RecordPart {
    pub fn new(path: PathBuf, start: u64, duration: u64) -> Self {
        Self {
            path,
            start,
            duration,
        }
    }

    pub fn from_path(path: PathBuf) -> Option<Self> {
        if !Self::is_valid_file(&path) {
            return None;
        }

        Self::parse_start_timestamp(&path).and_then(|start| match Self::discover_duration(&path) {
            Ok(duration) => Some(Self::new(path, start, duration)),
            Err(err) => {
                janus_err!(
                    "[CONFERENCE] Failed to get duration for {}: {}. Skipping part.",
                    path.as_path().to_string_lossy(),
                    err
                );

                None
            }
        })
    }

    fn is_valid_file(path: &PathBuf) -> bool {
        let extension = match path.extension() {
            Some(extension) => extension,
            None => return false,
        };

        if extension != MKV_EXTENSION {
            return false;
        }

        let stem = match path.as_path().file_stem() {
            Some(stem) => stem,
            None => return false,
        };

        if stem.to_string_lossy().starts_with(".") {
            return false;
        }

        if stem == FULL_RECORD_FILENAME {
            return false;
        }

        match path.metadata() {
            Ok(metadata) => metadata.is_file() && metadata.len() > 0,
            Err(err) => {
                janus_err!(
                    "[CONFERENCE] Failed to get metadata for {}: {}",
                    path.as_path().to_string_lossy(),
                    err
                );

                false
            }
        }
    }

    fn parse_start_timestamp(path: &PathBuf) -> Option<u64> {
        path.as_path()
            .file_stem()
            .and_then(|stem| stem.to_string_lossy().parse::<u64>().ok())
    }

    fn discover_duration(path: &PathBuf) -> Result<u64, Error> {
        gstreamer_pbutils::Discoverer::new(gst::ClockTime::from_seconds(DISCOVERER_TIMEOUT))?
            .discover_uri(&format!("file://{}", path.as_path().to_string_lossy()))?
            .get_duration()
            .mseconds()
            .ok_or_else(|| err_msg("Empty duration"))
    }
}

#[derive(Fail, Debug)]
pub enum RecorderError {
    #[fail(display = "{}", _0)]
    InternalError(#[cause] Error),
    #[fail(display = "{}", _0)]
    IoError(#[cause] io::Error),
    #[fail(display = "Recording missing")]
    RecordingMissing,
}

impl From<Error> for RecorderError {
    fn from(err: Error) -> RecorderError {
        RecorderError::InternalError(err)
    }
}

impl From<io::Error> for RecorderError {
    fn from(err: io::Error) -> RecorderError {
        RecorderError::IoError(err)
    }
}
