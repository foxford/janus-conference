pub type RoomId = String;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum JsepKind {
  Offer { sdp: String },
  Answer { sdp: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum RTCOperation {
  #[serde(rename = "rtc.create")]
  Create { room_id: RoomId },
  #[serde(rename = "rtc.read")]
  Read { room_id: RoomId },
}
