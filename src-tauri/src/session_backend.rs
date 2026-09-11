use tokio::sync::oneshot;

pub enum WriteAck {
    Immediate,
    Pending(oneshot::Receiver<Result<(), String>>),
}

impl WriteAck {
    pub async fn wait(self) -> Result<(), String> {
        match self {
            Self::Immediate => Ok(()),
            Self::Pending(receiver) => receiver
                .await
                .map_err(|_| "PTY write acknowledgement channel closed".to_string())?,
        }
    }
}

pub trait SessionBackend: Send + Sync {
    fn write(&self, data: &[u8]) -> Result<WriteAck, String>;
    fn resize(&self, rows: u16, cols: u16) -> Result<(), String>;
    fn pause(&self) -> Result<(), String>;
    fn resume(&self) -> Result<(), String>;
    fn detach(&self);
}
