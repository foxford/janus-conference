use std::thread;

use anyhow::Result;
use chrono::Duration;
use once_cell::sync::OnceCell;

use crate::switchboard::LockedSwitchboard as Switchboard;
use crate::{conf::Config, recorder::recorder};
use crate::{message_handler::JanusSender, recorder::RecorderHandlesCreator};

pub static APP: OnceCell<App> = OnceCell::new();

macro_rules! app {
    () => {
        crate::app::APP
            .get()
            .ok_or_else(|| anyhow::format_err!("App is not initialized"))
    };
}

#[derive(Debug)]
pub struct App {
    pub config: Config,
    pub switchboard: Switchboard,
    pub recorders_creator: RecorderHandlesCreator,
    pub janus_sender: JanusSender,
}

impl App {
    pub fn init(config: Config) -> Result<()> {
        if let Some(sentry_config) = config.sentry.as_ref() {
            svc_error::extension::sentry::init(sentry_config);
            info!("Sentry initialized");
        }
        let (recorder, handles_creator) = recorder(config.recordings.clone());

        let app = App::new(config, handles_creator)?;
        APP.set(app).expect("Already initialized");

        thread::spawn(|| recorder.start());

        thread::spawn(|| {
            if let Ok(app) = app!() {
                let interval = Duration::seconds(app.config.general.vacuum_interval);

                if let Err(err) = app.switchboard.vacuum_publishers_loop(interval) {
                    err!("Vacuum publishers loop failed: {}", err);
                }
            }
        });

        Ok(())
    }

    pub fn new(config: Config, recorders_creator: RecorderHandlesCreator) -> Result<Self> {
        Ok(Self {
            config,
            switchboard: Switchboard::new(),
            recorders_creator,
            janus_sender: JanusSender::new(),
        })
    }
}
