/// Pipeline diagnostics collected from a session backend.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PipelineDiag {
    pub pty_reader_bytes_total: u64,
    pub pty_reader_reads_total: u64,
    pub flusher_batches_total: u64,
    pub flusher_bytes_total: u64,
    pub flusher_wakeups_total: u64,
    pub flusher_sleep_ms_total: f64,
    pub sink_send_calls_total: u64,
    pub sink_send_bytes_total: u64,
    pub output_queue_capacity_bytes: usize,
    pub max_pending_pty_output_bytes: u64,
    pub queue_depth_at_end_bytes: usize,
    pub output_bytes_dropped: u64,
    pub producer_block_count: u64,
    pub producer_block_duration_ms: f64,
}

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
    fn pipeline_diag(&self) -> PipelineDiag { PipelineDiag::default() }
}
