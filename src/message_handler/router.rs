use super::operation;
use super::operation::Operation;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "method")]
pub enum Method {
    #[serde(rename = "stream.create")]
    StreamCreate(operation::stream_create::Request),
    #[serde(rename = "stream.read")]
    StreamRead(operation::stream_read::Request),
    #[serde(rename = "stream.upload")]
    StreamUpload(operation::stream_upload::Request),
}

impl Into<Box<dyn Operation>> for Method {
    fn into(self) -> Box<dyn Operation> {
        match self {
            Method::StreamCreate(op) => Box::new(op),
            Method::StreamRead(op) => Box::new(op),
            Method::StreamUpload(op) => Box::new(op),
        }
    }
}
