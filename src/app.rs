use atom::AtomSetOnce;
use failure::Error;

use crate::conf::Config;
use crate::message_handler::MessageHandler;
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
    pub message_handler: MessageHandler,
    pub uploader: Uploader,
}

impl App {
    pub fn init(config: Config) -> Result<(), Error> {
        APP.set_if_none(Box::new(App::new(config)?));

        app!().and_then(|app| {
            app.message_handler.start();
            app.switchboard.start_vacuum_thread();
            Ok(())
        })
    }

    pub fn new(config: Config) -> Result<Self, Error> {
        let uploader = Uploader::new(config.uploading.clone())?;

        Ok(Self {
            config,
            switchboard: Switchboard::new(),
            message_handler: MessageHandler::new(),
            uploader,
        })
    }
}
