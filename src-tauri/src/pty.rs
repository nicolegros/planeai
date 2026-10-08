use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;
use std::time::Duration;
use tauri::ipc::{Channel, Response};

use crate::daemon_client::DataConnection;
use crate::output_observer::{NoopObserver, OutputObserver};
use crate::pty_planeai_core_adapter::{PlaneaiPtyBackend, SpawnCommand};
use crate::session_backend::{SessionBackend, WriteAck};
use planeai_pty::FlowControl;

/// Describes what command to run inside the PTY.
#[derive(Debug)]
pub enum PtyTarget {
    /// Attach to an existing tmux session.
    TmuxAttach { tmux_name: String },
    /// Spawn a command string in a local PTY (shell tabs and agent sessions).
    /// The command is wrapped in the platform shell (`bash -c` on Unix, `cmd /C` on Windows).
    Shell { command: String, cwd: String },
    /// Run a program with its arguments in a local PTY, without a shell to parse them.
    Program { argv: Vec<String>, cwd: String },
    /// Attach to a daemon-managed session via data connection.
    Daemon {
        session_id: String,
        socket_path: PathBuf,
    },
    /// Attach to a resource hosted by PlaneAI's private rmux daemon.
    ///
    /// The workspace and pane are resolved by the caller from the `pty_key`
    /// mapping, so the PTY layer never has to reach into the database.
    Rmux {
        pty_key: String,
        workspace: planeai_rmux::WorkspaceName,
        handle: planeai_rmux::ResourceHandle,
    },
}

pub fn gone_error(pty_key: &str) -> String {
    format!("{pty_key} no longer runs")
}

/// Told the pty key of every PTY whose process ended by itself; a detach silences it.
pub type ExitSink = Arc<dyn Fn(&str) + Send + Sync>;

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
    fn write(&self, data: &[u8]) -> Result<WriteAck, String> {
        let writer = self.writer.clone();
        let data = data.to_vec();
        let (acknowledge, receiver) = tokio::sync::oneshot::channel();
        tauri::async_runtime::spawn(async move {
            let result = async {
                let mut w = writer.lock().await;
                planeai_daemon::protocol::write_frame(
                    &mut *w,
                    planeai_daemon::protocol::FRAME_INPUT,
                    &data,
                )
                .await
                .map_err(|e| e.to_string())
            }
            .await;
            let _ = acknowledge.send(result);
        });
        Ok(WriteAck::Pending(receiver))
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

/// The bytes a daemon data frame puts in the terminal, if any.
fn daemon_frame_output(session_id: &str, frame_type: u8, payload: Vec<u8>) -> Option<Vec<u8>> {
    use planeai_daemon::protocol::{FRAME_ERROR, FRAME_GAP, FRAME_OUTPUT};
    match frame_type {
        FRAME_OUTPUT if !payload.is_empty() => Some(payload),
        FRAME_GAP => {
            let lagged = serde_json::from_slice::<serde_json::Value>(&payload)
                .ok()
                .and_then(|gap| gap["lagged"].as_u64());
            tracing::warn!(session_id, ?lagged, "daemon output gap");
            let dropped = lagged.map_or("Output".to_string(), |n| format!("{n} output event(s)"));
            // Shown in the terminal, like an rmux gap, so the scrollback never jumps silently.
            Some(
                format!(
                    "\r\n\x1b[2m[planeai] output gap: {dropped} dropped by the PlaneAI daemon - \
                     this session fell behind\x1b[0m\r\n"
                )
                .into_bytes(),
            )
        }
        FRAME_ERROR => {
            tracing::error!(session_id, error = %String::from_utf8_lossy(&payload), "daemon data error");
            None
        }
        _ => None,
    }
}

// ─── Rmux Backend ────────────────────────────────────────────────────────────

/// Writes and resizes for an rmux-hosted session.
///
/// Resize addresses the pane's **window**: probe 2 in ADR-0012 showed
/// `Pane::resize` is a no-op for a sole pane in a detached session, so resizing
/// the pane alone would leave the child's tty at its original size.
///
/// `pause`/`resume` deliberately do **not** stop reading the rmux stream. rmux
/// applies no backpressure to the child, so a paused transport loses output
/// (probe 7). The reader keeps draining into a bounded buffer and flow control
/// gates delivery to the frontend instead.
struct RmuxBackend {
    pane: Arc<rmux_sdk::Pane>,
    window: Arc<rmux_sdk::Window>,
    cancelled: Arc<AtomicBool>,
    flow: Arc<FlowControl>,
}

impl SessionBackend for RmuxBackend {
    fn write(&self, data: &[u8]) -> Result<WriteAck, String> {
        // `send_text` is the literal `send-keys -l` path, so bytes reach the tty
        // unmodified. It takes a str, so non-UTF-8 input is rejected rather than
        // silently mangled.
        let text = match std::str::from_utf8(data) {
            Ok(text) => text.to_string(),
            Err(error) => return Err(format!("rmux input must be UTF-8: {error}")),
        };
        let pane = self.pane.clone();
        let (acknowledge, receiver) = tokio::sync::oneshot::channel();
        tauri::async_runtime::spawn(async move {
            let result = pane.send_text(&text).await.map_err(|e| e.to_string());
            let _ = acknowledge.send(result);
        });
        Ok(WriteAck::Pending(receiver))
    }

    fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        let window = self.window.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = window.resize(Some(cols), Some(rows)).await {
                tracing::debug!(%error, "rmux window resize failed");
            }
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
        // Release the flusher so it can observe cancellation and exit.
        self.flow.resume();
    }
}

// ─── PtyManager ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, Box<dyn SessionBackend>>>>,
    observer: Arc<RwLock<Arc<dyn OutputObserver>>>,
    exits: ExitSink,
}

impl PtyManager {
    pub fn new(exits: ExitSink) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            observer: Arc::new(RwLock::new(Arc::new(NoopObserver))),
            exits,
        }
    }

    pub fn set_observer(&self, observer: Arc<dyn OutputObserver>) {
        *self.observer.write().unwrap() = observer;
    }

    /// Report a process that ended outside any attached PTY, such as a daemon session's.
    pub fn report_exit(&self, pty_key: &str) {
        (self.exits)(pty_key);
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
        on_data: Channel<Response>,
        env: Vec<(String, String)>,
    ) -> Result<(), String> {
        // Handle daemon target via async path
        if let PtyTarget::Daemon {
            session_id: sid,
            socket_path,
        } = target
        {
            return self.attach_daemon(&sid, socket_path, on_data);
        }

        // Handle rmux target via async path
        if let PtyTarget::Rmux {
            pty_key,
            workspace,
            handle,
        } = target
        {
            return self.attach_rmux(&pty_key, workspace, handle, on_data);
        }

        // An agent's terminal remounts when its split leaf changes: its running local PTY gets
        // the new output channel. Terminal tabs reconnect through `reconnect` instead.
        if matches!(&target, PtyTarget::Shell { .. })
            && self.reconnect(session_id, on_data.clone()).is_ok()
        {
            return Ok(());
        }

        let (command, cwd) = match target {
            PtyTarget::Shell { command, cwd } => (SpawnCommand::Shell(command), cwd),
            PtyTarget::Program { argv, cwd } => (SpawnCommand::Argv(argv), cwd),
            PtyTarget::TmuxAttach { tmux_name } => {
                #[cfg(not(windows))]
                {
                    let tmux_bin = crate::command::tmux_bin().to_string();
                    // Quote tmux_name to prevent shell metacharacter injection.
                    let escaped_name = tmux_name.replace('\'', "'\\''");
                    let cmd = format!("{} attach-session -t '={}'", tmux_bin, escaped_name);
                    let cwd = std::env::current_dir()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    (SpawnCommand::Shell(cmd), cwd)
                }
                #[cfg(windows)]
                {
                    let _ = tmux_name;
                    return Err("tmux not available on Windows".to_string());
                }
            }
            PtyTarget::Daemon { .. } => unreachable!(),
            PtyTarget::Rmux { .. } => unreachable!(),
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let observer = self.observer.read().unwrap().clone();
        let backend = PlaneaiPtyBackend::spawn(
            session_id,
            command,
            &cwd,
            env,
            self.exits.clone(),
            on_data,
            cancelled,
            observer,
        )?;
        self.publish(session_id, Box::new(backend))
    }

    /// Make a spawned local backend the session's, replacing any previous one.
    fn publish(&self, session_id: &str, backend: Box<dyn SessionBackend>) -> Result<(), String> {
        let mut sessions = self.sessions.write().map_err(|e| e.to_string())?;
        if let Some(old) = sessions.insert(session_id.to_string(), backend) {
            old.detach();
        }
        Ok(())
    }

    /// Attach to a daemon-managed session via data connection.
    fn attach_daemon(
        &self,
        session_id: &str,
        socket_path: PathBuf,
        on_data: Channel<Response>,
    ) -> Result<(), String> {
        tracing::info!(session_id, "attaching to daemon session");
        let exits = self.exits.clone();
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
                    exits(&sid_clone);
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
            let exits_flusher = exits.clone();
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
                                    exits_flusher(&exit_key);
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
                    exits_flusher(&exit_key);
                }
            });

            // ── Async reader: accumulates daemon frames into coalescing buffer ──
            let mut reader = reader;
            loop {
                if cancelled_clone.load(Ordering::Acquire) {
                    break;
                }
                match planeai_daemon::protocol::read_frame(&mut reader).await {
                    Ok((frame_type, payload)) => {
                        let Some(payload) = daemon_frame_output(&sid_clone, frame_type, payload)
                        else {
                            continue;
                        };
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

    /// Attach to an rmux-hosted session.
    ///
    /// The reader loop never stops consuming the rmux stream, even while the
    /// frontend is paused: rmux drops output for a slow subscriber rather than
    /// throttling the child (ADR-0012, probe 7). Backpressure is applied to
    /// delivery instead, via the same flusher/`FlowControl` arrangement the
    /// daemon backend uses.
    fn attach_rmux(
        &self,
        pty_key: &str,
        workspace: planeai_rmux::WorkspaceName,
        handle: planeai_rmux::ResourceHandle,
        on_data: Channel<Response>,
    ) -> Result<(), String> {
        tracing::info!(pty_key, %workspace, "attaching to rmux resource");
        let exits = self.exits.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flow = Arc::new(FlowControl::new());
        let sid = pty_key.to_string();

        {
            let sessions = self.sessions.read().map_err(|e| e.to_string())?;
            if let Some(old) = sessions.get(pty_key) {
                old.detach();
            }
        }

        let cancelled_clone = cancelled.clone();
        let flow_clone = flow.clone();
        let observer = self.observer.read().unwrap().clone();
        let sessions_arc = self.sessions.clone();

        tauri::async_runtime::spawn(async move {
            // Retries once if the cached connection died while idle, which it can:
            // the daemon exits with its last session and closes idle clients.
            let attached = match crate::rmux_client::with_retry(|client| {
                let workspace = workspace.clone();
                async move { client.attach_resource(&workspace, handle).await }
            })
            .await
            {
                Ok(attached) => attached,
                Err(error) => {
                    tracing::error!(pty_key = %sid, %error, "rmux attach failed");
                    exits(&sid);
                    return;
                }
            };
            let (pane, window, mut stream) = attached.into_parts();

            let backend: Box<dyn SessionBackend> = Box::new(RmuxBackend {
                pane: Arc::new(pane),
                window: Arc::new(window),
                cancelled: cancelled_clone.clone(),
                flow: flow_clone.clone(),
            });
            {
                let mut sessions = sessions_arc.write().unwrap();
                sessions.insert(sid.clone(), backend);
            }

            // Bounded buffer shared between the async reader and the sync flusher.
            let pending = Arc::new((
                Mutex::new(planeai_rmux::OutputBuffer::with_default_capacity()),
                Condvar::new(),
            ));
            let done = Arc::new(AtomicBool::new(false));

            // ── Flusher thread: coalesces and honours frontend backpressure ──
            let pending_f = pending.clone();
            let done_f = done.clone();
            let cancelled_f = cancelled_clone.clone();
            let flow_f = flow_clone;
            let exit_key = sid.clone();
            let exits_flusher = exits.clone();
            thread::spawn(move || {
                let (lock, cv) = &*pending_f;
                loop {
                    {
                        let mut buffer = lock.lock().unwrap();
                        while buffer.is_empty() {
                            if done_f.load(Ordering::Acquire) {
                                // A gap recorded just before the stream closed is
                                // still owed to the terminal: exiting without it
                                // would make the scrollback silently jump.
                                let (chunk, gap) = (buffer.take(), buffer.take_gap());
                                if let Some(gap) = gap {
                                    tracing::warn!(
                                        session_id = %exit_key,
                                        cause = ?gap.cause,
                                        "rmux output gap"
                                    );
                                    let _ = on_data.send(Response::new(gap.notice()));
                                }
                                if !chunk.is_empty() {
                                    let _ = on_data.send(Response::new(chunk));
                                }
                                if !cancelled_f.load(Ordering::Acquire) {
                                    exits_flusher(&exit_key);
                                }
                                return;
                            }
                            let (next, _) = cv.wait_timeout(buffer, FLUSH_MAX_IDLE).unwrap();
                            buffer = next;
                        }
                    }

                    // Frontend backpressure gates delivery only; the reader above
                    // keeps draining rmux so the daemon never drops our output.
                    flow_f.wait_if_paused();

                    thread::sleep(FLUSH_COALESCE);

                    let (chunk, gap) = {
                        let mut buffer = lock.lock().unwrap();
                        (buffer.take(), buffer.take_gap())
                    };
                    if let Some(gap) = gap {
                        tracing::warn!(
                            session_id = %exit_key,
                            cause = ?gap.cause,
                            "rmux output gap"
                        );
                        // Sent as its own message so the common path never copies
                        // the chunk to prepend a notice to it.
                        if on_data.send(Response::new(gap.notice())).is_err() {
                            break;
                        }
                    }
                    if chunk.is_empty() {
                        continue;
                    }
                    if on_data.send(Response::new(chunk)).is_err() {
                        break;
                    }
                }
                if !cancelled_f.load(Ordering::Acquire) {
                    exits_flusher(&exit_key);
                }
            });

            // ── Async reader: drains rmux continuously, never pausing ──
            loop {
                if cancelled_clone.load(Ordering::Acquire) {
                    break;
                }
                match stream.next().await {
                    Ok(Some(rmux_sdk::PaneRecoveryEvent::Rebase(rebase))) => {
                        // A rebase is the authoritative screen. Its keyframe already
                        // carries the reset sequences, so feeding it rebuilds the
                        // terminal; what must not happen is stitching it onto bytes
                        // from the previous epoch, so anything still buffered and
                        // undelivered is discarded first.
                        let first = rebase.reason == rmux_sdk::PaneRecoveryRebaseReason::Initial;
                        if !first {
                            tracing::info!(
                                session_id = %sid,
                                reason = ?rebase.reason,
                                epoch = rebase.epoch,
                                "rmux pane rebased; replacing the rendered screen"
                            );
                        }
                        observer.on_output(&sid, rebase.keyframe.len());
                        let (lock, cv) = &*pending;
                        {
                            let mut buffer = lock.lock().unwrap();
                            if !first {
                                // Only a lag rebase means output was lost; the rest
                                // are faithful re-syncs and need no gap notice.
                                if rebase.reason == rmux_sdk::PaneRecoveryRebaseReason::Lag {
                                    buffer.record_transport_gap(1);
                                }
                                buffer.replace_with_keyframe(&rebase.keyframe);
                            } else {
                                buffer.push(&rebase.keyframe);
                            }
                            if !rebase.coverage.history_complete() {
                                // The keyframe could not carry every retained row,
                                // so scrollback above it is genuinely missing.
                                buffer.record_history_shortfall(
                                    rebase
                                        .coverage
                                        .history_rows_total
                                        .saturating_sub(rebase.coverage.history_rows_included),
                                );
                            }
                        }
                        cv.notify_one();
                    }
                    Ok(Some(rmux_sdk::PaneRecoveryEvent::Bytes { bytes, .. })) => {
                        observer.on_output(&sid, bytes.len());
                        let (lock, cv) = &*pending;
                        lock.lock().unwrap().push(&bytes);
                        cv.notify_one();
                    }
                    Ok(Some(rmux_sdk::PaneRecoveryEvent::End(reason))) => {
                        tracing::debug!(session_id = %sid, ?reason, "rmux recovery stream ended");
                        break;
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(error) => {
                        tracing::debug!(session_id = %sid, %error, "rmux stream ended");
                        break;
                    }
                }
            }

            done.store(true, Ordering::Release);
            pending.1.notify_one();
        });

        Ok(())
    }

    /// Write input bytes to a session's PTY and wait for transport acknowledgement.
    pub async fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let acknowledgement = {
            let sessions = self.sessions.read().map_err(|e| e.to_string())?;
            let backend = sessions.get(session_id).ok_or("session not attached")?;
            backend.write(data)?
        };
        acknowledgement.wait().await
    }

    /// Resize a session's PTY.
    pub fn resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<(), String> {
        let sessions = self.sessions.read().map_err(|e| e.to_string())?;
        let backend = sessions.get(session_id).ok_or("session not attached")?;
        backend.resize(rows, cols)
    }

    /// Connect new output to a local PTY still running, never starting one.
    pub fn reconnect(&self, pty_key: &str, on_data: Channel<Response>) -> Result<(), String> {
        let sessions = self.sessions.read().map_err(|e| e.to_string())?;
        match sessions.get(pty_key) {
            Some(existing) if existing.rebind_output(on_data) => Ok(()),
            _ => Err(gone_error(pty_key)),
        }
    }

    /// Detach a session's PTY (cleanup).
    pub fn detach(&self, session_id: &str) {
        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        if let Some(backend) = sessions.remove(session_id) {
            backend.detach();
        }
    }

    /// Whether a process runs behind the key; backends that cannot tell count as running.
    pub fn is_running(&self, pty_key: &str) -> bool {
        self.sessions
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(pty_key)
            .is_some_and(|backend| !backend.has_exited())
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

#[cfg(test)]
mod tests {
    use super::{daemon_frame_output, PtyManager};
    use crate::session_backend::{SessionBackend, WriteAck};
    use planeai_daemon::protocol::{FRAME_EOF, FRAME_GAP, FRAME_OUTPUT};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[derive(Default)]
    struct FakeBackend {
        detached: Arc<AtomicBool>,
        exited: Arc<AtomicBool>,
    }

    impl SessionBackend for FakeBackend {
        fn write(&self, _data: &[u8]) -> Result<WriteAck, String> {
            Ok(WriteAck::Immediate)
        }
        fn resize(&self, _rows: u16, _cols: u16) -> Result<(), String> {
            Ok(())
        }
        fn pause(&self) -> Result<(), String> {
            Ok(())
        }
        fn resume(&self) -> Result<(), String> {
            Ok(())
        }
        fn detach(&self) {
            self.detached.store(true, Ordering::SeqCst);
        }
        fn has_exited(&self) -> bool {
            self.exited.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn publishing_replaces_and_ends_the_previous_backend() {
        let manager = PtyManager::new(Arc::new(|_: &str| {}));
        let first = FakeBackend::default();
        let first_detached = first.detached.clone();
        manager.publish("s1:2", Box::new(first)).unwrap();
        manager
            .publish("s1:2", Box::new(FakeBackend::default()))
            .unwrap();
        assert!(first_detached.load(Ordering::SeqCst));
        assert!(manager.is_running("s1:2"));
    }

    #[test]
    fn a_backend_runs_until_it_exits_or_detaches() {
        let manager = PtyManager::new(Arc::new(|_: &str| {}));
        let backend = FakeBackend::default();
        let (exited, detached) = (backend.exited.clone(), backend.detached.clone());
        manager.publish("s1:2", Box::new(backend)).unwrap();
        assert!(manager.is_running("s1:2"));
        exited.store(true, Ordering::SeqCst);
        assert!(!manager.is_running("s1:2"));
        manager.detach("s1:2");
        assert!(detached.load(Ordering::SeqCst));
        assert!(!manager.is_running("s1:2"));
    }

    #[test]
    fn daemon_output_frames_reach_the_terminal_unchanged() {
        assert_eq!(
            daemon_frame_output("s1", FRAME_OUTPUT, b"hello".to_vec()),
            Some(b"hello".to_vec())
        );
        assert_eq!(daemon_frame_output("s1", FRAME_EOF, Vec::new()), None);
    }

    #[test]
    fn a_daemon_gap_frame_shows_a_notice_instead_of_its_json() {
        let shown = daemon_frame_output("s1", FRAME_GAP, br#"{"lagged":7}"#.to_vec());

        assert_eq!(
            shown.map(String::from_utf8),
            Some(Ok(
                "\r\n\x1b[2m[planeai] output gap: 7 output event(s) dropped by the \
                     PlaneAI daemon - this session fell behind\x1b[0m\r\n"
                    .to_string()
            ))
        );
    }
}
