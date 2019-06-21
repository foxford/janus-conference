use std::fs;
use std::path::{Path, PathBuf};

use failure::{err_msg, Error};
use rand::Rng;

const TEST_RECORDING_PATH: &str = "./tests/testsuite/files/recording";
const RECORDINGS_DIR: &str = "./recordings";

// Test recording directory with some video files. The directory gets deleted on drop.
pub struct TestRecording {
    id: String,
    path: PathBuf,
}

impl TestRecording {
    pub fn new() -> Result<Self, Error> {
        let mut rng = rand::thread_rng();
        let id = rng.gen::<u64>().to_string();
        let path = Path::new(RECORDINGS_DIR).join(&id);
        fs::create_dir(&path)?;
        Self::copy_test_files(&path)?;
        Ok(Self { id, path })
    }

    pub fn id(&self) -> &String {
        &self.id
    }

    fn copy_test_files(destination_path: &PathBuf) -> Result<(), Error> {
        for entry in fs::read_dir(TEST_RECORDING_PATH)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("mkv") {
                let name = path
                    .file_name()
                    .ok_or_else(|| err_msg("Failed to get file name"))?;

                fs::copy(&path, &destination_path.join(&name))?;
            }
        }

        Ok(())
    }
}

impl Drop for TestRecording {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_dir_all(&self.path) {
            panic!("Failed to cleanup test recording: {}", err);
        }
    }
}
