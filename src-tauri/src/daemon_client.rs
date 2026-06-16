//! Async client for the planeai-daemon process.
//!
//! Provides a control connection (JSON-line protocol) and data connections
//! (binary frames for PTY I/O).

use planeai_daemon::protocol::{write_frame, CONN_CONTROL, CONN_DATA, FRAME_OUTPUT};
use planeai_ipc::r#async::AsyncIpcStream;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

/// High-level async client for the daemon's control connection.
pub struct DaemonClient {
    writer: tokio::io::WriteHalf<AsyncIpcStream>,
    event_rx: mpsc::UnboundedReceiver<DaemonEvent>,
    response_rx: mpsc::UnboundedReceiver<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct DaemonEvent {
    pub event: String,
    pub session_id: String,
}

impl DaemonClient {
    /// Connect to the daemon socket. Sends the 0x00 control type byte.
    pub async fn connect(socket_path: &Path) -> Result<Self, String> {
        tracing::debug!(path = %socket_path.display(), "connecting to daemon control socket");
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
        tracing::info!(session_id, command, "spawning session in daemon");
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

    /// Receive the next event (async).
    pub async fn recv_event(&mut self) -> Option<DaemonEvent> {
        self.event_rx.recv().await
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

// ─── Sync IPC ────────────────────────────────────────────────────────────────

/// Sync query to daemon for session list (avoids async/block_on during startup).
pub fn list_sessions_sync() -> Option<std::collections::HashSet<String>> {
    use std::io::{BufRead, Write};

    let app_dir = crate::paths::app_data_dir();
    let mut stream = planeai_ipc::connect(planeai_ipc::Channel::Daemon, &app_dir).ok()?;
    stream.write_all(&[0x00]).ok()?; // control connection type byte
    let req = serde_json::json!({"cmd": "list"});
    stream.write_all(format!("{}\n", req).as_bytes()).ok()?;

    let mut line = String::new();
    let mut reader = std::io::BufReader::new(stream);
    reader.read_line(&mut line).ok()?;

    let val: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let sessions = val.get("sessions")?.as_array()?;
    let ids: std::collections::HashSet<String> = sessions
        .iter()
        .filter_map(|s| s.get("session_id")?.as_str().map(|s| s.to_string()))
        .collect();
    Some(ids)
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
    tracing::warn!("daemon binary not found alongside executable, falling back to PATH");
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
        tracing::debug!("daemon already running");
        return Ok(());
    }

    tracing::info!(
        binary = %sidecar_path.display(),
        socket = %socket_path.display(),
        "spawning daemon process"
    );

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
            tracing::info!("daemon started successfully");
            return Ok(());
        }
    }

    tracing::error!("daemon did not start within 2s");
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
