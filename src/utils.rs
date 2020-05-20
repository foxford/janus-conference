use anyhow::{format_err, Context, Result};
use janus::{JanssonDecodingFlags, JanssonEncodingFlags, JanssonValue};
use serde::de::DeserializeOwned;
use serde_json::Value;

// courtesy of c_string crate, which also has some other stuff we aren't interested in
// taking in as a dependency here.
macro_rules! c_str {
    ($lit:expr) => {
        unsafe { CStr::from_ptr(concat!($lit, "\0").as_ptr() as *const $crate::c_char) }
    };
}

pub fn serde_to_jansson(json: &Value) -> Result<JanssonValue> {
    JanssonValue::from_str(&json.to_string(), JanssonDecodingFlags::empty())
        .map_err(|err| format_err!("{}", err))
}

pub fn jansson_to_serde<T: DeserializeOwned>(json: &JanssonValue) -> Result<T> {
    let json = json.to_libcstring(JanssonEncodingFlags::empty());
    let json = json.to_string_lossy();
    serde_json::from_str(&json).context("Failed to parse JSON")
}
