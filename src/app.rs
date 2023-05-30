use std::{net::SocketAddr, thread};

use anyhow::Result;
use once_cell::sync::OnceCell;
use prometheus::{Encoder, Registry, TextEncoder};

use crate::{conf::Config, recorder::recorder, register};
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
    pub fir_interval: chrono::Duration,
}

impl App {
    pub fn init(config: Config) -> Result<()> {
        if let Some(sentry_config) = config.sentry.as_ref() {
            svc_error::extension::sentry::init(sentry_config);
            info!("Sentry initialized");
        }
        let healh_check = start_health_check(config.general.health_check_addr);
        if let Some(registry) = config.registry.clone() {
            thread::spawn(move || {
                register::register(
                    &registry.description,
                    &registry.conference_url,
                    &registry.token,
                );
                async_std::task::spawn(healh_check);
            });
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
                if let Err(err) = app.switchboard.vacuum_publishers_loop(
                    app.config.general.vacuum_interval,
                    app.config.general.sessions_ttl,
                ) {
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
        let switchboard_cfg = config.switchboard.clone();
        Ok(Self {
            fir_interval: chrono::Duration::from_std(config.general.fir_interval)?,
            config,
            switchboard: Switchboard::new(switchboard_cfg),
            recorders_creator,
            janus_sender: JanusSender::new(),
            metrics,
        })
    }
}

async fn start_health_check(bind_addr: SocketAddr) {
    let mut app = tide::new();
    app.at("/")
        .get(|_req: tide::Request<()>| async move { Ok(tide::Response::new(200)) });
    if let Err(err) = app.listen(bind_addr).await {
        err!("Healthcheck errored: {:?}", err)
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
