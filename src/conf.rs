use std::path::Path;

use anyhow::Result;

use crate::recorder;

const CONFIG_FILE_NAME: &str = "janus.plugin.conference.toml";

#[derive(Clone, Deserialize, Debug)]
pub struct Config {
    pub general: General,
    pub recordings: recorder::Config,
    pub constraint: Constraint,
    pub sentry: Option<svc_error::extension::sentry::Config>,
    pub upload: UploadConfig,
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
pub struct General {
    pub vacuum_interval: i64,
}

#[derive(Clone, Deserialize, Debug)]
pub struct Constraint {
    pub writer: WriterConstraint,
}

#[derive(Clone, Deserialize, Debug)]
pub struct WriterConstraint {
    pub bitrate: Option<u32>,
}

#[derive(Clone, Deserialize, Debug)]
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
