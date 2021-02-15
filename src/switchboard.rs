use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{bail, format_err, Result};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use janus::session::SessionWrapper;
use uuid::Uuid;

use crate::bidirectional_multimap::BidirectionalMultimap;
use crate::janus_callbacks;
use crate::janus_rtp::JanusRtpSwitchingContext;
use crate::recorder::Recorder;

///////////////////////////////////////////////////////////////////////////////

pub type StreamId = Uuid;
pub type Session = Box<Arc<SessionWrapper<SessionId>>>;
pub type LockedSession = Arc<Mutex<Session>>;

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct SessionId(u64);

impl SessionId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct SessionState {
    switching_context: JanusRtpSwitchingContext,
    fir_seq: AtomicI32,
    initial_rembs_counter: AtomicU64,
    last_remb_timestamp: AtomicI64,
    last_rtp_packet_timestamp: AtomicI64,
    recorder: Option<Recorder>,
}

impl SessionState {
    fn new() -> Self {
        Self {
            switching_context: JanusRtpSwitchingContext::new(),
            fir_seq: AtomicI32::new(0),
            initial_rembs_counter: AtomicU64::new(0),
            last_remb_timestamp: AtomicI64::new(0),
            last_rtp_packet_timestamp: AtomicI64::new(0),
            recorder: None,
        }
    }

    pub fn switching_context(&self) -> &JanusRtpSwitchingContext {
        &self.switching_context
    }

    pub fn increment_fir_seq(&self) -> i32 {
        self.fir_seq.fetch_add(1, Ordering::Relaxed)
    }

    pub fn initial_rembs_counter(&self) -> u64 {
        self.initial_rembs_counter.load(Ordering::Relaxed)
    }

    pub fn increment_initial_rembs_counter(&self) -> u64 {
        self.initial_rembs_counter.fetch_add(1, Ordering::Relaxed)
    }

    pub fn last_remb_timestamp(&self) -> Option<DateTime<Utc>> {
        match self.last_remb_timestamp.load(Ordering::Relaxed) {
            0 => None,
            timestamp => {
                let naive_dt = NaiveDateTime::from_timestamp(timestamp, 0);
                Some(DateTime::from_utc(naive_dt, Utc))
            }
        }
    }

    pub fn touch_last_remb_timestamp(&self) {
        self.last_remb_timestamp
            .store(Utc::now().timestamp(), Ordering::Relaxed);
    }

    fn since_last_rtp_packet_timestamp(&self) -> Option<Duration> {
        match self.last_rtp_packet_timestamp.load(Ordering::Relaxed) {
            0 => None,
            timestamp => {
                let naive_dt = NaiveDateTime::from_timestamp(timestamp, 0);
                let dt = DateTime::from_utc(naive_dt, Utc);
                Some(Utc::now() - dt)
            }
        }
    }

    pub fn touch_last_rtp_packet_timestamp(&self) {
        self.last_rtp_packet_timestamp
            .store(Utc::now().timestamp(), Ordering::Relaxed);
    }

    pub fn reset(&mut self) {
        self.fir_seq.store(0, Ordering::Relaxed);
        self.initial_rembs_counter.store(0, Ordering::Relaxed);
        self.last_remb_timestamp.store(0, Ordering::Relaxed);
        self.last_rtp_packet_timestamp.store(0, Ordering::Relaxed);
        self.unset_recorder();
    }

    pub fn recorder(&self) -> Option<&Recorder> {
        self.recorder.as_ref()
    }

    pub fn recorder_mut(&mut self) -> Option<&mut Recorder> {
        self.recorder.as_mut()
    }

    pub fn set_recorder(&mut self, recorder: Recorder) -> &mut Self {
        self.recorder = Some(recorder);
        self
    }

    fn unset_recorder(&mut self) -> &mut Self {
        self.recorder = None;
        self
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct Switchboard {
    sessions: HashMap<SessionId, LockedSession>,
    states: HashMap<SessionId, SessionState>,
    writers: BidirectionalMultimap<SessionId, StreamId>,
    readers: BidirectionalMultimap<SessionId, StreamId>,
}

impl Switchboard {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            states: HashMap::new(),
            writers: BidirectionalMultimap::new(),
            readers: BidirectionalMultimap::new(),
        }
    }

    pub fn connect(&mut self, session: Session) -> Result<()> {
        let session_id = ***session;
        info!("Connecting session"; {"handle_id": session_id});
        let locked_session = Arc::new(Mutex::new(session));
        self.sessions.insert(session_id, locked_session);
        self.states.insert(session_id, SessionState::new());
        Ok(())
    }

    pub fn disconnect(&mut self, session_id: SessionId) -> Result<()> {
        info!("Disconnecting session asynchronously"; {"handle_id": session_id});

        let session = self.session(session_id)?.lock().map_err(|err| {
            format_err!("Failed to acquire session mutex {}: {}", session_id, err)
        })?;

        janus_callbacks::end_session(&session);
        Ok(())
    }

    pub fn handle_disconnect(&mut self, session_id: SessionId) -> Result<()> {
        info!(
            "Session is about to disconnect. Removing it from the switchboard.";
            {"handle_id": session_id}
        );

        self.sessions.remove(&session_id);
        self.states.remove(&session_id);
        self.writers.remove_key(&session_id);
        self.readers.remove_key(&session_id);
        Ok(())
    }

    pub fn session(&self, id: SessionId) -> Result<&LockedSession> {
        self.sessions
            .get(&id)
            .ok_or_else(|| format_err!("Session not found for id = {}", id))
    }

    pub fn state(&self, id: SessionId) -> Result<&SessionState> {
        self.states
            .get(&id)
            .ok_or_else(|| format_err!("Session state not found for id = {}", id))
    }

    fn state_mut(&mut self, id: SessionId) -> Result<&mut SessionState> {
        self.states
            .get_mut(&id)
            .ok_or_else(|| format_err!("Session state not found for id = {}", id))
    }

    pub fn writer_to(&self, reader: SessionId) -> Option<SessionId> {
        self.readers
            .get_values(&reader)
            .first()
            .and_then(|stream_id| self.writer_of(*stream_id))
    }

    pub fn writer_of(&self, stream_id: StreamId) -> Option<SessionId> {
        self.writers
            .get_keys(&stream_id)
            .first()
            .map(|id| id.to_owned())
    }

    pub fn written_by(&self, writer: SessionId) -> Option<StreamId> {
        self.writers.get_values(&writer).first().copied()
    }

    pub fn set_writer(&mut self, stream_id: StreamId, writer: SessionId) -> Result<()> {
        let app = app!()?;
        self.remove_writer(stream_id)?;
        info!("Setting writer"; {"rtc_id": stream_id, "handle_id": writer});
        self.writers.associate(writer, stream_id);

        if app.config.recordings.enabled {
            let mut recorder = Recorder::new(&app.config.recordings, stream_id);

            if let Err(err) = recorder.start_recording() {
                err!("Failed to start recording; stopping the stream"; {"rtc_id": stream_id});

                self.remove_stream(stream_id).map_err(|remove_err| {
                    format_err!(
                        "Failed to remove stream {}: {} while recovering from another error: {}",
                        stream_id,
                        remove_err,
                        err
                    )
                })?;

                return Err(err);
            }

            verb!("Attaching recorder"; {"rtc_id": stream_id, "handle_id": writer});
            self.state_mut(writer)?.set_recorder(recorder);
        }

        Ok(())
    }

    pub fn remove_writer(&mut self, stream_id: StreamId) -> Result<()> {
        let writer = match self.writer_of(stream_id) {
            Some(writer) => writer,
            None => return Ok(()),
        };

        self.state_mut(writer)?.reset();
        info!("Removing writer"; {"rtc_id": stream_id, "handle_id": writer});
        self.writers.remove_key(&writer);
        Ok(())
    }

    pub fn readers_of(&self, stream_id: StreamId) -> &[SessionId] {
        self.readers.get_keys(&stream_id)
    }

    pub fn read_by(&self, reader: SessionId) -> Option<StreamId> {
        self.readers.get_values(&reader).first().copied()
    }

    pub fn add_reader(&mut self, stream_id: StreamId, reader: SessionId) {
        verb!("Adding reader"; {"rtc_id": stream_id, "handle_id": reader});

        if self.read_by(reader) != Some(stream_id) {
            self.readers.associate(reader, stream_id);
        }
    }

    pub fn remove_reader(&mut self, stream_id: StreamId, reader: SessionId) {
        verb!("Removing reader"; {"rtc_id": stream_id, "handle_id": reader});
        self.readers.remove_pair(&reader, &stream_id);
    }

    pub fn stream_id_to(&self, session_id: SessionId) -> Option<StreamId> {
        self.readers
            .get_values(&session_id)
            .first()
            .copied()
            .or_else(|| self.written_by(session_id))
    }

    pub fn remove_stream(&mut self, stream_id: StreamId) -> Result<()> {
        info!("Removing stream"; {"rtc_id": stream_id});

        if let Some(writer) = self.writers.get_keys(&stream_id).to_owned().first() {
            self.stop_recording(*writer)?;
        }

        self.writers.remove_value(&stream_id);
        self.readers.remove_value(&stream_id);
        Ok(())
    }

    fn stop_recording(&mut self, writer: SessionId) -> Result<()> {
        let state = self.state_mut(writer)?;

        if let Some(recorder) = state.recorder_mut() {
            info!("Stopping recording"; {"handle_id": writer});

            recorder
                .stop_recording()
                .map_err(|err| format_err!("Failed to stop recording {}: {}", writer, err))?;
        }

        state.unset_recorder();
        Ok(())
    }

    pub fn vacuum_writers(&mut self, timeout: &Duration) -> Result<()> {
        let writers = self
            .writers
            .iter_all()
            .flat_map(|(k, values)| values.iter().map(move |v| (*k, *v)))
            .collect::<Vec<(SessionId, StreamId)>>();

        for (writer, stream_id) in writers {
            match self.vacuum_writer(writer, timeout) {
                Ok(false) => (),
                Ok(true) => warn!(
                    "Writer timed out; No RTP packets from PeerConnection in {} seconds",
                    timeout.num_seconds();
                    {"rtc_id": stream_id, "handle_id": writer}
                ),
                Err(err) => err!(
                    "Failed to vacuum writer: {}", err;
                    {"rtc_id": stream_id, "handle_id": writer}
                ),
            }
        }

        Ok(())
    }

    fn vacuum_writer(&mut self, writer: SessionId, timeout: &Duration) -> Result<bool> {
        let state = self.state(writer)?;

        let is_timed_out = match state.since_last_rtp_packet_timestamp() {
            None => false,
            Some(duration) => duration >= *timeout,
        };

        if is_timed_out {
            self.disconnect(writer)?;
        }

        Ok(is_timed_out)
    }
}

///////////////////////////////////////////////////////////////////////////////

const DISPATCHER_MAX_QUEUE_SIZE: usize = 1000;

enum DispatcherMessage {
    Dispatch(Box<dyn FnOnce(&mut Switchboard) + Send>),
    Halt,
}

pub struct Dispatcher {
    tx: crossbeam_channel::Sender<DispatcherMessage>,
}

impl Dispatcher {
    pub fn start() -> Self {
        let (tx, rx) = crossbeam_channel::bounded(DISPATCHER_MAX_QUEUE_SIZE);

        thread::spawn(move || {
            let mut switchboard = Switchboard::new();

            while let Ok(message) = rx.recv() {
                match message {
                    DispatcherMessage::Dispatch(callback) => callback(&mut switchboard),
                    DispatcherMessage::Halt => break,
                }
            }
        });

        Self { tx }
    }

    pub async fn dispatch<F, R>(&self, callback: F) -> Result<R>
    where
        F: FnOnce(&mut Switchboard) -> R + Send + 'static,
        R: Send + 'static,
    {
        let (ret_tx, ret_rx) = futures::channel::oneshot::channel();

        let boxed_callback = Box::new(move |switchboard: &mut Switchboard| {
            if let Err(_) = ret_tx.send(callback(switchboard)) {
                err!("Failed to send return value from switchboard dispatcher");
            }
        });

        let send_result = self
            .tx
            .try_send(DispatcherMessage::Dispatch(boxed_callback));

        if let Err(err) = send_result {
            bail!("Failed to send dispatch message to switchboard: {}", err);
        }

        ret_rx.await.map_err(|err| {
            anyhow!(
                "Failed to receive return value from switchboard dispatcher: {}",
                err
            )
        })
    }

    pub fn dispatch_sync<F, R>(&self, callback: F) -> Result<R>
    where
        F: FnOnce(&mut Switchboard) -> R + Send + 'static,
        R: Send + 'static,
    {
        async_std::task::block_on(async { self.dispatch(callback).await })
    }
}

impl Drop for Dispatcher {
    fn drop(&mut self) {
        if let Err(err) = self.tx.send(DispatcherMessage::Halt) {
            err!(
                "Failed to send halt message to switchboard dispatcher: {}",
                err
            );
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::ptr;

    use janus::PluginSession;
    use janus_plugin_sys::janus_refcount;
    use uuid::Uuid;

    use crate::app::App;
    use crate::conf::Config;

    use super::*;

    #[test]
    fn connect_two_streams() -> Result<()> {
        init_app()?;
        let mut switchboard = Switchboard::new();

        // Connect writers and readers.
        let writer1 = connect(&mut switchboard, 1)?;
        let reader11 = connect(&mut switchboard, 11)?;
        let reader12 = connect(&mut switchboard, 12)?;

        let writer2 = connect(&mut switchboard, 2)?;
        let reader21 = connect(&mut switchboard, 21)?;
        let reader22 = connect(&mut switchboard, 22)?;

        // Bind writers and readers to streams.
        let stream1 = Uuid::new_v4();
        switchboard.set_writer(stream1, writer1)?;
        switchboard.add_reader(stream1, reader11);
        switchboard.add_reader(stream1, reader12);

        let stream2 = Uuid::new_v4();
        switchboard.set_writer(stream2, writer2)?;
        switchboard.add_reader(stream2, reader21);
        switchboard.add_reader(stream2, reader22);

        for session_id in &[writer1, reader11, reader12, writer2, reader21, reader22] {
            // Assert session getter.
            {
                let locked_session = switchboard
                    .session(*session_id)?
                    .lock()
                    .expect("Failed to obtain session lock");

                assert_eq!(****locked_session, *session_id);
            }

            // Assert session setter and getter.
            {
                let state = switchboard.state_mut(*session_id)?;
                state.increment_initial_rembs_counter();
            }

            let state = switchboard.state(*session_id)?;
            assert_eq!(state.initial_rembs_counter(), 1);
        }

        // Assert writer_to.
        assert_eq!(switchboard.writer_to(reader11), Some(writer1));
        assert_eq!(switchboard.writer_to(reader12), Some(writer1));
        assert_eq!(switchboard.writer_to(reader21), Some(writer2));
        assert_eq!(switchboard.writer_to(reader22), Some(writer2));
        assert_eq!(switchboard.writer_to(writer1), None);
        assert_eq!(switchboard.writer_to(SessionId::new(100)), None);

        // Assert writer_of.
        assert_eq!(switchboard.writer_of(stream1), Some(writer1));
        assert_eq!(switchboard.writer_of(stream2), Some(writer2));
        assert_eq!(switchboard.writer_of(Uuid::new_v4()), None);

        // Assert written_by.
        assert_eq!(switchboard.written_by(writer1), Some(stream1));
        assert_eq!(switchboard.written_by(writer2), Some(stream2));
        assert_eq!(switchboard.written_by(reader11), None);
        assert_eq!(switchboard.written_by(SessionId::new(100)), None);

        // Assert readers_of.
        let stream1_readers = switchboard.readers_of(stream1);
        assert!(stream1_readers.contains(&reader11));
        assert!(stream1_readers.contains(&reader12));

        let stream2_readers = switchboard.readers_of(stream2);
        assert!(stream2_readers.contains(&reader21));
        assert!(stream2_readers.contains(&reader22));

        // Assert read_by.
        assert_eq!(switchboard.read_by(reader11), Some(stream1));
        assert_eq!(switchboard.read_by(reader12), Some(stream1));
        assert_eq!(switchboard.read_by(reader21), Some(stream2));
        assert_eq!(switchboard.read_by(reader22), Some(stream2));
        assert_eq!(switchboard.read_by(writer2), None);
        assert_eq!(switchboard.read_by(SessionId::new(100)), None);

        // Assert stream_id_to.
        assert_eq!(switchboard.stream_id_to(writer1), Some(stream1));
        assert_eq!(switchboard.stream_id_to(reader11), Some(stream1));
        assert_eq!(switchboard.stream_id_to(reader12), Some(stream1));
        assert_eq!(switchboard.stream_id_to(writer2), Some(stream2));
        assert_eq!(switchboard.stream_id_to(reader21), Some(stream2));
        assert_eq!(switchboard.stream_id_to(reader22), Some(stream2));
        assert_eq!(switchboard.stream_id_to(SessionId::new(100)), None);

        Ok(())
    }

    #[test]
    fn disconnect_writer() -> Result<()> {
        init_app()?;
        let mut switchboard = Switchboard::new();

        // Connect a writer and readers.
        let writer = connect(&mut switchboard, 0)?;
        let reader1 = connect(&mut switchboard, 1)?;
        let reader2 = connect(&mut switchboard, 2)?;

        // Bind writer and readers to a stream.
        let stream = Uuid::new_v4();
        switchboard.set_writer(stream, writer)?;
        switchboard.add_reader(stream, reader1);
        switchboard.add_reader(stream, reader2);

        // Disconnect writer.
        switchboard.handle_disconnect(writer)?;

        // Assert writer to be missing.
        assert!(switchboard.session(writer).is_err());
        assert!(switchboard.state(writer).is_err());
        assert_eq!(switchboard.writer_of(stream), None);
        assert_eq!(switchboard.written_by(writer), None);
        assert_eq!(switchboard.written_by(writer), None);
        assert_eq!(switchboard.stream_id_to(writer), None);

        // Assert readers to be still on the stream.
        let stream_readers = switchboard.readers_of(stream);
        assert!(stream_readers.contains(&reader1));
        assert!(stream_readers.contains(&reader2));

        for reader in &[reader1, reader2] {
            assert!(switchboard.session(*reader).is_ok());
            assert!(switchboard.state(*reader).is_ok());
            assert_eq!(switchboard.read_by(*reader), Some(stream));
            assert_eq!(switchboard.stream_id_to(*reader), Some(stream));
            assert_eq!(switchboard.writer_to(*reader), None);
        }

        Ok(())
    }

    #[test]
    fn disconnect_reader() -> Result<()> {
        init_app()?;
        let mut switchboard = Switchboard::new();

        // Connect a writer and readers.
        let writer = connect(&mut switchboard, 0)?;
        let reader1 = connect(&mut switchboard, 1)?;
        let reader2 = connect(&mut switchboard, 2)?;

        // Bind writer and readers to a stream.
        let stream = Uuid::new_v4();
        switchboard.set_writer(stream, writer)?;
        switchboard.add_reader(stream, reader1);
        switchboard.add_reader(stream, reader2);

        // Disconnect a reader.
        switchboard.handle_disconnect(reader1)?;

        // Assert the disconnect reader to be missing.
        assert!(switchboard.session(reader1).is_err());
        assert!(switchboard.state(reader1).is_err());
        assert_eq!(switchboard.read_by(reader1), None);
        assert_eq!(switchboard.stream_id_to(reader1), None);

        // Assert only the other reader on the stream.
        let stream_readers = switchboard.readers_of(stream);
        assert_eq!(stream_readers.len(), 1);
        assert_eq!(stream_readers[0], reader2);

        assert!(switchboard.session(reader2).is_ok());
        assert!(switchboard.state(reader2).is_ok());
        assert_eq!(switchboard.read_by(reader2), Some(stream));
        assert_eq!(switchboard.stream_id_to(reader2), Some(stream));

        // Assert the writer is still on the stream.
        assert_eq!(switchboard.writer_of(stream), Some(writer));
        assert!(switchboard.session(writer).is_ok());
        assert!(switchboard.state(writer).is_ok());
        assert_eq!(switchboard.written_by(writer), Some(stream));
        assert_eq!(switchboard.stream_id_to(writer), Some(stream));

        assert_eq!(switchboard.writer_to(reader1), None);
        assert_eq!(switchboard.writer_to(reader2), Some(writer));

        Ok(())
    }

    #[test]
    fn replace_writer() -> Result<()> {
        init_app()?;
        let mut switchboard = Switchboard::new();

        // Connect two sessions.
        let session1 = connect(&mut switchboard, 1)?;
        let session2 = connect(&mut switchboard, 2)?;

        // Make the first a writer and the second a reader.
        let stream = Uuid::new_v4();
        switchboard.set_writer(stream, session1)?;

        // Switch the writer to the second session.
        switchboard.set_writer(stream, session2)?;

        // Assert the second one has become a writer and the first one a reader.
        assert_eq!(switchboard.writer_of(stream), Some(session2));
        assert_eq!(switchboard.written_by(session1), None);
        assert_eq!(switchboard.written_by(session2), Some(stream));

        Ok(())
    }

    #[test]
    fn remove_writer() -> Result<()> {
        init_app()?;
        let mut switchboard = Switchboard::new();

        // Connect a session and make it a writer.
        let session = connect(&mut switchboard, 0)?;
        let stream = Uuid::new_v4();
        switchboard.set_writer(stream, session)?;

        // Remove the writer from the stream.
        switchboard.remove_writer(stream)?;

        // Assert there's no writer on the stream.
        assert_eq!(switchboard.writer_of(stream), None);
        assert_eq!(switchboard.written_by(session), None);

        Ok(())
    }

    #[test]
    fn remove_reader() -> Result<()> {
        init_app()?;
        let mut switchboard = Switchboard::new();

        // Connect two sessions and make them readers.
        let reader1 = connect(&mut switchboard, 1)?;
        let reader2 = connect(&mut switchboard, 2)?;

        let stream = Uuid::new_v4();
        switchboard.add_reader(stream, reader1);
        switchboard.add_reader(stream, reader2);

        // Remove the reader from the stream.
        switchboard.remove_reader(stream, reader1);

        // Assert there's no reader on the stream.
        assert_eq!(switchboard.readers_of(stream), &[reader2]);
        assert_eq!(switchboard.read_by(reader1), None);

        Ok(())
    }

    #[test]
    fn remove_stream() -> Result<()> {
        init_app()?;
        let mut switchboard = Switchboard::new();

        // Connect a writer and readers and bind them to the stream.
        let writer = connect(&mut switchboard, 0)?;
        let reader1 = connect(&mut switchboard, 1)?;
        let reader2 = connect(&mut switchboard, 2)?;

        let stream = Uuid::new_v4();
        switchboard.set_writer(stream, writer)?;
        switchboard.add_reader(stream, reader1);
        switchboard.add_reader(stream, reader2);

        // Remove the stream.
        switchboard.remove_stream(stream)?;

        // Assert all sessions have gone.
        assert_eq!(switchboard.writer_of(stream), None);
        assert!(switchboard.readers_of(stream).is_empty());

        Ok(())
    }

    #[test]
    fn add_reader_twice() -> Result<()> {
        init_app()?;
        let mut switchboard = Switchboard::new();
        let reader = connect(&mut switchboard, 0)?;

        let stream = Uuid::new_v4();
        switchboard.add_reader(stream, reader);
        switchboard.add_reader(stream, reader);

        assert_eq!(switchboard.readers_of(stream), &[reader]);

        Ok(())
    }

    #[test]
    fn dispatch_async() -> Result<()> {
        init_app()?;

        async_std::task::block_on(async {
            let dispatcher = Dispatcher::start();
            let session = build_session(123)?;
            let session_id = ***session;

            dispatcher
                .dispatch(move |switchboard| switchboard.connect(session))
                .await?
                .expect("Failed to connect");

            dispatcher
                .dispatch(move |switchboard| {
                    assert!(switchboard.session(session_id).is_ok());
                })
                .await
        })
    }

    #[test]
    fn dispatch_sync() -> Result<()> {
        init_app()?;
        let dispatcher = Dispatcher::start();
        let session = build_session(123)?;
        let session_id = ***session;

        dispatcher
            .dispatch_sync(move |switchboard| switchboard.connect(session))?
            .expect("Failed to connect");

        dispatcher
            .dispatch_sync(move |switchboard| assert!(switchboard.session(session_id).is_ok()))
    }

    ////////////////////////////////////////////////////////////////////////////

    fn init_app() -> Result<()> {
        let config = Config::from_path(Path::new("test"))?;
        App::init(config)
    }

    static mut PLUGIN_SESSION: PluginSession = PluginSession {
        gateway_handle: ptr::null_mut(),
        plugin_handle: ptr::null_mut(),
        stopped: 0,
        ref_: janus_refcount { count: 1, free },
    };

    extern "C" fn free(_ref: *const janus_refcount) {}

    fn build_session(id: u64) -> Result<Session> {
        let plugin_session_mut_ptr = unsafe { &mut PLUGIN_SESSION as *mut PluginSession };
        let session_id = SessionId::new(id);

        unsafe { SessionWrapper::associate(plugin_session_mut_ptr, session_id) }
            .map_err(|err| anyhow!("Failed to build session: {}", err))
    }

    fn connect(switchboard: &mut Switchboard, id: u64) -> Result<SessionId> {
        let session = build_session(id)?;
        let session_id = ***session;
        switchboard.connect(session)?;
        Ok(session_id)
    }
}
