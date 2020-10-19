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
}

impl App {
    pub fn init(config: Config) -> Result<()> {
        if let Some(sentry_config) = config.sentry.as_ref() {
            svc_error::extension::sentry::init(sentry_config);
            info!("Sentry initialized");
        }

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
                    err!("Vacuum publishers loop failed: {}", err);
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
        })
    }
}
