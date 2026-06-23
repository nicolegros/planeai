use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use tauri::ipc::{Channel, Response};
use tauri::AppHandle;

use crate::output_observer::{NoopObserver, OutputObserver};
use crate::pty_planeai_core_adapter::PlaneaiPtyBackend;
use crate::session_backend::SessionBackend;
#[cfg(not(windows))]
use crate::tmux;

/// Describes what command to run inside the PTY.
pub enum PtyTarget {
    /// Attach to an existing tmux session.
    TmuxAttach { tmux_name: String },
    /// Spawn a command in a local PTY (shell tabs and agent sessions).
    Shell {
        command: String,
        args: Vec<String>,
        cwd: String,
    },
}

// ─── PtyManager ──────────────────────────────────────────────────────────────

pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, Box<dyn SessionBackend>>>>,
    observer: RwLock<Arc<dyn OutputObserver>>,
    socket_path: std::sync::Mutex<Option<String>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            observer: RwLock::new(Arc::new(NoopObserver)),
            socket_path: std::sync::Mutex::new(None),
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
        let (command, args, cwd) = match target {
            PtyTarget::Shell { command, args, cwd } => (command, args, cwd),
            PtyTarget::TmuxAttach { tmux_name } => {
                #[cfg(not(windows))]
                {
                    let tmux_bin = tmux::tmux_bin().to_string();
                    let target_arg = format!("={}", tmux_name);
                    (
                        tmux_bin,
                        vec!["attach-session".to_string(), "-t".to_string(), target_arg],
                        std::env::current_dir()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                    )
                }
                #[cfg(windows)]
                {
                    let _ = tmux_name;
                    return Err("tmux not available on Windows".to_string());
                }
            }
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let observer = self.observer.read().unwrap().clone();
        let socket_path = self.socket_path.lock().unwrap().clone();
        let backend = PlaneaiPtyBackend::spawn(
            session_id,
            &command,
            &args,
            &cwd,
            dark_mode,
            app,
            on_data,
            cancelled,
            observer,
            socket_path.as_deref(),
        )?;
        let mut sessions = self.sessions.write().map_err(|e| e.to_string())?;
        if let Some(old) = sessions.get(session_id) {
            old.detach();
        }
        sessions.insert(session_id.to_string(), Box::new(backend));
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
