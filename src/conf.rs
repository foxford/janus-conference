use std::path::Path;

use anyhow::Result;
use config;

use crate::recorder;

const CONFIG_FILE_NAME: &str = "janus.plugin.conference.toml";

#[derive(Clone, Deserialize, Debug)]
pub struct Config {
    pub general: General,
    pub relay: Relay,
    pub recordings: recorder::Config,
    pub constraint: Constraint,
    pub sentry: Option<svc_error::extension::sentry::Config>,
}

impl Config {
    pub fn from_path(p: &Path) -> Result<Self> {
        let mut p = p.to_path_buf();
        p.push(CONFIG_FILE_NAME);

        let p = p.to_string_lossy();

        janus_info!("[CONFERENCE] Reading config located at {}", p);

        let mut parser = config::Config::default();
        parser.merge(config::File::new(&p, config::FileFormat::Toml))?;
        parser.merge(config::Environment::with_prefix("APP").separator("__"))?;

        let mut config = parser.try_into::<Config>()?;

        config.recordings.check()?;

        Ok(config)
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct General {
    pub vacuum_interval: i64,
}

#[derive(Clone, Deserialize, Debug)]
pub struct Relay {
    pub threads: usize,
}

#[derive(Clone, Deserialize, Debug)]
pub struct Constraint {
    pub publisher: PublisherConstraint,
}

#[derive(Clone, Deserialize, Debug)]
pub struct PublisherConstraint {
    pub bitrate: Option<u32>,
}
