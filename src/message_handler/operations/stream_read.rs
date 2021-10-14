use crate::switchboard::{AgentId, StreamId};
use anyhow::Result;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    pub id: StreamId,
    pub agent_id: AgentId,
}
impl Request {
    pub fn stream_read(&self, request: &super::Request) -> Result<()> {
        verb!("Calling stream.read operation"; {"rtc_id": self.id});

        app!()?.switchboard.with_write_lock(|mut switchboard| {
            switchboard.join_stream(self.id, request.session_id(), self.agent_id.to_owned())
        })?;

        Ok(())
    }
}
