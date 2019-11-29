use std::thread;
use std::time::Duration;

use atom::AtomSetOnce;
use failure::Error;

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
            .ok_or_else(|| failure::err_msg("App is not initialized"))
    };
}

pub struct App {
    pub config: Config,
    pub switchboard: Switchboard,
    pub message_handling_loop: MessageHandlingLoop,
    pub uploader: Uploader,
}

impl App {
    pub fn init(config: Config) -> Result<(), Error> {
        config.sentry.as_ref().map(|sentry_config| {
            janus_verb!("[CONFERENCE] Initializing Sentry");
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

    pub fn new(config: Config) -> Result<Self, Error> {
        let uploader = Uploader::new(config.uploading.clone())?;

        Ok(Self {
            config,
            switchboard: Switchboard::new(),
            message_handling_loop: MessageHandlingLoop::new(JanusSender::new()),
            uploader,
        })
    }
}
