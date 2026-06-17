use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;
use std::time::Duration;
use tauri::ipc::{Channel, Response};
use tauri::{AppHandle, Emitter};

use crate::daemon_client::DataConnection;
use crate::output_observer::{NoopObserver, OutputObserver};
use crate::session_backend::SessionBackend;
#[cfg(not(windows))]
use crate::tmux;

/// Describes what command to run inside the PTY.
pub enum PtyTarget {
    /// Attach to an existing tmux session.
    TmuxAttach { tmux_name: String },
    /// Spawn a local shell command (used for extra tabs, not for session backends).
    Shell {
        command: String,
        args: Vec<String>,
        cwd: String,
    },
    /// Attach to a daemon-managed session via data connection.
    Daemon {
        session_id: String,
        socket_path: PathBuf,
    },
}

/// Shared pause/resume state for a PTY reader thread.
struct FlowControl {
    paused: Mutex<bool>,
    cond: Condvar,
}

impl FlowControl {
    fn new() -> Self {
        Self {
            paused: Mutex::new(false),
            cond: Condvar::new(),
        }
    }

    fn pause(&self) {
        *self.paused.lock().unwrap() = true;
    }

    fn resume(&self) {
        let mut paused = self.paused.lock().unwrap();
        *paused = false;
        self.cond.notify_one();
    }

    fn wait_if_paused(&self) {
        let mut paused = self.paused.lock().unwrap();
        while *paused {
            paused = self.cond.wait(paused).unwrap();
        }
    }
}

// Flusher coalesces output so bursts arrive as single chunks.
const FLUSH_COALESCE: Duration = Duration::from_millis(4);
const FLUSH_MAX_IDLE: Duration = Duration::from_millis(50);
const READ_BUF: usize = 16 * 1024;

// ─── Local PTY Backend ───────────────────────────────────────────────────────

struct PtyHandle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    flow: Arc<FlowControl>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for PtyHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

struct LocalBackend {
    handle: Mutex<PtyHandle>,
}

impl SessionBackend for LocalBackend {
    fn write(&self, data: &[u8]) -> Result<(), String> {
        let mut h = self.handle.lock().map_err(|e| e.to_string())?;
        h.writer
            .write_all(data)
            .map_err(|e| format!("write failed: {e}"))
    }

    fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        let h = self.handle.lock().map_err(|e| e.to_string())?;
        h.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("resize failed: {e}"))
    }

    fn pause(&self) -> Result<(), String> {
        let h = self.handle.lock().map_err(|e| e.to_string())?;
        h.flow.pause();
        Ok(())
    }

    fn resume(&self) -> Result<(), String> {
        let h = self.handle.lock().map_err(|e| e.to_string())?;
        h.flow.resume();
        Ok(())
    }

    fn detach(&self) {
        if let Ok(h) = self.handle.lock() {
            h.cancelled.store(true, Ordering::Release);
        }
    }
}

// ─── Daemon Backend ──────────────────────────────────────────────────────────

struct DaemonBackend {
    writer: Arc<tokio::sync::Mutex<tokio::io::WriteHalf<planeai_ipc::r#async::AsyncIpcStream>>>,
    cancelled: Arc<AtomicBool>,
    flow: Arc<FlowControl>,
    session_id: String,
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
        let sid = self.session_id.clone();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let app_dir = crate::paths::app_data_dir();
            let mut stream = match planeai_ipc::connect(planeai_ipc::Channel::Daemon, &app_dir) {
                Ok(s) => s,
                Err(_) => return,
            };
            stream.write_all(&[0x00]).ok();
            let req =
                serde_json::json!({"cmd": "resize", "session_id": sid, "cols": cols, "rows": rows});
            let _ = stream.write_all(format!("{}\n", req).as_bytes());
            let mut buf = [0u8; 256];
            let _ = stream.read(&mut buf);
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
    socket_path: Mutex<Option<String>>,
    capture_file: Option<Arc<Mutex<std::fs::File>>>,
    capture_session: Option<String>,
}

impl PtyManager {
    pub fn new() -> Self {
        let (capture_file, capture_session) = match std::env::var("PLANEAI_BENCH_CAPTURE") {
            Ok(path) => {
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .expect("failed to open PLANEAI_BENCH_CAPTURE file");
                tracing::info!("bench capture writing to: {}", path);
                let session_filter = std::env::var("PLANEAI_BENCH_CAPTURE_SESSION").ok();
                (Some(Arc::new(Mutex::new(file))), session_filter)
            }
            Err(_) => (None, None),
        };
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            observer: RwLock::new(Arc::new(NoopObserver)),
            socket_path: Mutex::new(None),
            capture_file,
            capture_session,
        }
    }

    pub fn set_observer(&self, observer: Arc<dyn OutputObserver>) {
        *self.observer.write().unwrap() = observer;
    }

    pub fn set_socket_path(&self, path: String) {
        *self.socket_path.lock().unwrap() = Some(path);
    }

    /// Attach a PTY to a session. The command run inside depends on the PtyTarget variant.
    pub fn attach(
        &self,
        session_id: &str,
        target: PtyTarget,
        dark_mode: bool,
        app: AppHandle,
        on_data: Channel<Response>,
    ) -> Result<(), String> {
        // Handle daemon target via async path
        if let PtyTarget::Daemon {
            session_id: sid,
            socket_path,
        } = target
        {
            return self.attach_daemon(&sid, socket_path, app, on_data);
        }

        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("failed to open pty: {e}"))?;

        let mut cmd = match &target {
            PtyTarget::TmuxAttach { tmux_name } => {
                #[cfg(not(windows))]
                {
                    let target = format!("={}", tmux_name);
                    let mut c = CommandBuilder::new(tmux::tmux_bin());
                    c.args(["attach-session", "-t", &target]);
                    c
                }
                #[cfg(windows)]
                {
                    let _ = tmux_name;
                    return Err("tmux not available on Windows".to_string());
                }
            }
            PtyTarget::Shell { command, args, cwd } => {
                let mut c = CommandBuilder::new(command);
                c.args(args);
                c.cwd(cwd);
                c
            }
            PtyTarget::Daemon { .. } => unreachable!(),
        };
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORFGBG", if dark_mode { "15;0" } else { "0;15" });
        cmd.env("PLANEAI_SESSION_ID", session_id);
        if let Some(sock) = self.socket_path.lock().unwrap().as_deref() {
            cmd.env("PLANEAI_SOCKET", sock);
        }

        #[cfg(unix)]
        {
            let has_utf8 = |v: &str| {
                let upper = v.to_ascii_uppercase();
                upper.contains("UTF-8") || upper.contains("UTF8")
            };
            if !has_utf8(&std::env::var("LANG").unwrap_or_default()) {
                cmd.env("LANG", "en_US.UTF-8");
            }
            if !has_utf8(&std::env::var("LC_CTYPE").unwrap_or_default()) {
                cmd.env("LC_CTYPE", "en_US.UTF-8");
            }
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("failed to spawn: {e}"))?;
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("failed to get writer: {e}"))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("failed to get reader: {e}"))?;

        let cancelled = Arc::new(AtomicBool::new(false));

        let pty_handle = PtyHandle {
            master: pair.master,
            writer,
            child,
            flow: Arc::new(FlowControl::new()),
            cancelled: cancelled.clone(),
        };

        let flow_clone = pty_handle.flow.clone();

        let backend: Box<dyn SessionBackend> = Box::new(LocalBackend {
            handle: Mutex::new(pty_handle),
        });

        {
            let mut sessions = self.sessions.write().map_err(|e| e.to_string())?;
            if let Some(old) = sessions.get(session_id) {
                old.detach();
            }
            sessions.insert(session_id.to_string(), backend);
        }

        // Shared buffer between reader and flusher threads
        let pending: Arc<(Mutex<Vec<u8>>, Condvar)> =
            Arc::new((Mutex::new(Vec::with_capacity(READ_BUF)), Condvar::new()));
        let done = Arc::new(AtomicBool::new(false));

        // ── Reader thread ─────────────────────────────────────────────────
        let pending_r = pending.clone();
        let observer = self.observer.read().unwrap().clone();
        let sid = session_id.to_string();
        let done_r = done.clone();
        thread::spawn(move || {
            let mut buf = [0u8; READ_BUF];
            loop {
                flow_clone.wait_if_paused();
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let (lock, cv) = &*pending_r;
                        let mut g = lock.lock().unwrap();
                        g.extend_from_slice(&buf[..n]);
                        cv.notify_one();
                        observer.on_output(&sid, n);
                    }
                    Err(_) => break,
                }
            }
            done_r.store(true, Ordering::Release);
            pending_r.1.notify_one();
        });

        // ── Flusher thread ────────────────────────────────────────────────
        let pending_f = pending.clone();
        let done_f = done;
        let exit_key = session_id.to_string();
        let app_flusher = app.clone();
        let cancelled_f = cancelled;
        let capture_f = match (&self.capture_file, &self.capture_session) {
            (Some(f), None) => Some(f.clone()),
            (Some(f), Some(s)) if s == session_id => Some(f.clone()),
            _ => None,
        };
        thread::spawn(move || {
            let (lock, cv) = &*pending_f;
            loop {
                {
                    let mut g = lock.lock().unwrap();
                    while g.is_empty() {
                        if done_f.load(Ordering::Acquire) {
                            if !g.is_empty() {
                                let chunk = std::mem::take(&mut *g);
                                if let Some(ref cf) = capture_f {
                                    let _ = cf.lock().unwrap().write_all(&chunk);
                                }
                                let _ = on_data.send(Response::new(chunk));
                            }
                            if !cancelled_f.load(Ordering::Acquire) {
                                let _ = app_flusher
                                    .emit("pty-exited", serde_json::json!({ "pty_key": exit_key }));
                            }
                            return;
                        }
                        let (next, _) = cv.wait_timeout(g, FLUSH_MAX_IDLE).unwrap();
                        g = next;
                    }
                }

                thread::sleep(FLUSH_COALESCE);

                let chunk = std::mem::take(&mut *lock.lock().unwrap());
                if chunk.is_empty() {
                    continue;
                }
                if let Some(ref cf) = capture_f {
                    let _ = cf.lock().unwrap().write_all(&chunk);
                }
                if on_data.send(Response::new(chunk)).is_err() {
                    break;
                }
            }
            if !cancelled_f.load(Ordering::Acquire) {
                let _ = app_flusher.emit("pty-exited", serde_json::json!({ "pty_key": exit_key }));
            }
        });

        Ok(())
    }

    /// Attach to a daemon-managed session via data connection.
    /// Uses reader+flusher threads with coalescing (same pattern as local backend).
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
        let capture_d = match (&self.capture_file, &self.capture_session) {
            (Some(f), None) => Some(f.clone()),
            (Some(f), Some(s)) if s == session_id => Some(f.clone()),
            _ => None,
        };

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
                session_id: sid_clone.clone(),
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
                                    if let Some(ref cf) = capture_d {
                                        let _ = cf.lock().unwrap().write_all(&chunk);
                                    }
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
                    if let Some(ref cf) = capture_d {
                        let _ = cf.lock().unwrap().write_all(&chunk);
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
