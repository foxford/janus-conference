#[derive(Deserialize, Serialize)]
#[serde(remote = "http::StatusCode")]
pub(crate) struct HttpStatusCodeRef(#[serde(getter = "http_status_code_to_string")] String);

fn http_status_code_to_string(status_code: &http::StatusCode) -> String {
    status_code.as_u16().to_string()
}

impl From<HttpStatusCodeRef> for http::StatusCode {
    fn from(value: HttpStatusCodeRef) -> http::StatusCode {
        use std::str::FromStr;

        http::StatusCode::from_str(&value.0).unwrap()
    }
}
