use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;
use std::time::Duration;
use tauri::ipc::{Channel, Response};
use tauri::{AppHandle, Emitter};

use crate::notify;
#[cfg(not(windows))]
use crate::tmux;

/// Describes what command to run inside the PTY.
#[allow(dead_code)]
pub enum PtyTarget {
    /// Attach to an existing tmux session.
    TmuxAttach { tmux_name: String },
    /// Spawn a command directly (no tmux).
    Direct {
        command: String,
        args: Vec<String>,
        cwd: String,
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

struct PtyHandle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    flow: Arc<FlowControl>,
    /// Set to true when this PTY is being replaced by a new attach (not a natural exit).
    /// Flusher threads check this before emitting pty-exited.
    cancelled: Arc<AtomicBool>,
}

impl Drop for PtyHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

pub struct PtyManager {
    ptys: RwLock<HashMap<String, Arc<Mutex<PtyHandle>>>>,
    notify_state: Mutex<Option<notify::SharedNotifyState>>,
    socket_path: Mutex<Option<String>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            ptys: RwLock::new(HashMap::new()),
            notify_state: Mutex::new(None),
            socket_path: Mutex::new(None),
        }
    }

    pub fn set_notify_state(&self, state: notify::SharedNotifyState) {
        *self.notify_state.lock().unwrap() = Some(state);
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
            PtyTarget::Direct { command, args, cwd } => {
                let mut c = CommandBuilder::new(command);
                c.args(args);
                c.cwd(cwd);
                c
            }
        };
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORFGBG", if dark_mode { "15;0" } else { "0;15" });
        cmd.env("PLANEAI_SESSION_ID", session_id);
        if let Some(sock) = self.socket_path.lock().unwrap().as_deref() {
            cmd.env("PLANEAI_SOCKET", sock);
        }

        // Ensure UTF-8 locale for proper Nerd Font / Unicode rendering
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

        let handle = Arc::new(Mutex::new(PtyHandle {
            master: pair.master,
            writer,
            child,
            flow: Arc::new(FlowControl::new()),
            cancelled: cancelled.clone(),
        }));

        let flow_clone = handle.lock().unwrap().flow.clone();

        {
            let mut ptys = self.ptys.write().map_err(|e| e.to_string())?;
            // If replacing an existing PTY, mark it cancelled so its flusher
            // won't emit pty-exited (this is a re-attach, not a real exit).
            if let Some(old) = ptys.get(session_id) {
                if let Ok(h) = old.lock() {
                    h.cancelled.store(true, Ordering::Release);
                }
            }
            ptys.insert(session_id.to_string(), handle);
        }

        // Shared buffer between reader and flusher threads
        let pending: Arc<(Mutex<Vec<u8>>, Condvar)> =
            Arc::new((Mutex::new(Vec::with_capacity(READ_BUF)), Condvar::new()));
        let done = Arc::new(AtomicBool::new(false));

        // ── Reader thread: reads PTY → pushes into pending buffer ─────────
        let pending_r = pending.clone();
        let exit_event_name = format!("pty-exited-{session_id}");
        let notify_clone = self.notify_state.lock().unwrap().clone();
        let sid = session_id.to_string();
        let done_r = done.clone();
        let app_reader = app.clone();
        thread::spawn(move || {
            let mut buf = [0u8; READ_BUF];
            let mut was_busy = false;
            loop {
                flow_clone.wait_if_paused();
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let (lock, cv) = &*pending_r;
                        let mut g = lock.lock().unwrap();
                        g.extend_from_slice(&buf[..n]);
                        cv.notify_one();

                        if let Some(ref ns) = notify_clone {
                            let mut s = ns.lock().unwrap();
                            let was_idle =
                                s.get_state(&sid) != Some(crate::notify::AgentState::Busy);
                            s.notify_output(&sid);
                            if was_idle && !was_busy {
                                drop(s);
                                let _ = app_reader.emit(
                                    "agent-state-change",
                                    serde_json::json!({
                                        "session_id": &sid,
                                        "state": "Busy"
                                    }),
                                );
                            }
                            was_busy = true;
                        }
                    }
                    Err(_) => break,
                }
            }
            // Signal flusher to exit, flush remaining
            done_r.store(true, Ordering::Release);
            pending_r.1.notify_one();
        });

        // ── Flusher thread: coalesces pending data → sends via Channel ────
        let pending_f = pending.clone();
        let done_f = done.clone();
        let exit_event = exit_event_name.clone();
        let app_flusher = app.clone();
        let cancelled_f = cancelled.clone();
        thread::spawn(move || {
            let (lock, cv) = &*pending_f;
            loop {
                // Wait for data or done signal
                {
                    let mut g = lock.lock().unwrap();
                    while g.is_empty() {
                        if done_f.load(Ordering::Acquire) {
                            // Final flush
                            if !g.is_empty() {
                                let chunk = std::mem::take(&mut *g);
                                let _ = on_data.send(Response::new(chunk));
                            }
                            if !cancelled_f.load(Ordering::Acquire) {
                                let _ = app_flusher.emit(&exit_event, ());
                            }
                            return;
                        }
                        let (next, _) = cv.wait_timeout(g, FLUSH_MAX_IDLE).unwrap();
                        g = next;
                    }
                }

                // Coalesce: wait a short window so burst data arrives together
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
                let _ = app_flusher.emit(&exit_event, ());
            }
        });

        Ok(())
    }

    /// Write input bytes to a session's PTY.
    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let ptys = self.ptys.read().map_err(|e| e.to_string())?;
        let handle = ptys.get(session_id).ok_or("session not attached")?;
        let mut h = handle.lock().map_err(|e| e.to_string())?;
        h.writer
            .write_all(data)
            .map_err(|e| format!("write failed: {e}"))
    }

    /// Resize a session's PTY.
    pub fn resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<(), String> {
        let ptys = self.ptys.read().map_err(|e| e.to_string())?;
        let handle = ptys.get(session_id).ok_or("session not attached")?;
        let h = handle.lock().map_err(|e| e.to_string())?;
        h.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("resize failed: {e}"))
    }

    /// Detach a session's PTY (cleanup).
    #[allow(dead_code)]
    pub fn detach(&self, session_id: &str) {
        let mut ptys = self.ptys.write().unwrap_or_else(|e| e.into_inner());
        ptys.remove(session_id);
    }

    /// Pause reading from a session's PTY (flow control back pressure).
    pub fn pause(&self, session_id: &str) -> Result<(), String> {
        let ptys = self.ptys.read().map_err(|e| e.to_string())?;
        let handle = ptys.get(session_id).ok_or("session not attached")?;
        let h = handle.lock().map_err(|e| e.to_string())?;
        h.flow.pause();
        Ok(())
    }

    /// Resume reading from a session's PTY (flow control).
    pub fn resume(&self, session_id: &str) -> Result<(), String> {
        let ptys = self.ptys.read().map_err(|e| e.to_string())?;
        let handle = ptys.get(session_id).ok_or("session not attached")?;
        let h = handle.lock().map_err(|e| e.to_string())?;
        h.flow.resume();
        Ok(())
    }
}
