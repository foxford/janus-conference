use std::fs::File;
use std::io::Read;
use std::path::Path;

use failure::Error;
use toml;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub recording: Recording,
}

impl Config {
    pub fn from_path(s: &Path) -> Result<Self, Error> {
        let mut f = File::open(s)?;
        let mut config_str = String::new();
        f.read_to_string(&mut config_str)?;

        let config: Self = toml::from_str(&config_str).map_err(|err| Error::from(err))?;

        config.recording.check()?;

        Ok(config)
    }
}

#[derive(Deserialize, Debug)]
pub struct Recording {
    pub root_save_directory: String,
}

impl Recording {
    pub fn check(&self) -> Result<(), Error> {
        if !Path::new(&self.root_save_directory).exists() {
            return Err(format_err!(
                "Recording: root_save_directory {} does not exist",
                self.root_save_directory
            ));
        }

        Ok(())
    }
}
