use std::fs::File;
use std::io::Read;
use std::path::Path;

use failure::Error;
use toml;

use recorder::RecordingConfig;
use uploader::UploadingConfig;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub recordings: RecordingConfig,
    #[serde(skip)]
    pub uploading: UploadingConfig,
}

impl Config {
    pub fn from_path(s: &Path) -> Result<Self, Error> {
        let mut f = File::open(s)?;
        let mut config_str = String::new();
        f.read_to_string(&mut config_str)?;

        let mut config: Self = toml::from_str(&config_str).map_err(|err| Error::from(err))?;

        config.recordings.check()?;
        config.uploading.check()?;

        Ok(config)
    }
}
