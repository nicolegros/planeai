use std::sync::atomic::{AtomicU64, Ordering};

/// Shared atomic counters for pipeline diagnostics.
pub struct PipelineDiagnostics {
    pub reader_bytes: AtomicU64,
    pub reader_reads: AtomicU64,
    pub flusher_batches: AtomicU64,
    pub flusher_bytes: AtomicU64,
    pub flusher_wakeups: AtomicU64,
    pub flusher_sleep_ns: AtomicU64,
}

impl PipelineDiagnostics {
    pub fn new() -> Self {
        Self {
            reader_bytes: AtomicU64::new(0),
            reader_reads: AtomicU64::new(0),
            flusher_batches: AtomicU64::new(0),
            flusher_bytes: AtomicU64::new(0),
            flusher_wakeups: AtomicU64::new(0),
            flusher_sleep_ns: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> DiagnosticsSnapshot {
        DiagnosticsSnapshot {
            reader_bytes: self.reader_bytes.load(Ordering::Relaxed),
            reader_reads: self.reader_reads.load(Ordering::Relaxed),
            flusher_batches: self.flusher_batches.load(Ordering::Relaxed),
            flusher_bytes: self.flusher_bytes.load(Ordering::Relaxed),
            flusher_wakeups: self.flusher_wakeups.load(Ordering::Relaxed),
            flusher_sleep_ns: self.flusher_sleep_ns.load(Ordering::Relaxed),
        }
    }
}

impl Default for PipelineDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct DiagnosticsSnapshot {
    pub reader_bytes: u64,
    pub reader_reads: u64,
    pub flusher_batches: u64,
    pub flusher_bytes: u64,
    pub flusher_wakeups: u64,
    pub flusher_sleep_ns: u64,
}
