//! Async client for the planeai-daemon process.
//!
//! Provides a control connection (JSON-line protocol) and data connections
//! (binary frames for PTY I/O).

use planeai_daemon::protocol::{
    write_frame, CONN_CONTROL, CONN_DATA, FRAME_ATTACH, FRAME_HELLO, PROTOCOL_VERSION,
};
use planeai_ipc::r#async::AsyncIpcStream;
use std::collections::HashMap;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use uuid::Uuid;

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
            "mode": "replace_exited",
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
    /// Connects, sends 0x01 type byte, then performs HELLO/ATTACH handshake.
    pub async fn open(socket_path: &Path, session_id: &str) -> Result<Self, String> {
        let stream = AsyncIpcStream::connect(socket_path)
            .await
            .map_err(|e| format!("failed to connect for data: {e}"))?;
        let (reader, mut writer) = tokio::io::split(stream);

        writer
            .write_all(&[CONN_DATA])
            .await
            .map_err(|e| format!("failed to send data byte: {e}"))?;

        write_frame(&mut writer, FRAME_HELLO, &[PROTOCOL_VERSION])
            .await
            .map_err(|e| format!("hello failed: {e}"))?;
        write_frame(&mut writer, FRAME_ATTACH, session_id.as_bytes())
            .await
            .map_err(|e| format!("attach failed: {e}"))?;

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

pub const CONTROL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Spawn a daemon-backed shell tab through a correlated request lifecycle.
///
/// A timeout is not reported until the daemon acknowledges `cancel_spawn`: a
/// queued spawn is suppressed, while one already created is killed by its exact
/// request ID. This prevents a delayed control response from leaving an orphan.
pub async fn spawn_shell_tab(
    socket_path: &Path,
    session_id: &str,
    command: &str,
    args: &[String],
    cwd: &str,
    env: &HashMap<String, String>,
) -> Result<(), String> {
    spawn_shell_tab_with_timeout(
        socket_path,
        session_id,
        command,
        args,
        cwd,
        env,
        CONTROL_TIMEOUT,
    )
    .await
}

async fn spawn_shell_tab_with_timeout(
    socket_path: &Path,
    session_id: &str,
    command: &str,
    args: &[String],
    cwd: &str,
    env: &HashMap<String, String>,
    control_timeout: std::time::Duration,
) -> Result<(), String> {
    let daemon = tokio::time::timeout(control_timeout, list_daemon_sessions(socket_path))
        .await
        .map_err(|_| "daemon shell-tab lookup timed out".to_string())??;
    if daemon.running.contains(session_id) {
        return Ok(());
    }

    if daemon.supports_correlated_shell_tab_spawn() {
        return spawn_shell_tab_correlated(
            socket_path,
            session_id,
            command,
            args,
            cwd,
            env,
            control_timeout,
        )
        .await;
    }

    // A daemon retained across an app update does not understand request_id or
    // cancel_spawn. Use its legacy spawn contract instead of sending either.
    spawn_shell_tab_legacy(
        socket_path,
        session_id,
        command,
        args,
        cwd,
        env,
        control_timeout,
    )
    .await
}

/// Kill a daemon-backed shell tab through a bounded asynchronous control request.
/// A missing session is already absent, so it is equivalent to a successful kill.
pub async fn kill_shell_tab(socket_path: &Path, session_id: &str) -> Result<(), String> {
    let request = serde_json::json!({
        "cmd": "kill",
        "session_id": session_id,
    });
    match tokio::time::timeout(CONTROL_TIMEOUT, send_control_request(socket_path, &request)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) if error.contains("session not found") => Ok(()),
        Ok(Err(error)) => Err(format!("daemon shell-tab kill failed: {error}")),
        Err(_) => Err("daemon shell-tab kill timed out".to_string()),
    }
}

async fn spawn_shell_tab_correlated(
    socket_path: &Path,
    session_id: &str,
    command: &str,
    args: &[String],
    cwd: &str,
    env: &HashMap<String, String>,
    control_timeout: std::time::Duration,
) -> Result<(), String> {
    let request_id = Uuid::new_v4().to_string();
    let request = serde_json::json!({
        "cmd": "spawn",
        "request_id": request_id,
        "session_id": session_id,
        "command": command,
        "args": args,
        "cwd": cwd,
        "env": env,
        "mode": "replace_exited",
    });

    let spawn_error =
        match tokio::time::timeout(control_timeout, send_control_request(socket_path, &request))
            .await
        {
            Ok(Ok(_)) => return Ok(()),
            Ok(Err(error)) => error,
            Err(_) => "daemon shell-tab spawn timed out".to_string(),
        };

    tracing::warn!(session_id, request_id, error = %spawn_error, "cancelling uncertain daemon shell-tab spawn");
    let cancel = serde_json::json!({
        "cmd": "cancel_spawn",
        "request_id": request_id,
    });
    tokio::time::timeout(control_timeout, send_control_request(socket_path, &cancel))
        .await
        .map_err(|_| "daemon shell-tab spawn cancellation was not acknowledged".to_string())??;
    Err(format!("{spawn_error}; daemon spawn was cancelled"))
}

/// Spawn with the pre-v3 daemon contract. Once the request line is written, a
/// lost response is ambiguous: the daemon may have spawned the tab. Returning
/// success keeps the UI handle alive, letting its terminal attach or its later
/// close clean up the exact session rather than leaving an orphan behind.
async fn spawn_shell_tab_legacy(
    socket_path: &Path,
    session_id: &str,
    command: &str,
    args: &[String],
    cwd: &str,
    env: &HashMap<String, String>,
    control_timeout: std::time::Duration,
) -> Result<(), String> {
    let stream = AsyncIpcStream::connect(socket_path)
        .await
        .map_err(|e| format!("failed to connect to daemon: {e}"))?;
    let (reader, mut writer) = tokio::io::split(stream);
    writer
        .write_all(&[CONN_CONTROL])
        .await
        .map_err(|e| format!("failed to send control byte: {e}"))?;
    let request = serde_json::json!({
        "cmd": "spawn",
        "session_id": session_id,
        "command": command,
        "args": args,
        "cwd": cwd,
        "env": env,
        "mode": "replace_exited",
    });
    let mut line = serde_json::to_string(&request).map_err(|e| e.to_string())?;
    line.push('\n');
    if let Err(error) = writer.write_all(line.as_bytes()).await {
        tracing::warn!(session_id, %error, "legacy daemon shell-tab spawn write was ambiguous; retaining tab handle");
        return Ok(());
    }

    let mut reader = BufReader::new(reader);
    line.clear();
    match tokio::time::timeout(control_timeout, reader.read_line(&mut line)).await {
        Ok(Ok(bytes)) if bytes > 0 => {
            let response: serde_json::Value = serde_json::from_str(line.trim())
                .map_err(|e| format!("invalid daemon response: {e}"))?;
            if let Some(error) = response.get("error").and_then(serde_json::Value::as_str) {
                return Err(error.to_string());
            }
            Ok(())
        }
        Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {
            tracing::warn!(
                session_id,
                "legacy daemon shell-tab spawn response was ambiguous; retaining tab handle"
            );
            Ok(())
        }
    }
}

struct DaemonSessionList {
    running: std::collections::HashSet<String>,
    protocol_version: Option<u8>,
}

impl DaemonSessionList {
    fn supports_correlated_shell_tab_spawn(&self) -> bool {
        self.protocol_version
            .is_some_and(|version| version >= PROTOCOL_VERSION)
    }
}

/// Fetch the running IDs and advertised daemon protocol. Pre-update daemons
/// omit protocol_version, which selects the legacy shell-tab spawn fallback.
async fn list_daemon_sessions(socket_path: &Path) -> Result<DaemonSessionList, String> {
    let response = send_control_request(socket_path, &serde_json::json!({ "cmd": "list" })).await?;
    let sessions = response
        .get("sessions")
        .and_then(serde_json::Value::as_array)
        .ok_or("invalid daemon list response")?;
    let protocol_version = response
        .get("protocol_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|version| u8::try_from(version).ok());
    Ok(DaemonSessionList {
        running: running_session_ids(sessions),
        protocol_version,
    })
}

fn running_session_ids(sessions: &[serde_json::Value]) -> std::collections::HashSet<String> {
    sessions
        .iter()
        .filter(|session| {
            session.get("status").and_then(serde_json::Value::as_str) == Some("running")
        })
        .filter_map(|session| session.get("session_id")?.as_str().map(str::to_owned))
        .collect()
}

async fn send_control_request(
    socket_path: &Path,
    request: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let stream = AsyncIpcStream::connect(socket_path)
        .await
        .map_err(|e| format!("failed to connect to daemon: {e}"))?;
    let (reader, mut writer) = tokio::io::split(stream);
    writer
        .write_all(&[CONN_CONTROL])
        .await
        .map_err(|e| format!("failed to send control byte: {e}"))?;
    let mut line = serde_json::to_string(request).map_err(|e| e.to_string())?;
    line.push('\n');
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(|e| format!("daemon control write failed: {e}"))?;

    let mut reader = BufReader::new(reader);
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("daemon control read failed: {e}"))?;
        if bytes == 0 {
            return Err("daemon control connection closed".to_string());
        }
        let response: serde_json::Value = serde_json::from_str(line.trim())
            .map_err(|e| format!("invalid daemon response: {e}"))?;
        if response.get("event").is_some() {
            continue;
        }
        if let Some(error) = response.get("error").and_then(serde_json::Value::as_str) {
            return Err(error.to_string());
        }
        return Ok(response);
    }
}

/// Sync query to daemon for startup reconciliation. Shell tabs use the async
/// cancellable control lifecycle above instead.
pub fn list_sessions_sync() -> Option<std::collections::HashSet<String>> {
    list_sessions_sync_inner()
}

fn list_sessions_sync_inner() -> Option<std::collections::HashSet<String>> {
    use std::io::{BufRead, Write};

    let app_dir = planeai_paths::app_data_dir();
    let mut stream = planeai_ipc::connect(planeai_ipc::Channel::Daemon, &app_dir).ok()?;
    stream.set_read_timeout(Some(CONTROL_TIMEOUT)).ok()?;
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

    // Ensure parent directory exists (Unix sockets need a directory; Windows named pipes don't)
    #[cfg(unix)]
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed to create socket dir: {e}"))?;
    }

    // Spawn detached daemon process
    spawn_detached(sidecar_path, socket_path, scrollback_bytes)?;

    // Retry with capped exponential backoff.
    // Windows needs a longer budget due to slower process creation.
    #[cfg(windows)]
    const DELAYS_MS: &[u64] = &[100, 200, 400, 800, 800, 800];
    #[cfg(not(windows))]
    const DELAYS_MS: &[u64] = &[50, 100, 200, 400, 800];

    for delay in DELAYS_MS {
        tokio::time::sleep(std::time::Duration::from_millis(*delay)).await;
        if try_connect(socket_path).await {
            tracing::info!("daemon started successfully");
            return Ok(());
        }
    }

    let budget_ms: u64 = DELAYS_MS.iter().sum();
    tracing::error!(budget_ms, "daemon did not start in time");
    Err(format!("daemon did not start within {budget_ms}ms"))
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
        // Safety: pre_exec runs in the forked child before exec.
        // Raise the FD soft limit so the daemon (even an old binary) doesn't hit 256.
        unsafe {
            cmd.pre_exec(|| {
                planeai_paths::raise_fd_limit();
                Ok(())
            });
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn timed_out_effective_spawn_is_acknowledged_cancelled_without_orphaning_a_shell() {
        use planeai_daemon::server::DaemonServer;
        use planeai_daemon::transport::DaemonListener;
        use std::sync::Arc;
        use tokio::io::AsyncReadExt;
        use tokio::net::UnixListener;
        use tokio::sync::{oneshot, Mutex};

        let dir = tempfile::tempdir().unwrap();
        let daemon_socket = dir.path().join("daemon.sock");
        let proxy_socket = dir.path().join("proxy.sock");
        let listener = DaemonListener::bind(&daemon_socket).unwrap();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());
        let daemon = Arc::new(DaemonServer::new(4096));
        let daemon_task = tokio::spawn(async move { daemon.run(listener, shutdown_rx).await });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let (spawn_effective_tx, spawn_effective_rx) = oneshot::channel();
        let spawn_effective_tx = Arc::new(Mutex::new(Some(spawn_effective_tx)));
        let proxy_listener = UnixListener::bind(&proxy_socket).unwrap();
        let proxy_daemon_socket = daemon_socket.clone();
        let proxy_task = tokio::spawn(async move {
            loop {
                let (stream, _) = proxy_listener.accept().await.unwrap();
                let daemon_socket = proxy_daemon_socket.clone();
                let spawn_effective_tx = Arc::clone(&spawn_effective_tx);
                tokio::spawn(async move {
                    let (reader, mut writer) = tokio::io::split(stream);
                    let mut reader = BufReader::new(reader);
                    let mut discriminator = [0u8; 1];
                    reader.read_exact(&mut discriminator).await.unwrap();
                    assert_eq!(discriminator, [CONN_CONTROL]);

                    let mut line = String::new();
                    reader.read_line(&mut line).await.unwrap();
                    let request: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
                    let response = send_control_request(&daemon_socket, &request)
                        .await
                        .unwrap();

                    if request["cmd"] == "spawn" {
                        // The real daemon has now completed the spawn, but this
                        // proxy deliberately withholds its response so the client
                        // must use the correlated cancellation path.
                        if let Some(tx) = spawn_effective_tx.lock().await.take() {
                            tx.send(()).unwrap();
                        }
                        std::future::pending::<()>().await;
                    }

                    let mut response = serde_json::to_string(&response).unwrap();
                    response.push('\n');
                    writer.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });

        send_control_request(
            &daemon_socket,
            &serde_json::json!({
                "cmd": "spawn",
                "session_id": "unrelated-shell",
                "command": "sleep",
                "args": ["999"],
                "mode": "create_only",
            }),
        )
        .await
        .unwrap();

        let proxy_for_client = proxy_socket.clone();
        let spawn = tokio::spawn(async move {
            spawn_shell_tab_with_timeout(
                &proxy_for_client,
                "timed-out-shell",
                "sleep",
                &["999".to_string()],
                "/",
                &HashMap::new(),
                std::time::Duration::from_millis(250),
            )
            .await
        });

        tokio::time::timeout(std::time::Duration::from_secs(1), spawn_effective_rx)
            .await
            .expect("daemon should spawn before the response is delayed")
            .expect("proxy should observe the effective spawn");
        let error = spawn.await.unwrap().unwrap_err();
        assert_eq!(
            error,
            "daemon shell-tab spawn timed out; daemon spawn was cancelled"
        );

        let sessions = send_control_request(&daemon_socket, &serde_json::json!({ "cmd": "list" }))
            .await
            .unwrap();
        let sessions = sessions["sessions"].as_array().unwrap();
        assert_eq!(
            running_session_ids(sessions),
            std::collections::HashSet::from(["unrelated-shell".to_string()]),
            "only the unrelated shell may remain running"
        );
        let cancelled = sessions
            .iter()
            .find(|session| session["session_id"] == "timed-out-shell")
            .expect("effective spawn must be retained as a killed session");
        assert_eq!(cancelled["status"], "killed");

        kill_shell_tab(&daemon_socket, "unrelated-shell")
            .await
            .unwrap();
        proxy_task.abort();
        shutdown_tx.send(()).unwrap();
        daemon_task.await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn legacy_daemon_fallback_omits_request_lifecycle_fields() {
        use std::sync::Arc;
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixListener;
        use tokio::sync::Mutex;

        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("legacy.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let requests = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let recorded_requests = Arc::clone(&requests);
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let (reader, mut writer) = tokio::io::split(stream);
                let mut reader = BufReader::new(reader);
                let mut discriminator = [0u8; 1];
                reader.read_exact(&mut discriminator).await.unwrap();
                assert_eq!(discriminator, [CONN_CONTROL]);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let request: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
                recorded_requests.lock().await.push(request.clone());
                let response = if request["cmd"] == "list" {
                    // The missing protocol_version emulates a pre-v3 daemon.
                    serde_json::json!({ "sessions": [] })
                } else {
                    serde_json::json!({ "ok": true, "session_id": "legacy:1" })
                };
                writer
                    .write_all(format!("{response}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });

        spawn_shell_tab_with_timeout(
            &socket,
            "legacy:1",
            "sleep",
            &["999".to_string()],
            "/",
            &HashMap::new(),
            std::time::Duration::from_secs(1),
        )
        .await
        .unwrap();
        server.await.unwrap();

        let requests = requests.lock().await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["cmd"], "list");
        assert_eq!(requests[1]["cmd"], "spawn");
        assert!(requests[1].get("request_id").is_none());
        assert_ne!(requests[1]["cmd"], "cancel_spawn");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn legacy_daemon_timeout_retains_the_shell_tab_handle_without_cancellation() {
        use std::sync::Arc;
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixListener;
        use tokio::sync::Mutex;

        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("legacy-timeout.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let requests = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let recorded_requests = Arc::clone(&requests);
        let server = tokio::spawn(async move {
            for request_number in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let (reader, mut writer) = tokio::io::split(stream);
                let mut reader = BufReader::new(reader);
                let mut discriminator = [0u8; 1];
                reader.read_exact(&mut discriminator).await.unwrap();
                assert_eq!(discriminator, [CONN_CONTROL]);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let request: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
                recorded_requests.lock().await.push(request);
                if request_number == 0 {
                    writer.write_all(b"{\"sessions\":[]}\n").await.unwrap();
                } else {
                    // The spawn may already be effective, but this pre-update
                    // daemon never replies. The client must not cancel it.
                    std::future::pending::<()>().await;
                }
            }
        });

        let result = spawn_shell_tab_with_timeout(
            &socket,
            "legacy:timeout",
            "sleep",
            &["999".to_string()],
            "/",
            &HashMap::new(),
            std::time::Duration::from_millis(50),
        )
        .await;
        assert!(
            result.is_ok(),
            "ambiguous legacy spawn should retain its UI handle: {result:?}"
        );

        let requests = requests.lock().await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["cmd"], "spawn");
        assert!(requests[1].get("request_id").is_none());
        drop(requests);
        server.abort();
    }

    #[test]
    fn exited_and_killed_shell_tabs_are_not_treated_as_running() {
        let sessions = vec![
            serde_json::json!({ "session_id": "tab:1", "status": "running" }),
            serde_json::json!({ "session_id": "tab:2", "status": "exited" }),
            serde_json::json!({ "session_id": "tab:3", "status": "killed" }),
        ];

        assert_eq!(
            running_session_ids(&sessions),
            std::collections::HashSet::from(["tab:1".to_string()])
        );
    }
}
