use std::env;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use failure::Error;
use toml;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub recordings: Recordings,
    #[serde(skip)]
    pub uploading: Uploading,
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

#[derive(Deserialize, Debug)]
pub struct Recordings {
    pub recordings_directory: String,
}

impl Recordings {
    pub fn check(&mut self) -> Result<(), Error> {
        if !Path::new(&self.recordings_directory).exists() {
            return Err(format_err!(
                "Recordings: recordings directory {} does not exist",
                self.recordings_directory
            ));
        }

        Ok(())
    }
}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Uploading {
    pub bucket: String,
    pub region: String,
    pub endpoint: String,
    pub access_key: String,
    pub secret_key: String,
}

impl Uploading {
    pub fn check(&mut self) -> Result<(), Error> {
        self.region = env::var("AWS_REGION")?;
        self.endpoint = env::var("AWS_ENDPOINT")?;
        self.access_key = env::var("AWS_ACCESS_KEY_ID")?;
        self.secret_key = env::var("AWS_SECRET_ACCESS_KEY")?;
        self.bucket = env::var("AWS_BUCKET")?;

        Ok(())
    }
}
