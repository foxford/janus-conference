use std::time::{Duration, Instant};

use crate::{message_handler::MethodKind, switchboard::Switchboard};
use http::StatusCode;
use prometheus::{HistogramOpts, HistogramVec, IntCounterVec, IntGaugeVec, Opts, Registry};
use prometheus_static_metric::make_static_metric;

make_static_metric! {
    pub struct RequestStats: IntCounter {
        "status" => {
            success,
            failure,
        },
    }

    pub struct ResponseStats: IntCounter {
        "status" => {
            success,
            client_error,
            server_error,
        },
    }
}

make_static_metric! {
    pub struct RequestDuration: Histogram {
        "method" => {
            agent_leave,
            reader_config_update,
            stream_create,
            stream_read,
            stream_upload,
            writer_config_update,
            service_ping,
        },
    }
}

make_static_metric! {
    pub struct SwitchboardStats: IntGauge {
        "field" => {
            sessions,
            agents,
            avg_sessions_per_agent,
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
            queue
        },
    }
}

pub struct Metrics {
    request_duration: RequestDuration,
    request_stats: RequestStats,
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
        let request_stats =
            IntCounterVec::new(Opts::new("request_stats", "Request stats"), &["status"])?;

        let response_stats =
            IntCounterVec::new(Opts::new("response_stats", "Response stats"), &["status"])?;

        let switchboard_stats = IntGaugeVec::new(
            Opts::new("switchboard_stats", "Switchboard stats"),
            &["field"],
        )?;
        let recorder_stats =
            IntGaugeVec::new(Opts::new("recorder_stats", "Recorder stats"), &["field"])?;

        registry.register(Box::new(request_duration.clone()))?;
        registry.register(Box::new(request_stats.clone()))?;
        registry.register(Box::new(switchboard_stats.clone()))?;
        registry.register(Box::new(recorder_stats.clone()))?;
        registry.register(Box::new(response_stats.clone()))?;
        Ok(Self {
            request_duration: RequestDuration::from(&request_duration),
            request_stats: RequestStats::from(&request_stats),
            switchboard_stats: SwitchboardStats::from(&switchboard_stats),
            recorder_stats: RecorderStats::from(&recorder_stats),
            response_stats: ResponseStats::from(&response_stats),
        })
    }

    pub fn observe_success_request() {
        if let Ok(app) = app!() {
            app.metrics.request_stats.success.inc()
        }
    }

    pub fn observe_failed_request() {
        if let Ok(app) = app!() {
            app.metrics.request_stats.failure.inc()
        }
    }

    pub fn observe_switchboard(switchboard: &Switchboard) {
        if let Ok(app) = app!() {
            let switchboard_stats = &app.metrics.switchboard_stats;

            let sessions_count = switchboard.sessions_count() as i64;
            let agents_count = switchboard.agents_count() as i64;
            let avg_sessions_per_count = sessions_count / agents_count;

            switchboard_stats.sessions.set(sessions_count);
            switchboard_stats.agents.set(agents_count);
            switchboard_stats
                .avg_sessions_per_agent
                .set(avg_sessions_per_count);
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

    pub fn observe_request(start_time: Instant, method: MethodKind, status: StatusCode) {
        let elapsed = Self::duration_to_seconds(start_time.elapsed());
        if let Ok(app) = app!() {
            if status.is_client_error() {
                app.metrics.response_stats.client_error.inc()
            } else if status.is_server_error() {
                app.metrics.response_stats.server_error.inc()
            } else {
                app.metrics.response_stats.success.inc()
            }
            let request_duration = &app.metrics.request_duration;
            match method {
                MethodKind::AgentLeave => request_duration.agent_leave.observe(elapsed),
                MethodKind::ReaderConfigUpdate => {
                    request_duration.reader_config_update.observe(elapsed)
                }

                MethodKind::StreamCreate => request_duration.stream_create.observe(elapsed),
                MethodKind::StreamRead => request_duration.stream_read.observe(elapsed),
                MethodKind::StreamUpload => request_duration.stream_upload.observe(elapsed),
                MethodKind::WriterConfigUpdate => {
                    request_duration.writer_config_update.observe(elapsed)
                }
                MethodKind::ServicePing => request_duration.service_ping.observe(elapsed),
            }
        }
    }

    pub fn observe_recorder(recorders_count: usize, queue_size: usize, waiters_size: usize) {
        if let Ok(app) = app!() {
            app.metrics
                .recorder_stats
                .recorders
                .set(recorders_count as i64);
            app.metrics.recorder_stats.queue.set(queue_size as i64);
            app.metrics.recorder_stats.waiters.set(waiters_size as i64);
        }
    }

    #[inline]
    pub fn duration_to_seconds(d: Duration) -> f64 {
        let nanos = f64::from(d.subsec_nanos()) / 1e9;
        d.as_secs() as f64 + nanos
    }
}
