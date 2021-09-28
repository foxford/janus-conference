use std::time::Duration;

use crate::conf::Description;

pub fn register(description: &Description, conference_url: &str) {
    let register = || {
        let desc = serde_json::to_vec(&description)?;
        let response = ureq::post(conference_url).send_bytes(&desc)?;
        if response.status() == 200 {
            Ok(())
        } else {
            Err(anyhow!("Not registered"))
        }
    };
    while let Err(err) = register() {
        err!("Janus not registered: {:?}", err);
        std::thread::sleep(Duration::from_secs(1))
    }
}
