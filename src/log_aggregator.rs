use std::collections::HashMap;
use std::thread;

use crate::switchboard::{SessionId, StreamId};

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Hash, Eq, PartialEq)]
pub enum Event {
    SlowLink {
        stream_id: StreamId,
        uplink: bool,
    },
    RtpReplay {
        stream_id: StreamId,
        handle_id: SessionId,
        ssrc: u32,
        seq_number: u16,
        timestamp: u32,
    },
}

impl Event {
    fn log(&self, count: usize) {
        match self {
            Self::SlowLink { stream_id, uplink } => {
                warn!(
                    "Got {} slow link events; uplink = {}",
                    count, uplink;
                    {"rtc_id": stream_id}
                );
            }
            Self::RtpReplay {
                stream_id,
                handle_id,
                ssrc,
                seq_number,
                timestamp,
            } => {
                if count > 1 {
                    warn!(
                        "Relayed {} packets more than once; ssrc = {}, seq_number = {}, timestamp = {}",
                        count, ssrc, seq_number, timestamp;
                        {"handle_id": handle_id, "rtc_id": stream_id}
                    );
                }
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
enum Message {
    Register(Event),
    Flush,
}

#[derive(Debug)]
pub struct LogAggregator {
    tx: crossbeam_channel::Sender<Message>,
}

impl LogAggregator {
    pub fn start() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded::<Message>();

        thread::spawn(move || {
            let mut state: HashMap<Event, usize> = HashMap::new();

            while let Ok(message) = rx.recv() {
                match message {
                    Message::Register(event) => {
                        state
                            .entry(event)
                            .and_modify(|count| *count += 1)
                            .or_insert(1);
                    }
                    Message::Flush => {
                        for (event, count) in state.iter() {
                            event.log(*count);
                        }

                        state.clear();
                    }
                }
            }
        });

        Self { tx }
    }

    pub fn register(&self, event: Event) {
        if let Err(err) = self.tx.send(Message::Register(event)) {
            err!("Failed to register log aggregator item: {}", err);
        }
    }

    pub fn flush(&self) {
        if let Err(err) = self.tx.send(Message::Flush) {
            err!("Failed to flush log aggregator: {}", err);
        }
    }
}
