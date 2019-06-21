use std::fs;
use std::path::{Path, PathBuf};

use failure::Error;
use rand::Rng;

const RECORDINGS_DIR: &str = "./recordings";
const RECORDING_EXTENSION: &str = "mkv";

/// Stream with random id that cleans up recordings on drop.
pub struct TestStream {
    id: String,
    path: PathBuf,
}

impl TestStream {
    pub fn new() -> Self {
        let mut rng = rand::thread_rng();
        let id = rng.gen::<u64>().to_string();
        let path = Path::new(RECORDINGS_DIR).join(&id);
        Self { id, path }
    }

    pub fn id(&self) -> &String {
        &self.id
    }

    /// Returns paths to recorded files.
    pub fn recordings(&self) -> Result<Vec<PathBuf>, Error> {
        let recordings = fs::read_dir(&self.path)?
            .filter_map(|maybe_entry| {
                if let Ok(entry) = maybe_entry {
                    let path = entry.path();

                    if path.is_file()
                        && path.extension().and_then(|e| e.to_str()) == Some(RECORDING_EXTENSION)
                    {
                        return Some(path);
                    }
                }

                return None;
            })
            .collect();

        Ok(recordings)
    }
}

impl Drop for TestStream {
    // Clean up recording after the test.
    fn drop(&mut self) {
        if self.path.is_dir() {
            if let Err(err) = fs::remove_dir_all(&self.path) {
                panic!("Failed to cleanup test recording: {}", err);
            }
        }
    }
}
