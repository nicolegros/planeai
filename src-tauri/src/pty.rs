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

fn closed_before_start_error(pty_key: &str) -> String {
    format!("{pty_key} was closed before its program started")
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

/// Where a tab's close stands, for a spawn of a program in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloseState {
    /// Claimed, but no backend of the tab has been ended by it: if a program is spawning in the
    /// tab, the close raced it, and the program must not start.
    Closing,
    /// The close ended the tab's backend; a later spawn on its key, after a webview reload
    /// reused it, starts fresh.
    Closed,
}

#[derive(Clone)]
pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, Box<dyn SessionBackend>>>>,
    observer: Arc<RwLock<Arc<dyn OutputObserver>>>,
    /// Tabs being closed, or whose close ended their backend.
    tab_closes: Arc<Mutex<HashMap<String, CloseState>>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            observer: Arc::new(RwLock::new(Arc::new(NoopObserver))),
            tab_closes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn set_observer(&self, observer: Arc<dyn OutputObserver>) {
        *self.observer.write().unwrap() = observer;
    }

    /// Claim responsibility for closing a tab: false while another close of it is under way.
    /// A finished close is claimed again, since a tab reusing its key may close in turn.
    pub(crate) fn claim_tab_close(&self, pty_key: &str) -> bool {
        let previous = self
            .tab_closes()
            .insert(pty_key.to_string(), CloseState::Closing);
        previous != Some(CloseState::Closing)
    }

    /// Release a close claim when closing failed so a later retry can proceed.
    pub(crate) fn cancel_tab_close(&self, pty_key: &str) {
        self.tab_closes().remove(pty_key);
    }

    fn tab_closes(&self) -> std::sync::MutexGuard<'_, HashMap<String, CloseState>> {
        self.tab_closes.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Before a spawn the key starts fresh, refusing a program whose tab closed before anything
    /// ran in it; a close seen at publish can then only be one racing this spawn.
    fn prepare_attach(&self, pty_key: &str, runs_program: bool) -> Result<(), String> {
        let state = self.tab_closes().remove(pty_key);
        if runs_program && state == Some(CloseState::Closing) {
            return Err(closed_before_start_error(pty_key));
        }
        Ok(())
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
        let runs_program = matches!(target, PtyTarget::Program { .. });
        self.prepare_attach(session_id, runs_program)?;

        // Handle daemon target via async path
        if let PtyTarget::Daemon {
            session_id: sid,
            socket_path,
        } = target
        {
            return self.attach_daemon(&sid, socket_path, app, on_data);
        }

        // Handle rmux target via async path
        if let PtyTarget::Rmux {
            pty_key,
            workspace,
            handle,
        } = target
        {
            return self.attach_rmux(&pty_key, workspace, handle, app, on_data);
        }

        // A shell terminal can remount when its split leaf changes. Rebind its
        // output channel instead of killing and recreating the running local PTY.
        if matches!(&target, PtyTarget::Shell { .. } | PtyTarget::Program { .. }) {
            let sessions = self.sessions.read().map_err(|e| e.to_string())?;
            if let Some(existing) = sessions.get(session_id) {
                if existing.rebind_output(on_data.clone()) {
                    return Ok(());
                }
            }
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
            session_id, command, &cwd, env, app, on_data, cancelled, observer,
        )?;
        self.publish(session_id, runs_program, Box::new(backend))
    }

    /// Make a spawned local backend the session's, replacing any previous one. Checked again
    /// under the sessions lock: a close claimed during the spawn either shows here, or its
    /// detach waits for this lock and then ends the backend.
    fn publish(
        &self,
        session_id: &str,
        runs_program: bool,
        backend: Box<dyn SessionBackend>,
    ) -> Result<(), String> {
        let mut sessions = self.sessions.write().map_err(|e| e.to_string())?;
        if runs_program && self.tab_closes().remove(session_id).is_some() {
            backend.detach();
            return Err(closed_before_start_error(session_id));
        }
        if let Some(old) = sessions.get(session_id) {
            old.detach();
        }
        sessions.insert(session_id.to_string(), backend);
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
        app: AppHandle,
        on_data: Channel<Response>,
    ) -> Result<(), String> {
        tracing::info!(pty_key, %workspace, "attaching to rmux resource");
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
                    let _ = app.emit("pty-exited", serde_json::json!({ "pty_key": sid }));
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
            let app_flusher = app.clone();
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
                                    let _ = app_flusher.emit(
                                        "pty-exited",
                                        serde_json::json!({ "pty_key": exit_key }),
                                    );
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
                    let _ =
                        app_flusher.emit("pty-exited", serde_json::json!({ "pty_key": exit_key }));
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

    /// Detach a session's PTY (cleanup).
    pub fn detach(&self, session_id: &str) {
        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        if let Some(backend) = sessions.remove(session_id) {
            backend.detach();
            if let Some(state) = self.tab_closes().get_mut(session_id) {
                *state = CloseState::Closed;
            }
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

#[cfg(test)]
mod tests {
    use super::PtyManager;
    use crate::session_backend::{SessionBackend, WriteAck};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// A spawned backend that records whether it was ended.
    #[derive(Default)]
    struct FakeBackend {
        detached: Arc<AtomicBool>,
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
    }

    fn spawned() -> (Box<dyn SessionBackend>, Arc<AtomicBool>) {
        let backend = FakeBackend::default();
        let detached = backend.detached.clone();
        (Box::new(backend), detached)
    }

    #[test]
    fn a_program_tab_closed_before_its_spawn_never_starts() {
        let manager = PtyManager::new();
        manager.prepare_attach("s1:2", true).unwrap();
        assert!(manager.claim_tab_close("s1:2"));
        assert!(manager.prepare_attach("s1:2", true).is_err());
        // The refusal consumed the claim: a later tab on the key starts.
        manager.prepare_attach("s1:2", true).unwrap();
    }

    #[test]
    fn a_shell_tab_reattaching_is_closable_again() {
        let manager = PtyManager::new();
        assert!(manager.claim_tab_close("s1:2"));
        manager.prepare_attach("s1:2", false).unwrap();
        assert!(manager.claim_tab_close("s1:2"));
    }

    #[test]
    fn a_program_tab_closed_during_its_spawn_is_ended_instead_of_published() {
        let manager = PtyManager::new();
        let (backend, detached) = spawned();
        assert!(manager.claim_tab_close("s1:2"));
        assert!(manager.publish("s1:2", true, backend).is_err());
        assert!(detached.load(Ordering::SeqCst));
        assert!(!manager.sessions.read().unwrap().contains_key("s1:2"));
        manager.prepare_attach("s1:2", true).unwrap();
    }

    #[test]
    fn a_shell_tab_publishes_whatever_its_close_claim() {
        let manager = PtyManager::new();
        let (backend, detached) = spawned();
        assert!(manager.claim_tab_close("s1:2"));
        manager.publish("s1:2", false, backend).unwrap();
        assert!(!detached.load(Ordering::SeqCst));
        assert!(manager.sessions.read().unwrap().contains_key("s1:2"));
    }

    #[test]
    fn a_close_after_publishing_ends_the_program_and_frees_its_key() {
        let manager = PtyManager::new();
        let (backend, detached) = spawned();
        manager.publish("s1:2", true, backend).unwrap();
        assert!(manager.claim_tab_close("s1:2"));
        manager.detach("s1:2");
        assert!(detached.load(Ordering::SeqCst));
        // After a webview reload the layout may hand the key out again.
        manager.prepare_attach("s1:2", true).unwrap();
    }

    #[test]
    fn a_new_close_of_a_reused_key_still_stops_its_program() {
        let manager = PtyManager::new();
        let (backend, _) = spawned();
        manager.publish("s1:2", true, backend).unwrap();
        assert!(manager.claim_tab_close("s1:2"));
        manager.detach("s1:2");
        // A webview reload reuses the key, and its new tab closes before its program spawns.
        assert!(manager.claim_tab_close("s1:2"));
        assert!(manager.prepare_attach("s1:2", true).is_err());
    }

    #[test]
    fn a_detach_without_a_close_leaves_no_state_behind() {
        let manager = PtyManager::new();
        let (backend, _) = spawned();
        manager.publish("s1:2", false, backend).unwrap();
        manager.detach("s1:2");
        assert!(manager.tab_closes().is_empty());
        // So a later close that never ends anything still refuses a program spawn.
        assert!(manager.claim_tab_close("s1:2"));
        assert!(manager.prepare_attach("s1:2", true).is_err());
    }

    #[test]
    fn a_closed_shell_key_reused_for_a_program_starts() {
        let manager = PtyManager::new();
        let (shell, _) = spawned();
        manager.publish("s1:3", false, shell).unwrap();
        assert!(manager.claim_tab_close("s1:3"));
        manager.detach("s1:3");
        manager.prepare_attach("s1:3", true).unwrap();
        // A close racing this new spawn is still caught when it publishes.
        assert!(manager.claim_tab_close("s1:3"));
        let (program, detached) = spawned();
        assert!(manager.publish("s1:3", true, program).is_err());
        assert!(detached.load(Ordering::SeqCst));
    }

    #[test]
    fn a_tab_close_claim_blocks_a_concurrent_close_until_cancelled_or_reattached() {
        let manager = PtyManager::new();
        let pty_key = "session-1:2";

        assert!(manager.claim_tab_close(pty_key));
        assert!(
            !manager.claim_tab_close(pty_key),
            "a duplicate close must not claim the tab while its first close is under way"
        );

        manager.cancel_tab_close(pty_key);
        assert!(
            manager.claim_tab_close(pty_key),
            "a failed close must remain retryable"
        );

        manager.prepare_attach(pty_key, false).unwrap();
        assert!(
            manager.claim_tab_close(pty_key),
            "a newly attached PTY key must be closable again"
        );
    }
}
