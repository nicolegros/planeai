use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;
use std::time::Duration;
use tauri::ipc::{Channel, Response};
use tauri::{AppHandle, Emitter};

use crate::daemon_client::DataConnection;
use crate::output_observer::{NoopObserver, OutputObserver};
use crate::pty_planeai_core_adapter::PlaneaiPtyBackend;
use crate::session_backend::SessionBackend;
#[cfg(not(windows))]
use crate::tmux;
use planeai_pty::FlowControl;

/// Describes what command to run inside the PTY.
pub enum PtyTarget {
    /// Attach to an existing tmux session.
    TmuxAttach { tmux_name: String },
    /// Spawn a command string in a local PTY (shell tabs and agent sessions).
    /// The command is wrapped in the platform shell (`bash -c` on Unix, `cmd /C` on Windows).
    Shell { command: String, cwd: String },
    /// Attach to a daemon-managed session via data connection.
    Daemon {
        session_id: String,
        socket_path: PathBuf,
    },
}

// Flusher coalesces output so bursts arrive as single chunks.
const FLUSH_COALESCE: Duration = Duration::from_millis(4);
const FLUSH_MAX_IDLE: Duration = Duration::from_millis(50);
const READ_BUF: usize = 16 * 1024;

// ─── Daemon Backend ──────────────────────────────────────────────────────────

struct DaemonBackend {
    writer: Arc<tokio::sync::Mutex<tokio::io::WriteHalf<planeai_ipc::r#async::AsyncIpcStream>>>,
    cancelled: Arc<AtomicBool>,
    flow: Arc<FlowControl>,
}

impl SessionBackend for DaemonBackend {
    fn write(&self, data: &[u8]) -> Result<(), String> {
        let writer = self.writer.clone();
        let data = data.to_vec();
        tauri::async_runtime::spawn(async move {
            let mut w = writer.lock().await;
            let _ = planeai_daemon::protocol::write_frame(
                &mut *w,
                planeai_daemon::protocol::FRAME_INPUT,
                &data,
            )
            .await;
        });
        Ok(())
    }

    fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        let writer = self.writer.clone();
        let mut payload = [0u8; 4];
        payload[0..2].copy_from_slice(&cols.to_be_bytes());
        payload[2..4].copy_from_slice(&rows.to_be_bytes());
        tauri::async_runtime::spawn(async move {
            let mut w = writer.lock().await;
            let _ = planeai_daemon::protocol::write_frame(
                &mut *w,
                planeai_daemon::protocol::FRAME_RESIZE,
                &payload,
            )
            .await;
        });
        Ok(())
    }

    fn pause(&self) -> Result<(), String> {
        self.flow.pause();
        Ok(())
    }

    fn resume(&self) -> Result<(), String> {
        self.flow.resume();
        Ok(())
    }

    fn detach(&self) {
        self.cancelled.store(true, Ordering::Release);
        // Unblock the flusher if paused so it can exit
        self.flow.resume();
    }
}

// ─── PtyManager ──────────────────────────────────────────────────────────────

pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, Box<dyn SessionBackend>>>>,
    observer: RwLock<Arc<dyn OutputObserver>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            observer: RwLock::new(Arc::new(NoopObserver)),
        }
    }

    pub fn set_observer(&self, observer: Arc<dyn OutputObserver>) {
        *self.observer.write().unwrap() = observer;
    }

    /// Attach a PTY to a session. The command run inside depends on the PtyTarget variant.
    ///
    /// `env` is the pre-built environment for the PTY process.
    /// For local/tmux shell targets, callers should use `prepare_session()` to build the
    /// canonical env (PATH, TERM, PLANEAI_SESSION_ID) and add UI-specific vars on top.
    pub fn attach(
        &self,
        session_id: &str,
        target: PtyTarget,
        app: AppHandle,
        on_data: Channel<Response>,
        env: Vec<(String, String)>,
    ) -> Result<(), String> {
        // Handle daemon target via async path
        if let PtyTarget::Daemon {
            session_id: sid,
            socket_path,
        } = target
        {
            return self.attach_daemon(&sid, socket_path, app, on_data);
        }

        let (command, cwd) = match target {
            PtyTarget::Shell { command, cwd } => (command, cwd),
            PtyTarget::TmuxAttach { tmux_name } => {
                #[cfg(not(windows))]
                {
                    let tmux_bin = tmux::tmux_bin().to_string();
                    // Quote tmux_name to prevent shell metacharacter injection.
                    let escaped_name = tmux_name.replace('\'', "'\\''");
                    let cmd = format!("{} attach-session -t '={}'", tmux_bin, escaped_name);
                    let cwd = std::env::current_dir()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    (cmd, cwd)
                }
                #[cfg(windows)]
                {
                    let _ = tmux_name;
                    return Err("tmux not available on Windows".to_string());
                }
            }
            PtyTarget::Daemon { .. } => unreachable!(),
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let observer = self.observer.read().unwrap().clone();
        let backend = PlaneaiPtyBackend::spawn(
            session_id, &command, &cwd, env, app, on_data, cancelled, observer,
        )?;
        let mut sessions = self.sessions.write().map_err(|e| e.to_string())?;
        if let Some(old) = sessions.get(session_id) {
            old.detach();
        }
        sessions.insert(session_id.to_string(), Box::new(backend));
        Ok(())
    }

    /// Attach to a daemon-managed session via data connection.
    fn attach_daemon(
        &self,
        session_id: &str,
        socket_path: PathBuf,
        app: AppHandle,
        on_data: Channel<Response>,
    ) -> Result<(), String> {
        tracing::info!(session_id, "attaching to daemon session");
        let cancelled = Arc::new(AtomicBool::new(false));
        let flow = Arc::new(FlowControl::new());
        let sid = session_id.to_string();

        {
            let sessions = self.sessions.read().map_err(|e| e.to_string())?;
            if let Some(old) = sessions.get(session_id) {
                old.detach();
            }
        }

        let cancelled_clone = cancelled.clone();
        let flow_clone = flow.clone();
        let sid_clone = sid.clone();
        let observer = self.observer.read().unwrap().clone();
        let sessions_arc = self.sessions.clone();

        tauri::async_runtime::spawn(async move {
            let data_conn = match DataConnection::open(&socket_path, &sid_clone).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("daemon data connect failed for {}: {}", sid_clone, e);
                    let _ = app.emit("pty-exited", serde_json::json!({ "pty_key": sid_clone }));
                    return;
                }
            };

            let (reader, writer) = data_conn.into_split();
            let writer_arc = Arc::new(tokio::sync::Mutex::new(writer));

            let backend: Box<dyn SessionBackend> = Box::new(DaemonBackend {
                writer: writer_arc,
                cancelled: cancelled_clone.clone(),
                flow: flow_clone.clone(),
            });

            {
                let mut s = sessions_arc.write().unwrap();
                s.insert(sid_clone.clone(), backend);
            }

            // Coalescing buffer shared between async reader and sync flusher
            let pending: Arc<(Mutex<Vec<u8>>, Condvar)> =
                Arc::new((Mutex::new(Vec::with_capacity(READ_BUF)), Condvar::new()));
            let done = Arc::new(AtomicBool::new(false));

            // ── Flusher thread (coalesces output before sending to frontend) ──
            let pending_f = pending.clone();
            let done_f = done.clone();
            let cancelled_f = cancelled_clone.clone();
            let flow_f = flow_clone;
            let exit_key = sid_clone.clone();
            let app_flusher = app.clone();
            thread::spawn(move || {
                let (lock, cv) = &*pending_f;
                loop {
                    {
                        let mut g = lock.lock().unwrap();
                        while g.is_empty() {
                            if done_f.load(Ordering::Acquire) {
                                if !g.is_empty() {
                                    let chunk = std::mem::take(&mut *g);
                                    let _ = on_data.send(Response::new(chunk));
                                }
                                if !cancelled_f.load(Ordering::Acquire) {
                                    let _ = app_flusher.emit(
                                        "pty-exited",
                                        serde_json::json!({ "pty_key": exit_key }),
                                    );
                                }
                                return;
                            }
                            let (next, _) = cv.wait_timeout(g, FLUSH_MAX_IDLE).unwrap();
                            g = next;
                        }
                    }

                    // Wait for flow control (backpressure from frontend)
                    flow_f.wait_if_paused();

                    thread::sleep(FLUSH_COALESCE);

                    let chunk = std::mem::take(&mut *lock.lock().unwrap());
                    if chunk.is_empty() {
                        continue;
                    }
                    if on_data.send(Response::new(chunk)).is_err() {
                        break;
                    }
                }
                if !cancelled_f.load(Ordering::Acquire) {
                    let _ =
                        app_flusher.emit("pty-exited", serde_json::json!({ "pty_key": exit_key }));
                }
            });

            // ── Async reader: accumulates daemon frames into coalescing buffer ──
            let mut reader = reader;
            loop {
                if cancelled_clone.load(Ordering::Acquire) {
                    break;
                }
                match planeai_daemon::protocol::read_frame(&mut reader).await {
                    Ok((_frame_type, payload)) => {
                        observer.on_output(&sid_clone, payload.len());
                        let (lock, cv) = &*pending;
                        let mut g = lock.lock().unwrap();
                        // Cap buffer at 1MB to bound memory when paused
                        if g.len() + payload.len() > 1_048_576 {
                            let excess = g.len() + payload.len() - 1_048_576;
                            g.drain(..excess);
                        }
                        g.extend_from_slice(&payload);
                        cv.notify_one();
                    }
                    Err(_) => break,
                }
            }

            done.store(true, Ordering::Release);
            pending.1.notify_one();
        });

        Ok(())
    }

    /// Write input bytes to a session's PTY.
    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let sessions = self.sessions.read().map_err(|e| e.to_string())?;
        let backend = sessions.get(session_id).ok_or("session not attached")?;
        backend.write(data)
    }

    /// Resize a session's PTY.
    pub fn resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<(), String> {
        let sessions = self.sessions.read().map_err(|e| e.to_string())?;
        let backend = sessions.get(session_id).ok_or("session not attached")?;
        backend.resize(rows, cols)
    }

    /// Detach a session's PTY (cleanup).
    pub fn detach(&self, session_id: &str) {
        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        if let Some(backend) = sessions.remove(session_id) {
            backend.detach();
        }
    }

    /// Pause reading from a session's PTY (flow control back pressure).
    pub fn pause(&self, session_id: &str) -> Result<(), String> {
        let sessions = self.sessions.read().map_err(|e| e.to_string())?;
        let backend = sessions.get(session_id).ok_or("session not attached")?;
        backend.pause()
    }

    /// Resume reading from a session's PTY (flow control).
    pub fn resume(&self, session_id: &str) -> Result<(), String> {
        let sessions = self.sessions.read().map_err(|e| e.to_string())?;
        let backend = sessions.get(session_id).ok_or("session not attached")?;
        backend.resume()
    }
}
