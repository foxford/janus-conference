use std::thread;
use std::time::Duration as StdDuration;

use anyhow::Result;
use atom::AtomSetOnce;
use chrono::Duration;

use crate::conf::Config;
use crate::message_handler::{JanusSender, MessageHandlingLoop};
use crate::switchboard::Dispatcher as SwitchboardDispatcher;

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
    pub switchboard_dispatcher: SwitchboardDispatcher,
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
            verb!("Vacuum thread spawned");

            if let Ok(app) = app!() {
                let vacuum_interval_seconds = app.config.general.vacuum_interval;
                let interval = Duration::seconds(vacuum_interval_seconds);
                let std_interval = StdDuration::from_secs(vacuum_interval_seconds as u64);

                loop {
                    app.switchboard_dispatcher
                        .dispatch_sync(move |switchboard| switchboard.vacuum_writers(&interval))
                        .unwrap_or_else(|err| {
                            err!("Vacuum dispatch error: {}", err);
                            Ok(())
                        })
                        .unwrap_or_else(|err| err!("Vacuum error: {}", err));

                    thread::sleep(std_interval);
                }
            }
        });

        Ok(())
    }

    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            config,
            switchboard_dispatcher: SwitchboardDispatcher::start(),
            message_handling_loop: MessageHandlingLoop::new(JanusSender::new()),
        })
    }
}
