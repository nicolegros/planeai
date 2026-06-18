//! Daemon-backed session source for the Iced spike.
//!
//! Bridges the async daemon protocol (control + data connections) into the
//! pull-based PlaneAiTerminalSession trait via a dedicated tokio runtime thread
//! and bounded buffer (same pattern as planeai-local).
//!
//! Async strategy: one shared tokio runtime spawned on first daemon session,
//! each session gets its own async tasks for data I/O.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Instant;

use planeai_daemon::protocol::{
    read_frame, write_frame, CONN_CONTROL, CONN_DATA, FRAME_INPUT, FRAME_OUTPUT,
};
use planeai_ipc::r#async::AsyncIpcStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use crate::adapter::{PipelineDiag, PlaneAiTerminalSession};

const MAX_BUFFER: usize = 512 * 1024; // 512KB, matches planeai-local

/// Shared tokio runtime for all daemon sessions.
static DAEMON_RT: OnceLock<Runtime> = OnceLock::new();

fn daemon_runtime() -> &'static Runtime {
    DAEMON_RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("daemon-io")
            .build()
            .expect("failed to create daemon tokio runtime")
    })
}

fn daemon_socket_path() -> PathBuf {
    planeai_ipc::daemon_socket_path()
}

fn daemon_binary_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().unwrap_or(std::path::Path::new("."));
    let candidate = dir.join("planeai-daemon");
    if candidate.exists() {
        return candidate;
    }
    PathBuf::from("planeai-daemon")
}

/// Ensure the daemon is running (blocking call from sync context).
pub fn ensure_daemon_running_sync() -> anyhow::Result<()> {
    let rt = daemon_runtime();
    rt.block_on(async {
        let socket = daemon_socket_path();
        if AsyncIpcStream::connect(&socket).await.is_ok() {
            return Ok(());
        }
        let binary = daemon_binary_path();
        if let Some(parent) = socket.parent() {
            std::fs::create_dir_all(parent)?;
        }
        spawn_detached_daemon(&binary, &socket)?;
        for delay in [50, 100, 200, 400, 800] {
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            if AsyncIpcStream::connect(&socket).await.is_ok() {
                return Ok(());
            }
        }
        anyhow::bail!("daemon did not start within 2s")
    })
}

fn spawn_detached_daemon(binary: &std::path::Path, socket: &std::path::Path) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new(binary);
    cmd.arg("--socket-path")
        .arg(socket)
        .arg("--scrollback-bytes")
        .arg("1048576");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
    }
    cmd.spawn()?;
    Ok(())
}

// ─── Lifecycle functions ─────────────────────────────────────────────────────

/// Info about a daemon session from the List command.
#[derive(Debug, Clone)]
pub struct DaemonSessionInfo {
    pub session_id: String,
    pub alive: bool,
}

/// List existing daemon sessions.
pub fn list_daemon_sessions() -> anyhow::Result<Vec<DaemonSessionInfo>> {
    let rt = daemon_runtime();
    let socket = daemon_socket_path();
    rt.block_on(async {
        let mut stream = AsyncIpcStream::connect(&socket).await?;
        stream.write_all(&[CONN_CONTROL]).await?;
        let req = serde_json::json!({ "cmd": "list" });
        let mut line = serde_json::to_string(&req)?;
        line.push('\n');
        stream.write_all(line.as_bytes()).await?;
        let (reader, _) = tokio::io::split(stream);
        let mut buf_reader = BufReader::new(reader);
        let mut resp_line = String::new();
        buf_reader.read_line(&mut resp_line).await?;
        let resp: serde_json::Value = serde_json::from_str(resp_line.trim())?;
        if let Some(err) = resp.get("error") {
            anyhow::bail!("daemon list error: {}", err);
        }
        let sessions = resp
            .get("sessions")
            .and_then(|s| s.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(DaemonSessionInfo {
                            session_id: v.get("session_id")?.as_str()?.to_string(),
                            alive: v.get("alive")?.as_bool().unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(sessions)
    })
}

/// Attach to an existing daemon session (skip spawn, just open data connection).
pub fn attach(id: usize, session_id: &str, cols: u16, rows: u16) -> anyhow::Result<DaemonSession> {
    ensure_daemon_running_sync()?;
    let socket = daemon_socket_path();
    let rt = daemon_runtime();

    // Send attach command
    rt.block_on(async {
        send_control_command(
            &socket,
            &serde_json::json!({
                "cmd": "attach",
                "session_id": session_id,
            }),
        )
        .await
    })?;

    // Resize to current terminal size
    rt.block_on(async {
        send_control_command(
            &socket,
            &serde_json::json!({
                "cmd": "resize",
                "session_id": session_id,
                "cols": cols,
                "rows": rows,
            }),
        )
        .await
    })?;

    // Open data connection (same as spawn path)
    let buf = Arc::new(Mutex::new(Vec::new()));
    let buf_not_full = Arc::new(Condvar::new());
    let exited = Arc::new(AtomicBool::new(false));
    let max_pending = Arc::new(AtomicU64::new(0));
    let recv_bytes = Arc::new(AtomicU64::new(0));
    let send_calls = Arc::new(AtomicU64::new(0));
    let send_bytes_counter = Arc::new(AtomicU64::new(0));
    let block_count = Arc::new(AtomicU64::new(0));
    let block_ns = Arc::new(AtomicU64::new(0));

    let (input_tx, input_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (resize_tx, resize_rx) = mpsc::unbounded_channel::<(u16, u16)>();

    let attach_start = Instant::now();
    let buf_clone = buf.clone();
    let buf_not_full_clone = buf_not_full.clone();
    let exited_clone = exited.clone();
    let max_pending_clone = max_pending.clone();
    let recv_bytes_clone = recv_bytes.clone();
    let block_count_clone = block_count.clone();
    let block_ns_clone = block_ns.clone();
    let sid = session_id.to_string();
    let socket_clone = socket.clone();

    rt.spawn(async move {
        if let Err(e) = data_loop(
            &socket_clone,
            &sid,
            input_rx,
            buf_clone,
            buf_not_full_clone,
            exited_clone,
            max_pending_clone,
            recv_bytes_clone,
            block_count_clone,
            block_ns_clone,
        )
        .await
        {
            tracing::debug!("daemon data loop ended: {e}");
        }
    });

    let sid_resize = session_id.to_string();
    let socket_resize = socket.clone();
    rt.spawn(async move {
        resize_loop(&socket_resize, &sid_resize, resize_rx).await;
    });

    let attach_latency_ms = attach_start.elapsed().as_secs_f64() * 1000.0;

    Ok(DaemonSession {
        id,
        session_id: session_id.to_string(),
        buf,
        buf_not_full,
        exited,
        max_pending,
        input_tx,
        resize_tx,
        recv_bytes,
        send_calls,
        send_bytes: send_bytes_counter,
        block_count,
        block_ns,
        spawn_latency_ms: 0.0,
        attach_latency_ms,
    })
}

/// Send kill command to daemon for a session.
pub fn kill_daemon_session(session_id: &str) -> anyhow::Result<()> {
    let rt = daemon_runtime();
    let socket = daemon_socket_path();
    rt.block_on(async {
        send_control_command(
            &socket,
            &serde_json::json!({
                "cmd": "kill",
                "session_id": session_id,
            }),
        )
        .await
    })
}

/// Send detach command (informational).
pub fn detach_daemon_session(session_id: &str) -> anyhow::Result<()> {
    let rt = daemon_runtime();
    let socket = daemon_socket_path();
    rt.block_on(async {
        send_control_command(
            &socket,
            &serde_json::json!({
                "cmd": "detach",
                "session_id": session_id,
            }),
        )
        .await
    })
}

/// Check if daemon is reachable (quick connect attempt).
pub fn daemon_is_connected() -> bool {
    let rt = daemon_runtime();
    let socket = daemon_socket_path();
    rt.block_on(async { AsyncIpcStream::connect(&socket).await.is_ok() })
}

// ─── DaemonSession struct ────────────────────────────────────────────────────

/// Daemon-backed terminal session.
pub struct DaemonSession {
    id: usize,
    session_id: String,
    buf: Arc<Mutex<Vec<u8>>>,
    buf_not_full: Arc<Condvar>,
    exited: Arc<AtomicBool>,
    max_pending: Arc<AtomicU64>,
    input_tx: mpsc::UnboundedSender<Vec<u8>>,
    resize_tx: mpsc::UnboundedSender<(u16, u16)>,
    recv_bytes: Arc<AtomicU64>,
    send_calls: Arc<AtomicU64>,
    send_bytes: Arc<AtomicU64>,
    block_count: Arc<AtomicU64>,
    block_ns: Arc<AtomicU64>,
    spawn_latency_ms: f64,
    attach_latency_ms: f64,
}

impl DaemonSession {
    /// Spawn a new session via the daemon and attach to it.
    pub fn spawn(id: usize, cols: u16, rows: u16, command: Option<&str>) -> anyhow::Result<Self> {
        ensure_daemon_running_sync()?;

        let session_id = format!("iced-{}-{}", id, std::process::id());
        let cmd = command.unwrap_or("bash");

        let launch_req = planeai_core::session_launch::CreateSessionRequest {
            session_id: session_id.clone(),
            project_cwd: std::env::current_dir().unwrap_or_default(),
            session_target: planeai_core::session_launch::SessionTarget::Daemon,
            agent_command: cmd.to_string(),
            env: std::collections::HashMap::new(),
            extra_path_dirs: vec![],
            cols,
            rows,
            durable_logs: std::env::var("PLANEAI_SESSION_LOG_DIR").is_ok(),
        };
        let launch_result = planeai_core::session_launch::prepare_session(&launch_req)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        tracing::info!(
            caller = "iced",
            shared_launch_service = true,
            target = "daemon",
            cwd = %launch_result.cwd.display(),
            command_label = %launch_result.command_label,
            durable_logs = launch_req.durable_logs,
            extra_path_dirs_count = launch_req.extra_path_dirs.len(),
            "session created via shared launch service"
        );

        let socket = daemon_socket_path();
        let rt = daemon_runtime();

        // Spawn session via control connection
        let spawn_start = Instant::now();
        rt.block_on(async {
            let mut stream = AsyncIpcStream::connect(&socket).await?;
            stream.write_all(&[CONN_CONTROL]).await?;
            let req = serde_json::json!({
                "cmd": "spawn",
                "session_id": &launch_result.session_id,
                "command": &launch_result.program,
                "args": &launch_result.args,
                "cwd": launch_result.cwd.to_string_lossy(),
                "env": &launch_result.env,
            });
            let mut line = serde_json::to_string(&req)?;
            line.push('\n');
            stream.write_all(line.as_bytes()).await?;
            let (mut reader, _) = tokio::io::split(stream);
            let mut buf_reader = BufReader::new(&mut reader);
            let mut resp_line = String::new();
            buf_reader.read_line(&mut resp_line).await?;
            let resp: serde_json::Value = serde_json::from_str(resp_line.trim())?;
            if let Some(err) = resp.get("error") {
                anyhow::bail!("daemon spawn error: {}", err);
            }
            Ok::<(), anyhow::Error>(())
        })?;
        let spawn_latency_ms = spawn_start.elapsed().as_secs_f64() * 1000.0;

        // Resize immediately
        rt.block_on(async {
            send_control_command(
                &socket,
                &serde_json::json!({
                    "cmd": "resize",
                    "session_id": &session_id,
                    "cols": cols,
                    "rows": rows,
                }),
            )
            .await
        })?;

        // Open data connection
        let buf = Arc::new(Mutex::new(Vec::new()));
        let buf_not_full = Arc::new(Condvar::new());
        let exited = Arc::new(AtomicBool::new(false));
        let max_pending = Arc::new(AtomicU64::new(0));
        let recv_bytes = Arc::new(AtomicU64::new(0));
        let send_calls = Arc::new(AtomicU64::new(0));
        let send_bytes_counter = Arc::new(AtomicU64::new(0));
        let block_count = Arc::new(AtomicU64::new(0));
        let block_ns = Arc::new(AtomicU64::new(0));

        let (input_tx, input_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (resize_tx, resize_rx) = mpsc::unbounded_channel::<(u16, u16)>();

        let attach_start = Instant::now();
        let buf_clone = buf.clone();
        let buf_not_full_clone = buf_not_full.clone();
        let exited_clone = exited.clone();
        let max_pending_clone = max_pending.clone();
        let recv_bytes_clone = recv_bytes.clone();
        let block_count_clone = block_count.clone();
        let block_ns_clone = block_ns.clone();
        let sid = session_id.clone();
        let socket_clone = socket.clone();

        rt.spawn(async move {
            if let Err(e) = data_loop(
                &socket_clone,
                &sid,
                input_rx,
                buf_clone,
                buf_not_full_clone,
                exited_clone,
                max_pending_clone,
                recv_bytes_clone,
                block_count_clone,
                block_ns_clone,
            )
            .await
            {
                tracing::debug!("daemon data loop ended: {e}");
            }
        });

        let sid_resize = session_id.clone();
        let socket_resize = socket.clone();
        rt.spawn(async move {
            resize_loop(&socket_resize, &sid_resize, resize_rx).await;
        });

        let attach_latency_ms = attach_start.elapsed().as_secs_f64() * 1000.0;

        Ok(Self {
            id,
            session_id,
            buf,
            buf_not_full,
            exited,
            max_pending,
            input_tx,
            resize_tx,
            recv_bytes,
            send_calls,
            send_bytes: send_bytes_counter,
            block_count,
            block_ns,
            spawn_latency_ms,
            attach_latency_ms,
        })
    }

    /// Spawn a new session via the daemon with an explicit working directory.
    pub fn spawn_with_cwd(
        id: usize,
        cols: u16,
        rows: u16,
        command: Option<&str>,
        cwd: &std::path::Path,
        extra_path_dirs: &[String],
    ) -> anyhow::Result<Self> {
        ensure_daemon_running_sync()?;

        let session_id = format!("iced-{}-{}", id, std::process::id());
        let cmd = command.unwrap_or("bash");

        let launch_req = planeai_core::session_launch::CreateSessionRequest {
            session_id: session_id.clone(),
            project_cwd: cwd.to_path_buf(),
            session_target: planeai_core::session_launch::SessionTarget::Daemon,
            agent_command: cmd.to_string(),
            env: std::collections::HashMap::new(),
            extra_path_dirs: extra_path_dirs.to_vec(),
            cols,
            rows,
            durable_logs: std::env::var("PLANEAI_SESSION_LOG_DIR").is_ok(),
        };
        let launch_result = planeai_core::session_launch::prepare_session(&launch_req)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        tracing::info!(
            caller = "iced",
            shared_launch_service = true,
            target = "daemon",
            cwd = %launch_result.cwd.display(),
            command_label = %launch_result.command_label,
            durable_logs = launch_req.durable_logs,
            extra_path_dirs_count = launch_req.extra_path_dirs.len(),
            "session created via shared launch service"
        );

        let socket = daemon_socket_path();
        let rt = daemon_runtime();

        let spawn_start = Instant::now();
        rt.block_on(async {
            let mut stream = AsyncIpcStream::connect(&socket).await?;
            stream.write_all(&[CONN_CONTROL]).await?;
            let req = serde_json::json!({
                "cmd": "spawn",
                "session_id": &launch_result.session_id,
                "command": &launch_result.program,
                "args": &launch_result.args,
                "cwd": launch_result.cwd.to_string_lossy(),
                "env": &launch_result.env,
            });
            let mut line = serde_json::to_string(&req)?;
            line.push('\n');
            stream.write_all(line.as_bytes()).await?;
            let (mut reader, _) = tokio::io::split(stream);
            let mut buf_reader = BufReader::new(&mut reader);
            let mut resp_line = String::new();
            buf_reader.read_line(&mut resp_line).await?;
            let resp: serde_json::Value = serde_json::from_str(resp_line.trim())?;
            if let Some(err) = resp.get("error") {
                anyhow::bail!("daemon spawn error: {}", err);
            }
            Ok::<(), anyhow::Error>(())
        })?;
        let spawn_latency_ms = spawn_start.elapsed().as_secs_f64() * 1000.0;

        rt.block_on(async {
            send_control_command(
                &socket,
                &serde_json::json!({
                    "cmd": "resize",
                    "session_id": &session_id,
                    "cols": cols,
                    "rows": rows,
                }),
            )
            .await
        })?;

        let buf = Arc::new(Mutex::new(Vec::new()));
        let buf_not_full = Arc::new(Condvar::new());
        let exited = Arc::new(AtomicBool::new(false));
        let max_pending = Arc::new(AtomicU64::new(0));
        let recv_bytes = Arc::new(AtomicU64::new(0));
        let send_calls = Arc::new(AtomicU64::new(0));
        let send_bytes_counter = Arc::new(AtomicU64::new(0));
        let block_count = Arc::new(AtomicU64::new(0));
        let block_ns = Arc::new(AtomicU64::new(0));

        let (input_tx, input_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (resize_tx, resize_rx) = mpsc::unbounded_channel::<(u16, u16)>();

        let attach_start = Instant::now();
        let buf_clone = buf.clone();
        let buf_not_full_clone = buf_not_full.clone();
        let exited_clone = exited.clone();
        let max_pending_clone = max_pending.clone();
        let recv_bytes_clone = recv_bytes.clone();
        let block_count_clone = block_count.clone();
        let block_ns_clone = block_ns.clone();
        let sid = session_id.clone();
        let socket_clone = socket.clone();

        rt.spawn(async move {
            if let Err(e) = data_loop(
                &socket_clone,
                &sid,
                input_rx,
                buf_clone,
                buf_not_full_clone,
                exited_clone,
                max_pending_clone,
                recv_bytes_clone,
                block_count_clone,
                block_ns_clone,
            )
            .await
            {
                tracing::debug!("daemon data loop ended: {e}");
            }
        });

        let sid_resize = session_id.clone();
        let socket_resize = socket.clone();
        rt.spawn(async move {
            resize_loop(&socket_resize, &sid_resize, resize_rx).await;
        });

        let attach_latency_ms = attach_start.elapsed().as_secs_f64() * 1000.0;

        Ok(Self {
            id,
            session_id,
            buf,
            buf_not_full,
            exited,
            max_pending,
            input_tx,
            resize_tx,
            recv_bytes,
            send_calls,
            send_bytes: send_bytes_counter,
            block_count,
            block_ns,
            spawn_latency_ms,
            attach_latency_ms,
        })
    }

    pub fn spawn_latency_ms(&self) -> f64 {
        self.spawn_latency_ms
    }
    pub fn attach_latency_ms(&self) -> f64 {
        self.attach_latency_ms
    }
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

impl PlaneAiTerminalSession for DaemonSession {
    fn id(&self) -> usize {
        self.id
    }

    fn write(&self, bytes: &[u8]) -> anyhow::Result<()> {
        self.send_calls.fetch_add(1, Ordering::Relaxed);
        self.send_bytes
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        self.input_tx
            .send(bytes.to_vec())
            .map_err(|_| anyhow::anyhow!("input channel closed"))
    }

    fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.resize_tx
            .send((cols, rows))
            .map_err(|_| anyhow::anyhow!("resize channel closed"))
    }

    fn try_read_batch(&self) -> anyhow::Result<Option<Vec<u8>>> {
        let mut buf = self.buf.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        if buf.is_empty() {
            return Ok(None);
        }
        let data = std::mem::take(&mut *buf);
        self.buf_not_full.notify_one();
        Ok(Some(data))
    }

    fn has_exited(&self) -> bool {
        self.exited.load(Ordering::Acquire)
    }

    fn pending_bytes(&self) -> usize {
        self.buf.lock().unwrap().len()
    }

    fn max_pending_bytes(&self) -> usize {
        self.max_pending.load(Ordering::Relaxed) as usize
    }

    fn bytes_dropped(&self) -> u64 {
        0
    }

    fn pipeline_diag(&self) -> PipelineDiag {
        PipelineDiag {
            pty_reader_bytes_total: self.recv_bytes.load(Ordering::Relaxed),
            pty_reader_reads_total: 0,
            flusher_batches_total: 0,
            flusher_bytes_total: self.recv_bytes.load(Ordering::Relaxed),
            flusher_wakeups_total: 0,
            flusher_sleep_ms_total: 0.0,
            sink_send_calls_total: self.send_calls.load(Ordering::Relaxed),
            sink_send_bytes_total: self.send_bytes.load(Ordering::Relaxed),
            output_queue_capacity_bytes: MAX_BUFFER,
            max_pending_pty_output_bytes: self.max_pending.load(Ordering::Relaxed),
            queue_depth_at_end_bytes: self.buf.lock().unwrap().len(),
            output_bytes_dropped: 0,
            producer_block_count: self.block_count.load(Ordering::Relaxed),
            producer_block_duration_ms: self.block_ns.load(Ordering::Relaxed) as f64 / 1_000_000.0,
        }
    }
}

// ─── Async internals ─────────────────────────────────────────────────────────

/// Async data loop: reads output frames from daemon, writes to bounded buffer.
#[allow(clippy::too_many_arguments)]
async fn data_loop(
    socket: &std::path::Path,
    session_id: &str,
    mut input_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    buf: Arc<Mutex<Vec<u8>>>,
    buf_not_full: Arc<Condvar>,
    exited: Arc<AtomicBool>,
    max_pending: Arc<AtomicU64>,
    recv_bytes: Arc<AtomicU64>,
    block_count: Arc<AtomicU64>,
    block_ns: Arc<AtomicU64>,
) -> anyhow::Result<()> {
    let mut stream = AsyncIpcStream::connect(socket).await?;
    stream.write_all(&[CONN_DATA]).await?;

    // Handshake: send session_id
    write_frame(&mut stream, FRAME_OUTPUT, session_id.as_bytes()).await?;

    let (mut reader, mut writer) = tokio::io::split(stream);

    let input_task = tokio::spawn(async move {
        while let Some(data) = input_rx.recv().await {
            if write_frame(&mut writer, FRAME_INPUT, &data).await.is_err() {
                break;
            }
        }
    });

    loop {
        let result = read_frame(&mut reader).await;
        match result {
            Ok((FRAME_OUTPUT, payload)) => {
                recv_bytes.fetch_add(payload.len() as u64, Ordering::Relaxed);
                let mut b = buf.lock().unwrap();
                if !b.is_empty() && b.len() + payload.len() > MAX_BUFFER {
                    block_count.fetch_add(1, Ordering::Relaxed);
                    let t = Instant::now();
                    while !b.is_empty() && b.len() + payload.len() > MAX_BUFFER {
                        b = buf_not_full.wait(b).unwrap();
                    }
                    block_ns.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                }
                b.extend_from_slice(&payload);
                let len = b.len() as u64;
                max_pending.fetch_max(len, Ordering::Relaxed);
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }

    exited.store(true, Ordering::Release);
    input_task.abort();
    Ok(())
}

/// Async resize loop.
async fn resize_loop(
    socket: &std::path::Path,
    session_id: &str,
    mut resize_rx: mpsc::UnboundedReceiver<(u16, u16)>,
) {
    while let Some((cols, rows)) = resize_rx.recv().await {
        let _ = send_control_command(
            socket,
            &serde_json::json!({
                "cmd": "resize",
                "session_id": session_id,
                "cols": cols,
                "rows": rows,
            }),
        )
        .await;
    }
}

async fn send_control_command(
    socket: &std::path::Path,
    req: &serde_json::Value,
) -> anyhow::Result<()> {
    let mut stream = AsyncIpcStream::connect(socket).await?;
    stream.write_all(&[CONN_CONTROL]).await?;
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    stream.write_all(line.as_bytes()).await?;
    let (reader, _) = tokio::io::split(stream);
    let mut buf_reader = BufReader::new(reader);
    let mut resp = String::new();
    buf_reader.read_line(&mut resp).await?;
    Ok(())
}
