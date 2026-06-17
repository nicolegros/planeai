/// Adapter trait for terminal session backends.
///
/// This decouples the Iced terminal UI from any specific backend implementation.
/// Implementations:
/// - `SpikeLocalSession` (spike shell.rs) — fallback/test backend
/// - Future: `PlaneAiLocalSession` wrapping the existing LocalBackend from pty.rs
pub trait PlaneAiTerminalSession: Send {
    fn id(&self) -> usize;
    fn write(&self, bytes: &[u8]) -> anyhow::Result<()>;
    fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()>;
    fn try_read_batch(&self) -> anyhow::Result<Option<Vec<u8>>>;
    fn has_exited(&self) -> bool;
    fn pending_bytes(&self) -> usize;
    fn max_pending_bytes(&self) -> usize;
    fn bytes_dropped(&self) -> u64;
}
