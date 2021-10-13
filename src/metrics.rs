use crate::switchboard::Switchboard;
use prometheus::{
    HistogramOpts, HistogramTimer, HistogramVec, IntCounterVec, IntGaugeVec, Opts, Registry,
};
use prometheus_static_metric::make_static_metric;

make_static_metric! {
    pub struct ResponseStats: IntCounter {
        "status" => {
            success,
            server_error,
        },
    }
}

make_static_metric! {
    pub struct RequestDuration: Histogram {
        "method" => {
            reader_config_update,
            proxy,
            stream_upload,
            writer_config_update,
        },
    }
}

make_static_metric! {
    pub struct SwitchboardStats: IntGauge {
        "field" => {
            sessions,
            agents,
            publishers,
            publishers_subscribers,
            reader_configs,
            writer_configs,
            unused_sessions,
        },
    }
}

make_static_metric! {
    pub struct RecorderStats: IntGauge {
        "field" => {
            recorders,
            waiters,
        },
    }
}

pub struct Metrics {
    request_duration: RequestDuration,
    response_stats: ResponseStats,
    switchboard_stats: SwitchboardStats,
    recorder_stats: RecorderStats,
}

impl std::fmt::Debug for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Metrics")
    }
}

impl Metrics {
    pub fn new(registry: &Registry) -> anyhow::Result<Self> {
        let request_duration = HistogramVec::new(
            HistogramOpts::new("request_duration", "Request duration"),
            &["method"],
        )?;
        let response_stats =
            IntCounterVec::new(Opts::new("response_stats", "Response stats"), &["status"])?;

        let switchboard_stats = IntGaugeVec::new(
            Opts::new("switchboard_stats", "Switchboard stats"),
            &["field"],
        )?;
        let recorder_stats =
            IntGaugeVec::new(Opts::new("recorder_stats", "Recorder stats"), &["field"])?;

        registry.register(Box::new(request_duration.clone()))?;
        registry.register(Box::new(switchboard_stats.clone()))?;
        registry.register(Box::new(recorder_stats.clone()))?;
        registry.register(Box::new(response_stats.clone()))?;
        Ok(Self {
            request_duration: RequestDuration::from(&request_duration),
            switchboard_stats: SwitchboardStats::from(&switchboard_stats),
            recorder_stats: RecorderStats::from(&recorder_stats),
            response_stats: ResponseStats::from(&response_stats),
        })
    }

    pub fn observe_success_response() {
        if let Ok(app) = app!() {
            app.metrics.response_stats.success.inc()
        }
    }

    pub fn observe_failed_response() {
        if let Ok(app) = app!() {
            app.metrics.response_stats.server_error.inc()
        }
    }

    pub fn observe_switchboard(switchboard: &Switchboard) {
        if let Ok(app) = app!() {
            let switchboard_stats = &app.metrics.switchboard_stats;
            switchboard_stats
                .sessions
                .set(switchboard.sessions_count() as i64);
            switchboard_stats
                .agents
                .set(switchboard.agents_count() as i64);
            switchboard_stats
                .publishers
                .set(switchboard.publishers_count() as i64);
            switchboard_stats
                .publishers_subscribers
                .set(switchboard.publishers_subscribers_count() as i64);
            switchboard_stats
                .reader_configs
                .set(switchboard.reader_configs_count() as i64);
            switchboard_stats
                .writer_configs
                .set(switchboard.writer_configs_count() as i64);
            switchboard_stats
                .unused_sessions
                .set(switchboard.unused_sessions_count() as i64)
        }
    }

    pub fn observe_recorder(recorders_count: usize, waiters_size: usize) {
        if let Ok(app) = app!() {
            app.metrics
                .recorder_stats
                .recorders
                .set(recorders_count as i64);
            app.metrics.recorder_stats.waiters.set(waiters_size as i64);
        }
    }

    pub fn start_proxy() -> Option<HistogramTimer> {
        Some(app!().ok()?.metrics.request_duration.proxy.start_timer())
    }

    pub fn start_reader_config() -> Option<HistogramTimer> {
        Some(
            app!()
                .ok()?
                .metrics
                .request_duration
                .reader_config_update
                .start_timer(),
        )
    }

    pub fn start_writer_config() -> Option<HistogramTimer> {
        Some(
            app!()
                .ok()?
                .metrics
                .request_duration
                .writer_config_update
                .start_timer(),
        )
    }

    pub fn start_upload() -> Option<HistogramTimer> {
        Some(
            app!()
                .ok()?
                .metrics
                .request_duration
                .stream_upload
                .start_timer(),
        )
    }
}
