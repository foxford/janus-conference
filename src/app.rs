use std::ffi::CString;
use std::thread;
use std::time::Duration;

use atom::AtomSetOnce;
use failure::Error;
use tokio_threadpool::ThreadPool;

use crate::conf::Config;
use crate::janus_callbacks;
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
    pub thread_pool: ThreadPool,
}

impl App {
    pub fn init(config: Config) -> Result<(), Error> {
        APP.set_if_none(Box::new(App::new(config)?));
        Ok(())
    }

    pub fn new(config: Config) -> Result<Self, Error> {
        let uploader = Uploader::new(config.uploading.clone())?;

        Ok(Self {
            config,
            switchboard: Switchboard::new(),
            message_handler: MessageHandler::new(),
            uploader,
            thread_pool: ThreadPool::new(),
        })
    }

    pub fn handle_messages(&self) {
        janus_info!("[CONFERENCE] Message processing thread is alive.");

        self.message_handler.run(|msg, response, jsep| {
            janus_info!("[CONFERENCE] Sending response ({})", msg.transaction());

            let push_result = CString::new(msg.transaction().to_owned()).map(|transaction| {
                janus_callbacks::push_event(&msg.session(), transaction.into_raw(), response, jsep)
            });

            if let Err(err) = push_result {
                janus_err!("[CONFERENCE] Error pushing event: {}", err);
            }
        });
    }

    pub fn vacuum_publishers(&self) {
        let interval = Duration::new(self.config.general.vacuum_interval, 0);

        loop {
            thread::sleep(interval);

            let result = self.switchboard.with_read_lock(|switchboard| {
                switchboard.vacuum_publishers(&interval);
                Ok(())
            });

            result.unwrap_or_else(|err| janus_err!("[CONFERENCE] {}", err));
        }
    }
}
