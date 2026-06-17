pub type SessionId = usize;

#[derive(Debug, Clone)]
pub enum PtyEvent {
    Output { session_id: SessionId, bytes: Vec<u8> },
    Exit { session_id: SessionId, status: Option<i32> },
    Error { session_id: SessionId, message: String },
}

pub trait PtyEventSink: Send + Sync + 'static {
    fn send(&self, event: PtyEvent) -> anyhow::Result<()>;
}
