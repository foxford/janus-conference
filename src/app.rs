use std::thread;
use std::time::Duration;

use anyhow::Result;
use atom::AtomSetOnce;

use crate::conf::Config;
use crate::message_handler::{JanusSender, MessageHandlingLoop};
use crate::switchboard::LockedSwitchboard as Switchboard;
use crate::uploader::Uploader;

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
    pub uploader: Uploader,
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
                let interval = Duration::new(app.config.general.vacuum_interval, 0);
                app.switchboard.vacuum_publishers_loop(interval);
            }
        });

        Ok(())
    }

    pub fn new(config: Config) -> Result<Self> {
        let uploader = Uploader::build(config.uploading.clone())?;

        Ok(Self {
            config,
            switchboard: Switchboard::new(),
            message_handling_loop: MessageHandlingLoop::new(JanusSender::new()),
            uploader,
        })
    }
}
