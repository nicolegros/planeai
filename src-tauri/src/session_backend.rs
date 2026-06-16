/// Trait abstracting the PTY operations for different session backends.
pub trait SessionBackend: Send + Sync {
    fn write(&self, data: &[u8]) -> Result<(), String>;
    fn resize(&self, rows: u16, cols: u16) -> Result<(), String>;
    fn pause(&self) -> Result<(), String>;
    fn resume(&self) -> Result<(), String>;
    fn detach(&self);
}
