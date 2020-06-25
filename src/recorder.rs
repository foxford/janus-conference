use std::error::Error as StdError;
use std::fmt;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::result::Result as StdResult;
use std::sync::mpsc;
use std::{fs, io, thread};

use anyhow::{bail, format_err, Context, Error, Result};
use chrono::Utc;
use gstreamer as gst;

use crate::janus_recorder::{Codec, JanusRecorder};
use crate::switchboard::StreamId;

#[derive(Clone, Deserialize, Debug)]
pub struct Config {
    pub directory: String,
    pub enabled: bool,
}

impl Config {
    pub fn check(&mut self) -> Result<()> {
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

const WEBM_EXTENSION: &str = "webm";
const DISCOVERER_TIMEOUT: u64 = 15;
const FULL_RECORD_FILENAME: &str = "full";

#[derive(Debug)]
enum RecorderMsg {
    Stop,
    Packet { buf: Vec<i8>, is_video: bool },
}

#[derive(Debug)]
pub struct Recorder {
    sender: mpsc::Sender<RecorderMsg>,
    receiver_for_recorder_thread: Option<mpsc::Receiver<RecorderMsg>>,
    recorder_thread_handle: Option<thread::JoinHandle<Result<()>>>,
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
/// Recorder runs in separate thread.
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

    pub fn record_packet(&self, buf: &[i8], is_video: bool) -> Result<()> {
        let msg = RecorderMsg::Packet {
            buf: buf.to_vec(),
            is_video,
        };

        self.sender.send(msg).context("Failed to send packet")
    }

    pub fn finish_record(&mut self) -> StdResult<(u64, Vec<(u64, u64)>), RecorderError> {
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
                // file '/recordings/123/1234567890.webm'
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

        // Use ffmpeg for concatenation because it doesn't hang on corrupted videos.
        // No transcoding is made here because it would create a peak load on the server.
        //
        // ffmpeg -f concat -safe 0 -i /recordings/123/parts.txt -c copy -y /recordings/123/full.webm
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

    pub fn start_recording(&mut self) -> Result<()> {
        janus_info!("[CONFERENCE] Start recording");
        let dir = self.create_records_dir().to_string_lossy().into_owned();

        // Handle the pipeline in a separate thread.
        let recv = self
            .receiver_for_recorder_thread
            .take()
            .ok_or_else(|| format_err!("Empty receiver in recorder"))?;

        let handle = thread::spawn(move || {
            janus_verb!("[CONFERENCE] Recorder thread started");

            // Initialize recorders.
            let now = Utc::now().timestamp_millis();

            let video_filename = format!("{}.video", now);
            let mut video_recorder = JanusRecorder::create(&dir, &video_filename, Codec::VP8)?;

            let audio_filename = format!("{}.audio", now);
            let mut audio_recorder = JanusRecorder::create(&dir, &audio_filename, Codec::OPUS)?;

            janus_info!("[CONFERENCE] Recording to {}", dir);

            // Push RTP packets into the pipeline until stop message.
            for msg in recv.iter() {
                match msg {
                    RecorderMsg::Stop => break,
                    RecorderMsg::Packet { is_video, buf } => {
                        let res = if is_video {
                            video_recorder.save_frame(buf.as_slice())
                        } else {
                            audio_recorder.save_frame(buf.as_slice())
                        };

                        if let Err(err) = res {
                            janus_err!("[CONFERENCE] Failed to record frame: {}", err);
                        }
                    }
                }
            }

            video_recorder.close()?;
            audio_recorder.close()?;
            janus_info!("[CONFERENCE] Recording stopped");
            Ok(())
        });

        self.recorder_thread_handle = Some(handle);
        Ok(())
    }

    pub fn stop_recording(&mut self) -> Result<()> {
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
        let mut path = self.get_records_dir();
        path.push(FULL_RECORD_FILENAME);
        path.set_extension(WEBM_EXTENSION);
        path
    }

    fn get_records_dir(&self) -> PathBuf {
        let mut path = PathBuf::new();
        path.push(&self.save_root_dir);
        path.push(&self.stream_id.to_string());
        path
    }

    fn create_records_dir(&self) -> PathBuf {
        let path = self.get_records_dir();

        if let Err(err) = fs::create_dir(&path) {
            match err.kind() {
                ::std::io::ErrorKind::AlreadyExists => {}
                err => panic!("Failed to create directory for record: {:?}", err),
            }
        }

        path
    }

    pub fn delete_record(&self) -> StdResult<(), RecorderError> {
        let records_dir = self.get_records_dir();

        fs::remove_dir_all(records_dir).map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => RecorderError::RecordingMissing,
            _ => RecorderError::IoError(err),
        })
    }
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

        if extension != WEBM_EXTENSION {
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

    fn discover_duration(path: &PathBuf) -> Result<u64> {
        gstreamer_pbutils::Discoverer::new(gst::ClockTime::from_seconds(DISCOVERER_TIMEOUT))?
            .discover_uri(&format!("file://{}", path.as_path().to_string_lossy()))?
            .get_duration()
            .mseconds()
            .ok_or_else(|| format_err!("Empty duration"))
    }
}

#[derive(Debug)]
pub enum RecorderError {
    InternalError(Error),
    IoError(io::Error),
    RecordingMissing,
}

impl fmt::Display for RecorderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InternalError(source) => write!(f, "{}", source),
            Self::IoError(source) => write!(f, "{}", source),
            Self::RecordingMissing => write!(f, "Recording missing"),
        }
    }
}

impl StdError for RecorderError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::InternalError(source) => Some(source.as_ref()),
            Self::IoError(source) => Some(source),
            Self::RecordingMissing => None,
        }
    }
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
