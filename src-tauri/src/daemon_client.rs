//! Async client for the planeai-daemon process.
//!
//! Provides a control connection (JSON-line protocol) and data connections
//! (binary frames for PTY I/O).

use planeai_daemon::protocol::{
    read_frame, write_frame, CONN_CONTROL, CONN_DATA, FRAME_INPUT, FRAME_OUTPUT,
};
use planeai_ipc::r#async::AsyncIpcStream;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

/// High-level async client for the daemon's control connection.
#[allow(dead_code)]
pub struct DaemonClient {
    socket_path: PathBuf,
    writer: tokio::io::WriteHalf<AsyncIpcStream>,
    event_rx: mpsc::UnboundedReceiver<DaemonEvent>,
    response_rx: mpsc::UnboundedReceiver<serde_json::Value>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SessionInfo {
    pub session_id: String,
    pub alive: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DaemonEvent {
    pub event: String,
    pub session_id: String,
}

impl DaemonClient {
    /// Connect to the daemon socket. Sends the 0x00 control type byte.
    pub async fn connect(socket_path: &Path) -> Result<Self, String> {
        let stream = AsyncIpcStream::connect(socket_path)
            .await
            .map_err(|e| format!("failed to connect to daemon: {e}"))?;

        let (reader, mut writer) = tokio::io::split(stream);
        writer
            .write_all(&[CONN_CONTROL])
            .await
            .map_err(|e| format!("failed to send control byte: {e}"))?;

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (response_tx, response_rx) = mpsc::unbounded_channel();

        // Spawn reader task that routes incoming lines to events or responses
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let Ok(val) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                            continue;
                        };
                        if val.get("event").is_some() {
                            let evt = DaemonEvent {
                                event: val["event"].as_str().unwrap_or("").to_string(),
                                session_id: val["session_id"].as_str().unwrap_or("").to_string(),
                            };
                            let _ = event_tx.send(evt);
                        } else {
                            let _ = response_tx.send(val);
                        }
                    }
                }
            }
        });

        Ok(Self {
            socket_path: socket_path.to_path_buf(),
            writer,
            event_rx,
            response_rx,
        })
    }

    /// Spawn a new session in the daemon.
    pub async fn spawn_session(
        &mut self,
        session_id: &str,
        command: &str,
        args: &[String],
        cwd: &str,
        env: Option<&HashMap<String, String>>,
    ) -> Result<(), String> {
        let req = serde_json::json!({
            "cmd": "spawn",
            "session_id": session_id,
            "command": command,
            "args": args,
            "cwd": cwd,
            "env": env,
        });
        self.send_request(&req).await
    }

    /// Kill a session.
    #[allow(dead_code)]
    pub async fn kill_session(&mut self, session_id: &str) -> Result<(), String> {
        let req = serde_json::json!({ "cmd": "kill", "session_id": session_id });
        self.send_request(&req).await
    }

    /// Resize a session's PTY.
    pub async fn resize(&mut self, session_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let req = serde_json::json!({ "cmd": "resize", "session_id": session_id, "cols": cols, "rows": rows });
        self.send_request(&req).await
    }

    /// List all sessions.
    pub async fn list_sessions(&mut self) -> Result<Vec<SessionInfo>, String> {
        let req = serde_json::json!({ "cmd": "list" });
        let mut line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("write failed: {e}"))?;

        let resp = self.response_rx.recv().await.ok_or("connection closed")?;
        if let Some(err) = resp.get("error") {
            return Err(err.as_str().unwrap_or("unknown error").to_string());
        }
        let sessions = resp
            .get("sessions")
            .and_then(|s| serde_json::from_value::<Vec<SessionInfoRaw>>(s.clone()).ok())
            .unwrap_or_default();
        Ok(sessions
            .into_iter()
            .map(|s| SessionInfo {
                session_id: s.session_id,
                alive: s.alive,
            })
            .collect())
    }

    /// Receive the next event (async).
    #[allow(dead_code)]
    pub async fn recv_event(&mut self) -> Option<DaemonEvent> {
        self.event_rx.recv().await
    }

    /// Get the socket path this client is connected to.
    #[allow(dead_code)]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    async fn send_request(&mut self, req: &serde_json::Value) -> Result<(), String> {
        let mut line = serde_json::to_string(req).map_err(|e| e.to_string())?;
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("write failed: {e}"))?;

        let resp = self.response_rx.recv().await.ok_or("connection closed")?;
        if let Some(err) = resp.get("error") {
            Err(err.as_str().unwrap_or("unknown error").to_string())
        } else {
            Ok(())
        }
    }
}

#[derive(serde::Deserialize)]
struct SessionInfoRaw {
    session_id: String,
    alive: bool,
}

// ─── Data Connection ─────────────────────────────────────────────────────────

/// A data connection for streaming PTY I/O with a specific session.
pub struct DataConnection {
    reader: tokio::io::ReadHalf<AsyncIpcStream>,
    writer: tokio::io::WriteHalf<AsyncIpcStream>,
}

impl DataConnection {
    /// Open a data connection to the given session.
    /// Connects, sends 0x01 type byte, then sends session_id handshake frame.
    pub async fn open(socket_path: &Path, session_id: &str) -> Result<Self, String> {
        let stream = AsyncIpcStream::connect(socket_path)
            .await
            .map_err(|e| format!("failed to connect for data: {e}"))?;
        let (reader, mut writer) = tokio::io::split(stream);

        writer
            .write_all(&[CONN_DATA])
            .await
            .map_err(|e| format!("failed to send data byte: {e}"))?;

        write_frame(&mut writer, FRAME_OUTPUT, session_id.as_bytes())
            .await
            .map_err(|e| format!("handshake failed: {e}"))?;

        Ok(Self { reader, writer })
    }

    /// Read the next output frame from the daemon.
    #[allow(dead_code)]
    pub async fn read_output(&mut self) -> Result<Vec<u8>, String> {
        let (_frame_type, payload) = read_frame(&mut self.reader)
            .await
            .map_err(|e| format!("read failed: {e}"))?;
        Ok(payload)
    }

    /// Write input data to the daemon session.
    #[allow(dead_code)]
    pub async fn write_input(&mut self, data: &[u8]) -> Result<(), String> {
        write_frame(&mut self.writer, FRAME_INPUT, data)
            .await
            .map_err(|e| format!("write failed: {e}"))
    }

    /// Split into reader and writer halves for concurrent use.
    pub fn into_split(
        self,
    ) -> (
        tokio::io::ReadHalf<AsyncIpcStream>,
        tokio::io::WriteHalf<AsyncIpcStream>,
    ) {
        (self.reader, self.writer)
    }
}

// ─── Sidecar Spawning ────────────────────────────────────────────────────────

/// Resolve the daemon binary path. In production, use the bundled resource.
/// In development, use the workspace target directory.
pub fn resolve_daemon_binary(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager as _;

    let bin_name = if cfg!(windows) {
        "planeai-daemon.exe"
    } else {
        "planeai-daemon"
    };

    // Try resource dir (production bundle — externalBin places it here)
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled = resource_dir.join(bin_name);
        if bundled.exists() {
            return bundled;
        }
    }

    // Fallback: same directory as the main executable (workspace target/debug or target/release)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join(bin_name);
            if sibling.exists() {
                return sibling;
            }
        }
    }

    // Last resort: hope it's on PATH
    PathBuf::from(bin_name)
}

/// Ensure the daemon process is running. If not reachable, spawn it as a detached process.
pub async fn ensure_daemon_running(
    sidecar_path: &Path,
    socket_path: &Path,
    scrollback_bytes: usize,
) -> Result<(), String> {
    // Try connecting first
    if try_connect(socket_path).await {
        return Ok(());
    }

    // Ensure parent directory exists
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed to create socket dir: {e}"))?;
    }

    // Spawn detached daemon process
    spawn_detached(sidecar_path, socket_path, scrollback_bytes)?;

    // Retry with exponential backoff: 50, 100, 200, 400, 800ms (total ~1.5s)
    let delays = [50, 100, 200, 400, 800];
    for delay in delays {
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        if try_connect(socket_path).await {
            return Ok(());
        }
    }

    Err("daemon did not start within 2s".to_string())
}

async fn try_connect(socket_path: &Path) -> bool {
    AsyncIpcStream::connect(socket_path).await.is_ok()
}

fn spawn_detached(
    sidecar_path: &Path,
    socket_path: &Path,
    scrollback_bytes: usize,
) -> Result<(), String> {
    let mut cmd = std::process::Command::new(sidecar_path);
    cmd.arg("--socket-path")
        .arg(socket_path)
        .arg("--scrollback-bytes")
        .arg(scrollback_bytes.to_string());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const DETACHED_PROCESS: u32 = 0x00000008;
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }

    cmd.spawn()
        .map_err(|e| format!("failed to spawn daemon: {e}"))?;
    Ok(())
}
