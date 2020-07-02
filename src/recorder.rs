use std::error::Error as StdError;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::{fs, io, thread};

use anyhow::{bail, format_err, Context, Error, Result};
use chrono::Utc;

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

    pub fn get_records_dir(&self) -> PathBuf {
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

    pub fn delete_record(&self) -> Result<()> {
        fs::remove_dir_all(&self.get_records_dir()).context("Failed to delete record")
    }
}

#[derive(Debug)]
pub enum RecorderError {
    InternalError(Error),
    IoError(io::Error),
}

impl fmt::Display for RecorderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InternalError(source) => write!(f, "{}", source),
            Self::IoError(source) => write!(f, "{}", source),
        }
    }
}

impl StdError for RecorderError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::InternalError(source) => Some(source.as_ref()),
            Self::IoError(source) => Some(source),
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
