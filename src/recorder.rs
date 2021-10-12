use std::{
    collections::hash_map::Entry,
    path::{Path, PathBuf},
};
use std::{error::Error as StdError, time::Duration};
use std::{fmt, time::Instant};
use std::{fs, io};

use anyhow::{anyhow, bail, Context, Error, Result};
use chrono::{DateTime, Utc};
use crossbeam_channel::{Receiver, Sender};
use fnv::FnvHashMap;
use tokio::sync::oneshot;

use crate::switchboard::StreamId;
use crate::{
    janus_recorder::{Codec, JanusRecorder},
    metrics::Metrics,
};
use serde::Deserialize;

#[derive(Clone, Deserialize, Debug)]
pub struct Config {
    pub directory: String,
    pub enabled: bool,
    pub delete_records: bool,
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
    Stop {
        stream_id: StreamId,
    },
    Packet {
        buf: Vec<i8>,
        is_video: bool,
        stream_id: StreamId,
    },
    Start {
        stream_id: StreamId,
        dir: String,
        start_time: DateTime<Utc>,
    },
    WaitStop {
        waiter: oneshot::Sender<()>,
        stream_id: StreamId,
    },
}

#[derive(Debug)]
pub struct RecorderHandlesCreator {
    sender: Sender<RecorderMsg>,
    config: Config,
}

impl RecorderHandlesCreator {
    fn new(sender: Sender<RecorderMsg>, config: Config) -> Self {
        Self { sender, config }
    }

    pub fn new_handle(&self, stream_id: StreamId) -> RecorderHandle {
        RecorderHandle::new(&self.config, stream_id, self.sender.clone())
    }
}

pub struct Recorder {
    messages: Receiver<RecorderMsg>,
    metrics_update_interval: Duration,
}

impl Recorder {
    fn new(messages: Receiver<RecorderMsg>, metrics_update_interval: Duration) -> Self {
        Self {
            messages,
            metrics_update_interval,
        }
    }

    pub fn start(self) {
        let mut recorders = FnvHashMap::default();
        let mut now = Instant::now();
        let mut waiters: FnvHashMap<_, Vec<oneshot::Sender<()>>> = FnvHashMap::default();
        loop {
            let msg = self.messages.recv().expect("All senders dropped");
            if now.elapsed() > self.metrics_update_interval {
                Metrics::observe_recorder(recorders.len(), self.messages.len(), waiters.len());
                now = Instant::now();
            }

            match msg {
                RecorderMsg::Stop { stream_id } => {
                    if let Err(err) = Self::handle_stop(&mut recorders, stream_id).context("Stop") {
                        err!("Recording stopping error: {:?}", err; {"rtc_id": stream_id});
                    } else {
                        info!("Recording stopped"; {"rtc_id": stream_id});
                    }
                    if let Some(waiters) = waiters.remove(&stream_id) {
                        for mut waiter in waiters {
                            let _ = waiter.send(());
                        }
                    }
                }
                RecorderMsg::Packet {
                    buf,
                    is_video,
                    stream_id,
                } => {
                    if let Err(err) =
                        Self::handle_packet(&mut recorders, stream_id, buf.as_slice(), is_video)
                            .context("Packet")
                    {
                        err!("Failed to record frame: {:?}", err; {"rtc_id": stream_id});
                    }
                }
                RecorderMsg::Start {
                    dir,
                    stream_id,
                    start_time,
                } => {
                    if let Err(err) =
                        Self::handle_start(&mut recorders, stream_id, &dir, start_time)
                            .context("Start")
                    {
                        err!("Failed to create recorders: {:?}", err; {"rtc_id": stream_id})
                    } else {
                        info!("Recording to {}", dir; {"rtc_id": stream_id});
                    }
                }
                RecorderMsg::WaitStop {
                    mut waiter,
                    stream_id,
                } => {
                    if recorders.contains_key(&stream_id) {
                        waiters
                            .entry(stream_id)
                            .or_insert_with(Vec::new)
                            .push(waiter);
                    } else {
                        let _ = waiter.send(());
                    }
                }
            }
        }
    }

    fn handle_stop(
        recorders: &mut FnvHashMap<StreamId, Recorders<'_>>,
        stream_id: StreamId,
    ) -> Result<()> {
        if let Some(mut recorders) = recorders.remove(&stream_id) {
            recorders.audio.close()?;
            recorders.video.close()?;
        }
        Ok(())
    }

    fn handle_packet(
        recorders: &mut FnvHashMap<StreamId, Recorders<'_>>,
        stream_id: StreamId,
        packet: &[i8],
        is_video: bool,
    ) -> Result<()> {
        let recorders = recorders
            .get_mut(&stream_id)
            .ok_or_else(|| anyhow!("Recorders missing"))?;
        if is_video {
            recorders.video.save_frame(packet)
        } else {
            recorders.audio.save_frame(packet)
        }
    }

    fn handle_start(
        recorders: &mut FnvHashMap<StreamId, Recorders<'_>>,
        stream_id: StreamId,
        dir: &str,
        start_time: DateTime<Utc>,
    ) -> Result<()> {
        Self::create_records_dir(dir)?;
        let video_filename = format!("{}.video", start_time.timestamp_millis());
        let video = JanusRecorder::create(dir, &video_filename, Codec::VP8)?;

        let audio_filename = format!("{}.audio", start_time.timestamp_millis());
        let audio = JanusRecorder::create(dir, &audio_filename, Codec::Opus)?;

        match recorders.entry(stream_id) {
            Entry::Occupied(mut e) => {
                let mut v = e.insert(Recorders { audio, video });
                v.audio.close()?;
                v.video.close()?;
                Ok(())
            }
            Entry::Vacant(e) => {
                e.insert(Recorders { audio, video });
                Ok(())
            }
        }
    }

    fn create_records_dir(dir: &str) -> Result<(), std::io::Error> {
        if let Err(err) = fs::create_dir(&dir) {
            match err.kind() {
                std::io::ErrorKind::AlreadyExists => Ok(()),
                _ => Err(err),
            }
        } else {
            Ok(())
        }
    }
}

struct Recorders<'a> {
    audio: JanusRecorder<'a>,
    video: JanusRecorder<'a>,
}

pub fn recorder(
    config: Config,
    metrics: crate::conf::Metrics,
) -> (Recorder, RecorderHandlesCreator) {
    let (tx, rx) = crossbeam_channel::unbounded();
    (
        Recorder::new(rx, metrics.recorders_metrics_load_interval),
        RecorderHandlesCreator::new(tx, config),
    )
}

#[derive(Debug)]
pub struct RecorderHandle {
    sender: Sender<RecorderMsg>,
    stream_id: StreamId,
    save_root_dir: String,

    is_deletion_enabled: bool,
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
impl RecorderHandle {
    fn new(config: &Config, stream_id: StreamId, messages: Sender<RecorderMsg>) -> Self {
        Self {
            stream_id,
            save_root_dir: config.directory.clone(),
            is_deletion_enabled: config.delete_records,
            sender: messages,
        }
    }

    pub fn record_packet(&self, buf: &[i8], is_video: bool) -> Result<()> {
        let msg = RecorderMsg::Packet {
            buf: buf.to_vec(),
            is_video,
            stream_id: self.stream_id,
        };

        self.sender.send(msg).context("Failed to send packet")
    }

    pub fn start_recording(&self) -> Result<()> {
        info!("Start recording"; {"rtc_id": self.stream_id});

        let dir = self.get_records_dir().to_string_lossy().into_owned();

        self.sender
            .send(RecorderMsg::Start {
                stream_id: self.stream_id,
                dir,
                start_time: Utc::now(),
            })
            .context("Failed to start recording")
    }

    pub fn stop_recording(&self) -> Result<()> {
        self.sender
            .send(RecorderMsg::Stop {
                stream_id: self.stream_id,
            })
            .context("Failed to stop recording")
    }

    pub async fn wait_stop(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(RecorderMsg::WaitStop {
                waiter: tx,
                stream_id: self.stream_id,
            })
            .context("Failed to wait stop")?;
        let _ = rx.await;
        Ok(())
    }

    pub fn get_records_dir(&self) -> PathBuf {
        let mut path = PathBuf::new();
        path.push(&self.save_root_dir);
        path.push(&self.stream_id.to_string());
        path
    }

    pub fn check_existence(&self) -> Result<()> {
        let path = self.get_records_dir();
        let metadata = fs::metadata(&path).context("Record doesn't exist")?;

        if metadata.is_dir() {
            Ok(())
        } else {
            bail!(
                "Recording path {} is not a directory",
                path.to_string_lossy()
            );
        }
    }

    pub fn delete_record(&self) -> Result<()> {
        if self.is_deletion_enabled {
            fs::remove_dir_all(&self.get_records_dir()).context("Failed to delete record")
        } else {
            Ok(())
        }
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
