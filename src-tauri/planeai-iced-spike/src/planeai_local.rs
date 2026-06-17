//! PlaneAI-local session source for the Iced spike.
//!
//! Bridges the push-based TerminalOutputSink from pty_core into the
//! pull-based PlaneAiTerminalSession trait used by the spike's multi-session UI.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::adapter::PlaneAiTerminalSession;
use crate::pty_core::{LocalPtySession, TerminalOutputSink, TerminalSessionEvent, TerminalSessionHandle};

const MAX_BUFFER: usize = 512 * 1024; // 512KB, matches spike-local

/// Bounded-buffer sink: receives pushed output, blocks if full (lossless).
struct ChannelSink {
    buf: Arc<Mutex<Vec<u8>>>,
    buf_not_full: Arc<Condvar>,
    exited: Arc<AtomicBool>,
    max_pending: Arc<AtomicU64>,
}

impl TerminalOutputSink for ChannelSink {
    fn send(&self, event: TerminalSessionEvent) -> anyhow::Result<()> {
        match event {
            TerminalSessionEvent::Output { bytes, .. } => {
                let mut buf = self.buf.lock().unwrap();
                while buf.len() + bytes.len() > MAX_BUFFER {
                    buf = self.buf_not_full.wait(buf).unwrap();
                }
                buf.extend_from_slice(&bytes);
                let len = buf.len() as u64;
                let _ = self.max_pending.fetch_max(len, Ordering::Relaxed);
            }
            TerminalSessionEvent::Exit { .. } | TerminalSessionEvent::Error { .. } => {
                self.exited.store(true, Ordering::Release);
            }
        }
        Ok(())
    }
}

/// PlaneAI-local session: uses extracted PTY core with push-to-pull bridge.
pub struct PlaneAiLocalSession {
    session: LocalPtySession,
    buf: Arc<Mutex<Vec<u8>>>,
    buf_not_full: Arc<Condvar>,
    sink_exited: Arc<AtomicBool>,
    max_pending: Arc<AtomicU64>,
}

impl PlaneAiLocalSession {
    pub fn spawn(
        id: usize,
        cols: u16,
        rows: u16,
        command: Option<&str>,
    ) -> anyhow::Result<Self> {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let buf_not_full = Arc::new(Condvar::new());
        let sink_exited = Arc::new(AtomicBool::new(false));
        let max_pending = Arc::new(AtomicU64::new(0));

        let sink = Arc::new(ChannelSink {
            buf: buf.clone(),
            buf_not_full: buf_not_full.clone(),
            exited: sink_exited.clone(),
            max_pending: max_pending.clone(),
        });

        let session = LocalPtySession::spawn(id, cols, rows, command, sink)?;

        Ok(Self { session, buf, buf_not_full, sink_exited, max_pending })
    }
}

impl PlaneAiTerminalSession for PlaneAiLocalSession {
    fn id(&self) -> usize { self.session.id() }

    fn write(&self, bytes: &[u8]) -> anyhow::Result<()> {
        self.session.write(bytes)
    }

    fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.session.resize(cols, rows)
    }

    fn try_read_batch(&self) -> anyhow::Result<Option<Vec<u8>>> {
        let mut buf = self.buf.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        if buf.is_empty() { return Ok(None); }
        let data = std::mem::take(&mut *buf);
        self.buf_not_full.notify_one();
        Ok(Some(data))
    }

    fn has_exited(&self) -> bool {
        self.session.has_exited() || self.sink_exited.load(Ordering::Acquire)
    }

    fn pending_bytes(&self) -> usize {
        self.buf.lock().unwrap().len()
    }

    fn max_pending_bytes(&self) -> usize {
        self.max_pending.load(Ordering::Relaxed) as usize
    }

    fn bytes_dropped(&self) -> u64 {
        0 // Lossless: we block rather than drop
    }
}
