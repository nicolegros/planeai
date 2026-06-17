//! Tauri-independent local PTY core.
//!
//! Extracted from PlaneAI production backend (pty.rs).
//! Provides: TerminalSessionEvent, TerminalOutputSink, TerminalSessionHandle,
//! and a local PTY spawner with reader/flusher thread coalescing.

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

// ─── Shared Contract ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum TerminalSessionEvent {
    Output { session_id: usize, bytes: Vec<u8> },
    Exit { session_id: usize, status: Option<i32> },
    Error { session_id: usize, message: String },
}

pub trait TerminalOutputSink: Send + Sync + 'static {
    fn send(&self, event: TerminalSessionEvent) -> anyhow::Result<()>;
}

pub trait TerminalSessionHandle: Send {
    fn id(&self) -> usize;
    fn write(&self, bytes: &[u8]) -> anyhow::Result<()>;
    fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()>;
    fn pause(&self) -> anyhow::Result<()> { Ok(()) }
    fn resume(&self) -> anyhow::Result<()> { Ok(()) }
    fn kill(&self) -> anyhow::Result<()> { Ok(()) }
    fn has_exited(&self) -> bool;
}

// ─── Flow Control (from production pty.rs) ───────────────────────────────────

struct FlowControl {
    paused: Mutex<bool>,
    cond: Condvar,
}

impl FlowControl {
    fn new() -> Self {
        Self { paused: Mutex::new(false), cond: Condvar::new() }
    }
    fn pause(&self) { *self.paused.lock().unwrap() = true; }
    fn resume(&self) {
        let mut p = self.paused.lock().unwrap();
        *p = false;
        self.cond.notify_one();
    }
    fn wait_if_paused(&self) {
        let mut p = self.paused.lock().unwrap();
        while *p { p = self.cond.wait(p).unwrap(); }
    }
}

// ─── Constants (from production pty.rs) ──────────────────────────────────────

const FLUSH_COALESCE: Duration = Duration::from_millis(4);
const FLUSH_MAX_IDLE: Duration = Duration::from_millis(50);
const READ_BUF: usize = 16 * 1024;

// ─── Local PTY Session ───────────────────────────────────────────────────────

pub struct LocalPtySession {
    id: usize,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    flow: Arc<FlowControl>,
    exited: Arc<AtomicBool>,
}

impl LocalPtySession {
    /// Spawn a local PTY with the production reader/flusher coalescing pattern.
    /// Output is pushed through the provided sink.
    pub fn spawn(
        id: usize,
        cols: u16,
        rows: u16,
        command: Option<&str>,
        sink: Arc<dyn TerminalOutputSink>,
    ) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows, cols, pixel_width: 0, pixel_height: 0,
        })?;

        let mut cmd = if let Some(cmd_str) = command {
            let mut c = CommandBuilder::new("bash");
            c.args(["-c", cmd_str]);
            c
        } else {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            CommandBuilder::new(&shell)
        };
        cmd.env("TERM", "xterm-256color");

        pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;
        let flow = Arc::new(FlowControl::new());
        let exited = Arc::new(AtomicBool::new(false));

        // Shared coalescing buffer between reader and flusher
        let pending: Arc<(Mutex<Vec<u8>>, Condvar)> =
            Arc::new((Mutex::new(Vec::with_capacity(READ_BUF)), Condvar::new()));
        let done = Arc::new(AtomicBool::new(false));

        // Reader thread
        let pending_r = pending.clone();
        let done_r = done.clone();
        let flow_r = flow.clone();
        thread::spawn(move || {
            let mut buf = [0u8; READ_BUF];
            loop {
                flow_r.wait_if_paused();
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let (lock, cv) = &*pending_r;
                        let mut g = lock.lock().unwrap();
                        g.extend_from_slice(&buf[..n]);
                        cv.notify_one();
                    }
                    Err(_) => break,
                }
            }
            done_r.store(true, Ordering::Release);
            pending_r.1.notify_one();
        });

        // Flusher thread
        let pending_f = pending.clone();
        let done_f = done;
        let exited_f = exited.clone();
        let flow_f = flow.clone();
        thread::spawn(move || {
            let (lock, cv) = &*pending_f;
            loop {
                {
                    let mut g = lock.lock().unwrap();
                    while g.is_empty() {
                        if done_f.load(Ordering::Acquire) {
                            if !g.is_empty() {
                                let chunk = std::mem::take(&mut *g);
                                let _ = sink.send(TerminalSessionEvent::Output {
                                    session_id: id, bytes: chunk,
                                });
                            }
                            exited_f.store(true, Ordering::Release);
                            let _ = sink.send(TerminalSessionEvent::Exit {
                                session_id: id, status: None,
                            });
                            return;
                        }
                        let (next, _) = cv.wait_timeout(g, FLUSH_MAX_IDLE).unwrap();
                        g = next;
                    }
                }

                flow_f.wait_if_paused();
                thread::sleep(FLUSH_COALESCE);

                let chunk = std::mem::take(&mut *lock.lock().unwrap());
                if chunk.is_empty() { continue; }
                if sink.send(TerminalSessionEvent::Output {
                    session_id: id, bytes: chunk,
                }).is_err() {
                    break;
                }
            }
            exited_f.store(true, Ordering::Release);
        });

        Ok(Self {
            id,
            writer: Arc::new(Mutex::new(writer)),
            master: Arc::new(Mutex::new(pair.master)),
            flow,
            exited,
        })
    }
}

impl TerminalSessionHandle for LocalPtySession {
    fn id(&self) -> usize { self.id }

    fn write(&self, bytes: &[u8]) -> anyhow::Result<()> {
        let mut w = self.writer.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        w.write_all(bytes)?;
        w.flush()?;
        Ok(())
    }

    fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        let m = self.master.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        Ok(())
    }

    fn pause(&self) -> anyhow::Result<()> { self.flow.pause(); Ok(()) }
    fn resume(&self) -> anyhow::Result<()> { self.flow.resume(); Ok(()) }
    fn kill(&self) -> anyhow::Result<()> { Ok(()) }
    fn has_exited(&self) -> bool { self.exited.load(Ordering::Acquire) }
}
