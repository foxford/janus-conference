use std::{net::SocketAddr, path::Path, time::Duration};

use anyhow::Result;

use crate::{janus_rtp::AudioLevel, recorder};

const CONFIG_FILE_NAME: &str = "janus.plugin.conference.toml";

#[derive(Clone, Deserialize, Debug)]
pub struct Config {
    pub general: General,
    pub recordings: recorder::Config,
    pub speaking_notifications: Option<SpeakingNotifications>,
    pub constraint: Constraint,
    pub sentry: Option<svc_error::extension::sentry::Config>,
    pub upload: UploadConfig,
    pub metrics: Metrics,
    pub registry: Option<RegistryConfig>,
    pub switchboard: SwitchboardConfig,
}

impl Config {
    pub fn from_path(p: &Path) -> Result<Self> {
        let mut p = p.to_path_buf();
        p.push(CONFIG_FILE_NAME);

        let p = p.to_string_lossy();
        info!("Reading config located at {}", p);

        let mut parser = config::Config::default();
        parser.merge(config::File::new(&p, config::FileFormat::Toml))?;
        parser.merge(config::Environment::with_prefix("APP").separator("__"))?;

        let mut config = parser.try_into::<Config>()?;

        config.recordings.check()?;
        config.upload.check()?;

        Ok(config)
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct SwitchboardConfig {
    #[serde(default = "SwitchboardConfig::default_max_sessions_per_agent")]
    pub max_sessions_per_agent: usize,
    pub max_agents: Option<usize>,
}

impl SwitchboardConfig {
    fn default_max_sessions_per_agent() -> usize {
        1
    }

    pub fn set_max_agents_if_empty(mut self, max_agents: usize) -> Self {
        if self.max_agents.is_none() {
            self.max_agents = Some(max_agents);
        }

        self
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct RegistryConfig {
    pub conference_url: String,
    pub description: Description,
    pub token: String,
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct Description {
    pub capacity: Option<i32>,
    pub balancer_capacity: Option<i32>,
    pub group: Option<String>,
    pub janus_url: String,
    pub agent_id: String,
}

#[derive(Clone, Deserialize, Debug)]
pub struct General {
    #[serde(with = "humantime_serde")]
    pub vacuum_interval: Duration,
    #[serde(with = "humantime_serde")]
    pub fir_interval: Duration,
    #[serde(with = "humantime_serde")]
    pub sessions_ttl: Duration,
    pub health_check_addr: SocketAddr,
}

#[derive(Clone, Deserialize, Debug)]
pub struct Metrics {
    #[serde(with = "humantime_serde")]
    pub switchboard_metrics_load_interval: Duration,
    #[serde(with = "humantime_serde")]
    pub recorders_metrics_load_interval: Duration,
    pub bind_addr: SocketAddr,
}

#[derive(Clone, Deserialize, Debug)]
pub struct Constraint {
    pub writer: WriterConstraint,
}

#[derive(Clone, Deserialize, Debug)]
pub struct WriterConstraint {
    pub default_video_bitrate: u32,
    pub max_video_remb: u32,
    pub audio_bitrate: u32,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct UploadBackendConfig {
    access_key_id: String,
    secret_access_key: String,
    endpoint: String,
    region: String,
}

#[derive(Clone, Deserialize, Debug)]
pub struct UploadConfig {
    pub backends: Vec<String>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct SpeakingNotifications {
    pub audio_active_packets: usize,
    pub speaking_average_level: AudioLevel,
    pub not_speaking_average_level: AudioLevel,
}

impl UploadConfig {
    fn check(&self) -> Result<()> {
        for backend in &self.backends {
            let prefix = format!("APP_UPLOADING_{}", backend.to_uppercase());
            let env = config::Environment::with_prefix(&prefix).separator("__");

            let mut parser = config::Config::default();
            parser.merge(env)?;
            parser.try_into::<UploadBackendConfig>()?;
        }

        Ok(())
    }
}
