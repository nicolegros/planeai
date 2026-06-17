use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use crate::config::LocalPtyConfig;
use crate::diagnostics::PipelineDiagnostics;
use crate::event::{PtyEvent, PtyEventSink, SessionId};
use crate::flow_control::FlowControl;

/// A local PTY session with reader/flusher coalescing threads.
pub struct LocalPtySession {
    id: SessionId,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send>>>,
    flow: Arc<FlowControl>,
    exited: Arc<AtomicBool>,
    pub diag: Arc<PipelineDiagnostics>,
}

impl LocalPtySession {
    /// Spawn a local PTY session from config. Output pushed through the sink.
    pub fn spawn(config: LocalPtyConfig, sink: Arc<dyn PtyEventSink>) -> anyhow::Result<Self> {
        let id = config.session_id;
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: config.rows,
            cols: config.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = if let Some(ref cmd_str) = config.command {
            let mut c = CommandBuilder::new("bash");
            c.args(["-c", cmd_str]);
            c
        } else {
            let shell = config.shell.clone().unwrap_or_else(|| {
                std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
            });
            CommandBuilder::new(&shell)
        };
        cmd.env("TERM", "xterm-256color");
        if let Some(ref cwd) = config.cwd {
            cmd.cwd(cwd);
        }
        for (k, v) in &config.env {
            cmd.env(k, v);
        }

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;
        let flow = Arc::new(FlowControl::new());
        let exited = Arc::new(AtomicBool::new(false));
        let diag = Arc::new(PipelineDiagnostics::new());

        let read_buf_size = config.read_buffer_size;
        let coalesce = Duration::from_millis(config.coalesce_ms);
        let coalesce_threshold = config.coalesce_threshold_bytes;
        let flush_max_idle = Duration::from_millis(50);

        let pending: Arc<(Mutex<Vec<u8>>, Condvar)> =
            Arc::new((Mutex::new(Vec::with_capacity(read_buf_size)), Condvar::new()));
        let done = Arc::new(AtomicBool::new(false));

        // Reader thread
        {
            let pending = pending.clone();
            let done = done.clone();
            let flow = flow.clone();
            let diag = diag.clone();
            thread::spawn(move || {
                let mut buf = vec![0u8; read_buf_size];
                loop {
                    flow.wait_if_paused();
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            diag.reader_reads.fetch_add(1, Ordering::Relaxed);
                            diag.reader_bytes.fetch_add(n as u64, Ordering::Relaxed);
                            let (lock, cv) = &*pending;
                            let mut g = lock.lock().unwrap();
                            g.extend_from_slice(&buf[..n]);
                            cv.notify_one();
                        }
                        Err(_) => break,
                    }
                }
                done.store(true, Ordering::Release);
                pending.1.notify_one();
            });
        }

        // Flusher thread
        {
            let pending = pending.clone();
            let done = done;
            let exited = exited.clone();
            let flow = flow.clone();
            let diag = diag.clone();
            thread::spawn(move || {
                let (lock, cv) = &*pending;
                loop {
                    {
                        let mut g = lock.lock().unwrap();
                        while g.is_empty() {
                            if done.load(Ordering::Acquire) {
                                if !g.is_empty() {
                                    let chunk = std::mem::take(&mut *g);
                                    diag.flusher_batches.fetch_add(1, Ordering::Relaxed);
                                    diag.flusher_bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                                    let _ = sink.send(PtyEvent::Output {
                                        session_id: id, bytes: chunk,
                                    });
                                }
                                exited.store(true, Ordering::Release);
                                let _ = sink.send(PtyEvent::Exit {
                                    session_id: id, status: None,
                                });
                                return;
                            }
                            diag.flusher_wakeups.fetch_add(1, Ordering::Relaxed);
                            let (next, _) = cv.wait_timeout(g, flush_max_idle).unwrap();
                            g = next;
                        }
                        // Only coalesce if buffer is small — skip sleep under flood
                        if g.len() < coalesce_threshold {
                            drop(g);
                            flow.wait_if_paused();
                            let t = std::time::Instant::now();
                            thread::sleep(coalesce);
                            diag.flusher_sleep_ns.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                        } else {
                            drop(g);
                            flow.wait_if_paused();
                        }
                    }

                    let chunk = std::mem::take(&mut *lock.lock().unwrap());
                    if chunk.is_empty() { continue; }
                    diag.flusher_batches.fetch_add(1, Ordering::Relaxed);
                    diag.flusher_bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                    if sink.send(PtyEvent::Output {
                        session_id: id, bytes: chunk,
                    }).is_err() {
                        break;
                    }
                }
                exited.store(true, Ordering::Release);
            });
        }

        Ok(Self { id, writer: Arc::new(Mutex::new(writer)), master: Arc::new(Mutex::new(pair.master)), child: Arc::new(Mutex::new(child)), flow, exited, diag })
    }

    pub fn id(&self) -> SessionId { self.id }

    pub fn write(&self, bytes: &[u8]) -> anyhow::Result<()> {
        let mut w = self.writer.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        w.write_all(bytes)?;
        w.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        let m = self.master.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        Ok(())
    }

    pub fn pause(&self) { self.flow.pause(); }
    pub fn resume(&self) { self.flow.resume(); }
    pub fn kill(&self) -> anyhow::Result<()> {
        if let Ok(mut child) = self.child.lock() {
            child.kill()?;
        }
        Ok(())
    }
    pub fn has_exited(&self) -> bool { self.exited.load(Ordering::Acquire) }
    pub fn diagnostics(&self) -> &Arc<PipelineDiagnostics> { &self.diag }
}

impl Drop for LocalPtySession {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}
