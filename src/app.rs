use std::{net::SocketAddr, thread};

use anyhow::Result;
use axum::{handler::get, Router};
use http::StatusCode;
use once_cell::sync::OnceCell;
use prometheus::Registry;
use reqwest::Client;
use svc_utils::metrics::MetricsServer;
use tokio::runtime::{self};

use crate::{
    conf::Config,
    http::{
        client::JanusClient,
        server::{router, stream_upload::Uploader},
    },
    recorder::recorder,
    register,
};
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
    pub uploader: Uploader,
}

impl App {
    pub fn init(config: Config) -> Result<()> {
        let metrics_registry = Registry::new();
        let metrics = Metrics::new(&metrics_registry)?;
        let (recorder, handles_creator) =
            recorder(config.recordings.clone(), config.metrics.clone());
        thread::spawn(|| recorder.start());

        let uploader = Uploader::start();

        let app = App::new(config.clone(), handles_creator, metrics, uploader)?;
        APP.set(app).expect("Already initialized");

        thread::spawn(move || {
            runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Runtime building error")
                .block_on(async {
                    let janus_client = JanusClient::new(
                        config.general.janus_url.parse()?,
                        config.general.skip_events,
                    )
                    .await;
                    let server_router = router(janus_client);
                    let server_task = tokio::spawn(
                        axum::Server::bind(&config.general.bind_addr)
                            .serve(server_router.into_make_service()),
                    );
                    register::register(
                        &Client::new(),
                        &config.registry.description,
                        &config.registry.conference_url,
                        &config.registry.token,
                    )
                    .await;

                    let healthcheck_task =
                        tokio::spawn(start_health_check(config.general.health_check_addr));
                    let switchboard_observe_task = tokio::spawn(async {
                        loop {
                            if let Ok(app) = app!() {
                                let _ = app.switchboard.with_read_lock(|switchboard| {
                                    Metrics::observe_switchboard(&switchboard);
                                    Ok(())
                                });
                                tokio::time::sleep(
                                    app.config.metrics.switchboard_metrics_load_interval,
                                )
                                .await
                            }
                        }
                    });
                    let vacuum_task = tokio::spawn(async {
                        if let Ok(app) = app!() {
                            if let Err(err) = app
                                .switchboard
                                .vacuum_publishers_loop(
                                    app.config.general.vacuum_interval,
                                    app.config.general.sessions_ttl,
                                )
                                .await
                            {
                                err!("Vacuum publishers loop failed: {}", err);
                            }
                        }
                    });
                    let _metrics_server = MetricsServer::new_with_registry(
                        metrics_registry,
                        config.metrics.bind_addr,
                    );
                    if let Err(err) = tokio::try_join!(
                        server_task,
                        healthcheck_task,
                        switchboard_observe_task,
                        vacuum_task,
                    ) {
                        fatal!("Tokio thread exited: {:?}", err);
                    }
                    Ok::<(), anyhow::Error>(())
                })
        });

        Ok(())
    }

    pub fn new(
        config: Config,
        recorders_creator: RecorderHandlesCreator,
        metrics: Metrics,
        uploader: Uploader,
    ) -> Result<Self> {
        Ok(Self {
            fir_interval: chrono::Duration::from_std(config.general.fir_interval)?,
            config,
            switchboard: Switchboard::new(),
            recorders_creator,
            janus_sender: JanusSender::new(),
            metrics,
            uploader,
        })
    }
}

async fn start_health_check(bind_addr: SocketAddr) {
    let app = Router::new().route("/", get(|| async { StatusCode::OK }));
    axum::Server::bind(&bind_addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
