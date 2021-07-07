use std::{net::SocketAddr, thread};

use anyhow::Result;
use chrono::Duration;
use once_cell::sync::OnceCell;
use prometheus::{Encoder, Registry, TextEncoder};

use crate::{conf::Config, recorder::recorder};
use crate::{message_handler::JanusSender, recorder::RecorderHandlesCreator};
use crate::{metrics::Metrics, switchboard::LockedSwitchboard as Switchboard};

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
    pub metrics: Metrics,
}

impl App {
    pub fn init(config: Config) -> Result<()> {
        if let Some(sentry_config) = config.sentry.as_ref() {
            svc_error::extension::sentry::init(sentry_config);
            info!("Sentry initialized");
        }
        let (recorder, handles_creator) =
            recorder(config.recordings.clone(), config.metrics.clone());
        let metrics_registry = Registry::new();
        let metrics = Metrics::new(&metrics_registry)?;
        async_std::task::spawn(start_metrics_collector(
            metrics_registry,
            config.metrics.bind_addr,
        ));

        let app = App::new(config, handles_creator, metrics)?;
        APP.set(app).expect("Already initialized");
        thread::spawn(|| recorder.start());

        thread::spawn(|| loop {
            if let Ok(app) = app!() {
                let _ = app.switchboard.with_read_lock(|switchboard| {
                    Metrics::observe_switchboard(&switchboard);
                    Ok(())
                });
                thread::sleep(app.config.metrics.switchboard_metrics_load_interval)
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

    pub fn new(
        config: Config,
        recorders_creator: RecorderHandlesCreator,
        metrics: Metrics,
    ) -> Result<Self> {
        Ok(Self {
            config,
            switchboard: Switchboard::new(),
            recorders_creator,
            janus_sender: JanusSender::new(),
            metrics,
        })
    }
}

async fn start_metrics_collector(
    registry: Registry,
    bind_addr: SocketAddr,
) -> async_std::io::Result<()> {
    let mut app = tide::with_state(registry);
    app.at("/metrics")
        .get(|req: tide::Request<Registry>| async move {
            let registry = req.state();
            let mut buffer = vec![];
            let encoder = TextEncoder::new();
            let metric_families = registry.gather();
            match encoder.encode(&metric_families, &mut buffer) {
                Ok(_) => {
                    let mut response = tide::Response::new(200);
                    response.set_body(buffer);
                    Ok(response)
                }
                Err(err) => {
                    warn!("Metrics not gathered: {:#}", err);
                    Ok(tide::Response::new(500))
                }
            }
        });
    app.listen(bind_addr).await
}
