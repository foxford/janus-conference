use std::path::Path;

use config;
use failure::Error;

use recorder;
use uploader;

const CONFIG_FILE_NAME: &str = "janus.plugin.conference.toml";

#[derive(Deserialize, Debug)]
pub struct Config {
    pub recordings: recorder::Config,
    pub uploading: uploader::Config,
}

impl Config {
    pub fn from_path(p: &Path) -> Result<Self, Error> {
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
