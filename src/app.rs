use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use anyhow::Result;
use atom::AtomSetOnce;
use chrono::Duration;

use crate::conf::Config;
use crate::message_handler::{JanusSender, MessageHandlingLoop};
use crate::switchboard::LockedSwitchboard as Switchboard;

lazy_static! {
    pub static ref APP: AtomSetOnce<Box<App>> = AtomSetOnce::empty();
}

macro_rules! app {
    () => {
        crate::app::APP
            .get()
            .ok_or_else(|| anyhow::format_err!("App is not initialized"))
    };
}

pub struct App {
    pub config: Config,
    pub switchboard: Switchboard,
    pub message_handling_loop: MessageHandlingLoop,
    pub rtp_stats: RtpStats,
}

impl App {
    pub fn init(config: Config) -> Result<()> {
        config.sentry.as_ref().map(|sentry_config| {
            janus_info!("[CONFERENCE] Initializing Sentry");
            svc_error::extension::sentry::init(sentry_config)
        });

        let app = App::new(config)?;
        APP.set_if_none(Box::new(app));

        thread::spawn(|| {
            if let Ok(app) = app!() {
                app.message_handling_loop.start();
            }
        });

        thread::spawn(|| {
            if let Ok(app) = app!() {
                let interval = Duration::seconds(app.config.general.vacuum_interval);

                if let Err(err) = app.switchboard.vacuum_publishers_loop(interval) {
                    janus_err!("[CONFERENCE] {}", err);
                }
            }
        });

        thread::spawn(|| {
            if let Ok(app) = app!() {
                let interval = std::time::Duration::from_secs(5);

                loop {
                    app.rtp_stats.flush();
                    thread::sleep(interval);
                }
            }
        });

        Ok(())
    }

    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            config,
            switchboard: Switchboard::new(),
            message_handling_loop: MessageHandlingLoop::new(JanusSender::new()),
            rtp_stats: RtpStats::new(),
        })
    }
}

///////////////////////////////////////////////////////////////////////////////

pub struct RtpStats {
    subscribers_count: AtomicU64,
    packets_count: AtomicU64,
    total_lock_acquire_time: AtomicU64,
    total_relay_time: AtomicU64,
    total_recording_time: AtomicU64,
    total_callback_time: AtomicU64,
}

impl RtpStats {
    fn new() -> Self {
        Self {
            subscribers_count: AtomicU64::new(0),
            packets_count: AtomicU64::new(0),
            total_lock_acquire_time: AtomicU64::new(0),
            total_relay_time: AtomicU64::new(0),
            total_recording_time: AtomicU64::new(0),
            total_callback_time: AtomicU64::new(0),
        }
    }

    pub fn set_subscribers_count(&self, value: u64) {
        self.subscribers_count.store(value, Ordering::Relaxed);
    }

    pub fn increment_packets_count(&self) {
        self.packets_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_lock_acquire_time(&self, value: u64) {
        self.total_lock_acquire_time
            .fetch_add(value, Ordering::Relaxed);
    }

    pub fn add_relay_time(&self, value: u64) {
        self.total_relay_time.fetch_add(value, Ordering::Relaxed);
    }

    pub fn add_recording_time(&self, value: u64) {
        self.total_recording_time
            .fetch_add(value, Ordering::Relaxed);
    }

    pub fn add_callback_time(&self, value: u64) {
        self.total_callback_time.fetch_add(value, Ordering::Relaxed);
    }

    fn flush(&self) {
        let packets_count = self.packets_count.load(Ordering::Relaxed);

        if packets_count != 0 {
            janus_info!(
                "[CONFERENCE][RTP_STATS] {} {} {} {} {} {}",
                self.subscribers_count.load(Ordering::Relaxed),
                packets_count,
                self.total_lock_acquire_time.load(Ordering::Relaxed) as f32 / packets_count as f32,
                self.total_relay_time.load(Ordering::Relaxed) as f32 / packets_count as f32,
                self.total_recording_time.load(Ordering::Relaxed) as f32 / packets_count as f32,
                self.total_callback_time.load(Ordering::Relaxed) as f32 / packets_count as f32,
            );
        }

        self.subscribers_count.store(0, Ordering::Relaxed);
        self.packets_count.store(0, Ordering::Relaxed);
        self.total_lock_acquire_time.store(0, Ordering::Relaxed);
        self.total_relay_time.store(0, Ordering::Relaxed);
        self.total_recording_time.store(0, Ordering::Relaxed);
        self.total_callback_time.store(0, Ordering::Relaxed);
    }
}
