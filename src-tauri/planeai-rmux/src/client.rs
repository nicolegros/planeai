//! Client for PlaneAI's private rmux daemon.
//!
//! One rmux session hosts a whole task workspace; each PlaneAI terminal resource
//! is one **window** inside it holding a single pane. The window matters: probe 2
//! in ADR-0012 showed `Pane::resize` is a no-op for a sole pane, so geometry
//! changes must address the window, and PlaneAI tabs are independently sized
//! full terminals rather than simultaneously visible splits.

use std::collections::HashSet;
use std::time::Duration;

use rmux_sdk::{
    EnsureSession, Pane, PaneId, PaneRecoveryStream, Rmux, Session, SessionName, TerminalSizeSpec,
    Window,
};

use crate::config::{Endpoint, RmuxConfig};
use crate::error::{means_pane_gone, Error, Result};
use crate::naming::{self, WorkspaceName};

/// How much scrollback an AXI read considers. Matches the tmux backend's window.
const SCROLLBACK_LINES: i64 = 10_000;

/// A terminal resource to create inside a workspace.
#[derive(Debug, Clone)]
pub struct ResourceSpawn {
    /// Which rmux session hosts it.
    pub workspace: WorkspaceName,
    /// PlaneAI's canonical identity for the resource (`session_id` or
    /// `session_id:tab_index`). Used as the rmux window name so a live daemon is
    /// inspectable with `rmux list-windows`.
    pub pty_key: String,
    /// Command line, run as explicit argv under the platform shell.
    pub command: String,
    /// Working directory: the worktree or project checkout for this resource.
    pub cwd: String,
    /// Environment entries as `KEY=VALUE`, already carrying PlaneAI's augmented PATH.
    pub env: Vec<String>,
    pub cols: u16,
    pub rows: u16,
}

/// The renewable handle to a spawned resource.
///
/// `PaneId` is stable for a daemon lifetime and survives window index
/// recompression, which window indices do not. It is not durable across a daemon
/// restart; PlaneAI's own `pty_key` remains the canonical identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceHandle {
    pub pane_id: PaneId,
}

impl ResourceHandle {
    pub fn as_u32(self) -> u32 {
        self.pane_id.as_u32()
    }

    pub fn from_u32(value: u32) -> Self {
        Self {
            pane_id: PaneId::new(value),
        }
    }
}

/// A live pane inside a PlaneAI workspace, as the daemon reports it.
///
/// Carries the workspace name rather than a [`WorkspaceName`] because the daemon
/// is the source here: a name PlaneAI no longer recognises must still be
/// reportable, so parsing is left to the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePane {
    pub workspace_key: String,
    pub pane_id: u32,
}

/// A live pane plus the handles needed to drive and resize it.
pub struct AttachedPane {
    pane: Pane,
    window: Window,
    stream: PaneRecoveryStream,
}

impl AttachedPane {
    pub fn into_parts(self) -> (Pane, Window, PaneRecoveryStream) {
        (self.pane, self.window, self.stream)
    }
}

/// Connection to the private daemon, starting it if necessary.
pub struct RmuxClient {
    rmux: Rmux,
    config: RmuxConfig,
}

impl RmuxClient {
    /// Connect, starting the daemon if it is not listening.
    ///
    /// Always uses `connect_or_start`: killing the last rmux session terminates
    /// the daemon, so a bare `connect` would fail for an ordinary, recoverable
    /// reason (ADR-0012).
    ///
    /// Retries a transport failure with backoff. A daemon that is shutting down
    /// leaves its socket in place for a moment, and connecting during that window
    /// succeeds at the socket and then hits EOF — so a single attempt reports
    /// "daemon closed the transport" even though starting a fresh one would work.
    pub async fn connect(config: RmuxConfig) -> Result<Self> {
        if let Some(parent) = config.socket_parent() {
            // A Unix socket cannot be bound into a directory that does not exist.
            if let Err(error) = std::fs::create_dir_all(parent) {
                tracing::warn!(
                    directory = %parent.display(),
                    %error,
                    "could not create rmux runtime directory"
                );
            }
        }
        if let Some(binary) = config.daemon_binary() {
            // The SDK would otherwise search only this process's own PATH.
            std::env::set_var(crate::config::DAEMON_BINARY_ENV, binary);
        }

        const BACKOFF_MS: &[u64] = &[0, 50, 100, 200, 400, 800];
        let mut last_error = None;
        for delay in BACKOFF_MS {
            if *delay > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(*delay)).await;
            }
            match Self::attempt_connect(&config).await {
                Ok(rmux) => {
                    tracing::info!(endpoint = %config.label(), "connected to private rmux daemon");
                    return Ok(Self { rmux, config });
                }
                Err(source) => {
                    let transient = matches!(source, rmux_sdk::RmuxError::Transport { .. });
                    last_error = Some(source);
                    if !transient {
                        break;
                    }
                }
            }
        }

        Err(Error::DaemonUnavailable {
            endpoint: config.label(),
            source: last_error.expect("at least one attempt was made"),
        })
    }

    /// Connect to a daemon already running, never starting one: `None` when none listens,
    /// which hosts nothing. For lookups and teardown, which only need to learn that.
    pub async fn connect_existing(config: RmuxConfig) -> Result<Option<Self>> {
        // A missing pipe would otherwise be waited for until the connect times out.
        #[cfg(windows)]
        if pipe_missing(&config) {
            return Ok(None);
        }
        let builder = match config.endpoint() {
            Endpoint::UnixSocket(path) => Rmux::builder().unix_socket(path.clone()),
            Endpoint::WindowsPipe(pipe) => Rmux::builder().windows_pipe(pipe.clone()),
        };
        match builder.connect().await {
            Ok(rmux) => Ok(Some(Self { rmux, config })),
            Err(rmux_sdk::RmuxError::Transport { source, .. })
                if matches!(
                    source.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                Ok(None)
            }
            // A daemon stopping while the connect retried: its pipe is gone now.
            #[cfg(windows)]
            Err(_) if pipe_missing(&config) => Ok(None),
            Err(source) => Err(Error::DaemonUnavailable {
                endpoint: config.label(),
                source,
            }),
        }
    }

    async fn attempt_connect(
        config: &RmuxConfig,
    ) -> std::result::Result<Rmux, rmux_sdk::RmuxError> {
        let builder = match config.endpoint() {
            Endpoint::UnixSocket(path) => Rmux::builder().unix_socket(path.clone()),
            Endpoint::WindowsPipe(pipe) => Rmux::builder().windows_pipe(pipe.clone()),
        };
        builder.connect_or_start().await
    }

    pub fn config(&self) -> &RmuxConfig {
        &self.config
    }

    /// The pane hosting `pty_key` in this workspace, if the daemon still has it.
    ///
    /// Correlates on the window name, which [`Self::spawn_resource`] sets at
    /// creation time for every resource. This is what makes a spawn safe to
    /// repeat: `rmux_client::with_retry` re-runs an operation when the transport
    /// dies, and a transport failure cannot distinguish "the daemon never saw the
    /// request" from "the daemon spawned the pane and the reply was lost". Without
    /// correlation the retry adds a second pane and abandons the first, which is
    /// the orphan probe 5 produced (ADR-0012).
    ///
    /// Terminal tabs also reconnect and close through it, never trusting a recorded pane
    /// id. Costs one round trip per pane in the workspace. Fails rather than answer "none"
    /// when a pane that may be this one could not be read, or the connection died.
    pub async fn find_resource(
        &self,
        workspace: &WorkspaceName,
        pty_key: &str,
    ) -> Result<Option<ResourceHandle>> {
        let name = self.workspace_name(workspace)?;
        let panes = match self.rmux.find_panes().session(name.as_ref()).all().await {
            Ok(panes) => panes,
            Err(source) => {
                let error = Error::Sdk {
                    operation: "find_panes",
                    source,
                };
                // No workspace means no resource, which is an answer rather than
                // a failure. A dead connection is not: `with_retry` reconnects and asks again.
                return if error.confirms_session_absent() {
                    Ok(None)
                } else {
                    Err(error)
                };
            }
        };
        if panes.is_empty() {
            return Ok(None);
        }

        let session = match self.session(&name).await {
            Ok(session) => session,
            Err(error) if error.confirms_session_absent() => return Ok(None),
            Err(error) => return Err(error),
        };
        // Unreadable panes that may be this one.
        let mut unread = None;
        for discovered in panes {
            // Resolve through the pane rather than a window index so no
            // assumption about index bases is baked in (ADR-0012 records that
            // rmux's own reference was wrong about this).
            let read = async { session.pane_by_id(discovered.pane_id).await?.info().await };
            let snapshot = match read.await {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    if !means_pane_gone(&error) {
                        unread = Some(error);
                    }
                    continue;
                }
            };
            let names_this_resource = snapshot
                .windows
                .iter()
                .any(|window| window.name.as_deref() == Some(pty_key));
            if names_this_resource {
                return Ok(Some(ResourceHandle {
                    pane_id: discovered.pane_id,
                }));
            }
        }
        unread.map_or(Ok(None), |source| Err(Error::sdk("read_pane")(source)))
    }

    /// Create a terminal resource, materialising its workspace if needed.
    ///
    /// Idempotent per `pty_key`: a resource that already exists is adopted rather
    /// than duplicated, so this is safe to retry after an ambiguous failure. See
    /// [`Self::find_resource`].
    ///
    /// No `OwnedSession` lease is taken: probe 1 (ADR-0012) showed sessions made
    /// this way already outlive the client that created them, which is the
    /// persistence this backend exists for. A lease would only add a way to kill
    /// them by accident.
    pub async fn spawn_resource(&self, spawn: &ResourceSpawn) -> Result<ResourceHandle> {
        if let Some(existing) = self.find_resource(&spawn.workspace, &spawn.pty_key).await? {
            tracing::info!(
                pty_key = %spawn.pty_key,
                pane_id = existing.as_u32(),
                "adopting the rmux pane already hosting this resource instead of spawning a second"
            );
            return Ok(existing);
        }

        let name = self.workspace_name(&spawn.workspace)?;
        let size = TerminalSizeSpec::new(spawn.cols, spawn.rows);

        // `create_or_reuse` makes this idempotent per workspace: the first
        // resource creates the session, later ones join it.
        let session = self
            .rmux
            .ensure_session(
                EnsureSession::named(name.clone())
                    .create_or_reuse()
                    .detached(true)
                    .argv(shell_argv(&spawn.command))
                    .working_directory(spawn.cwd.clone())
                    .environment(spawn.env.clone())
                    // Named here rather than renamed afterwards so the name a
                    // retry correlates on is set by the same request that creates
                    // the window. A later rename can be lost with the reply.
                    .window_name(spawn.pty_key.clone())
                    .size(size),
            )
            .await
            .map_err(Error::sdk("ensure_session"))?;

        // The session's initial window already runs this resource's command when
        // the session was created for it, so adopt that window instead of
        // spawning a second copy.
        if session.was_created() {
            let adopted = self.first_pane_id(&name).await?;
            tracing::info!(
                workspace = %name.as_ref(),
                pty_key = %spawn.pty_key,
                "rmux workspace created with its first resource"
            );
            return Ok(ResourceHandle { pane_id: adopted });
        }

        // Every resource needs the same environment, not just the first. The
        // initial pane gets it through `ensure_session`; a window created later
        // must be given it explicitly, or the child runs without PlaneAI's
        // augmented PATH and a configured tool like an editor is simply not found.
        let mut builder = session
            .new_window_with()
            .name(spawn.pty_key.clone())
            .spawn(shell_argv(&spawn.command))
            .cwd(spawn.cwd.clone());
        for entry in &spawn.env {
            if let Some((key, value)) = entry.split_once('=') {
                builder = builder.env(key, value);
            }
        }
        let window = builder.await.map_err(Error::sdk("new_window"))?;
        let panes = window.panes().await.map_err(Error::sdk("window_panes"))?;
        let pane_id = panes
            .first()
            .ok_or_else(|| Error::NoPane {
                session_id: spawn.pty_key.clone(),
            })?
            .id;

        tracing::info!(
            workspace = %name.as_ref(),
            pty_key = %spawn.pty_key,
            "rmux resource added to existing workspace"
        );
        Ok(ResourceHandle { pane_id })
    }

    /// Open an output stream for an existing resource.
    ///
    /// Never creates a process: attach and spawn are distinct in PlaneAI's
    /// lifecycle, and an attach that spawned would resurrect a killed agent.
    pub async fn attach_resource(
        &self,
        workspace: &WorkspaceName,
        handle: ResourceHandle,
    ) -> Result<AttachedPane> {
        let name = self.workspace_name(workspace)?;
        let located = self
            .rmux
            .find_panes()
            .session(name.as_ref())
            .all()
            .await
            .map_err(Error::sdk("find_panes"))?;
        let found = located
            .iter()
            .find(|candidate| candidate.pane_id == handle.pane_id)
            .ok_or_else(|| Error::ResourceGone {
                pane_id: handle.as_u32(),
            })?;

        let session = self.session(&name).await?;
        let pane = session
            .pane_by_id(handle.pane_id)
            .await
            .map_err(Error::sdk("pane_by_id"))?;
        let window = session.window(found.window_index);
        // The recovery stream, not a raw stream anchored at `Oldest`. Both replay
        // earlier output, but only this one opens with a `Rebase` carrying an
        // authoritative keyframe — ANSI bytes that reset and reconstruct the screen,
        // including the alternate buffer and title — and re-issues one whenever
        // continuation breaks (lag, resize, clear-history, terminal reset, a new
        // process generation). Replaying raw retained bytes can only reconstruct
        // while the whole transcript is still retained, and silently diverges once
        // it is not (ADR-0012).
        let stream = pane
            .recover_output()
            .await
            .map_err(Error::sdk("recover_output"))?;

        Ok(AttachedPane {
            pane,
            window,
            stream,
        })
    }

    /// Resolve a resource's pane.
    async fn pane(&self, workspace: &WorkspaceName, handle: ResourceHandle) -> Result<Pane> {
        let name = self.workspace_name(workspace)?;
        let session = self.session(&name).await?;
        session
            .pane_by_id(handle.pane_id)
            .await
            .map_err(Error::sdk("pane_by_id"))
    }

    /// Send a prompt and submit it.
    ///
    /// The text and the Enter byte ([`SUBMIT_BYTE`]) go out as separate inputs,
    /// with Enter held back until the pane has finished echoing the text. An
    /// agent TUI reads input arriving in one burst as a paste, and an Enter inside
    /// a paste becomes a newline in the prompt instead of submitting it. A fixed
    /// delay is not enough: a large multi-line prompt, such as editor feedback
    /// carrying code context, takes the agent longer to ingest than a short one.
    pub async fn submit_text(
        &self,
        workspace: &WorkspaceName,
        handle: ResourceHandle,
        text: &str,
    ) -> Result<()> {
        let pane = self.pane(workspace, handle).await?;
        // Subscribed before writing, so the echo cannot be missed.
        let mut output = pane
            .output_stream()
            .await
            .map_err(Error::sdk("output_stream"))?;
        // Writes are not retried as transport errors: a lost reply cannot be told
        // apart from lost input, and a retry could type the text twice.
        let delivery = |source| Error::PromptDelivery { source };
        pane.send_text(text).await.map_err(delivery)?;
        wait_for_echo_to_settle(&mut output, SUBMIT_QUIET, SUBMIT_SETTLE_LIMIT).await;
        pane.send_text(SUBMIT_BYTE).await.map_err(delivery)
    }

    /// Capture a resource's pane as plain text, ANSI stripped.
    ///
    /// This is what AXI reads are built on. rmux offers no byte-offset resume, so
    /// incremental reads compare captures rather than seeking (ADR-0012).
    ///
    /// Trailing blank rows are removed. A capture is a fixed-height grid, so an
    /// unfilled pane is padded to its full row count; keeping that padding would
    /// hold the line count constant while content changed, and every incremental
    /// read would look like a history rewrite and redeliver everything.
    pub async fn capture_resource_text(
        &self,
        workspace: &WorkspaceName,
        handle: ResourceHandle,
    ) -> Result<String> {
        let pane = self.pane(workspace, handle).await?;
        let capture = pane
            .capture_pane()
            .escape_sequences(false)
            // Start in scrollback and let the end default to the bottom of the
            // visible pane. An explicit `end(-1)` would stop at the last history
            // line and omit the screen, where the newest output is. The tmux
            // backend omits `-E` for the same reason.
            .start(-SCROLLBACK_LINES)
            .await
            .map_err(Error::sdk("capture_pane"))?;
        let text = String::from_utf8_lossy(&capture.stdout);
        Ok(trim_trailing_blank_lines(&text))
    }

    /// The capture exactly as the daemon returned it, before trimming.
    ///
    /// Exists so a test can assert what rmux actually sends — that an unfilled
    /// pane really is padded, and that lines really are newline-terminated —
    /// rather than those remaining assumptions inherited from tmux.
    #[doc(hidden)]
    pub async fn capture_raw_for_test(
        &self,
        workspace: &WorkspaceName,
        handle: ResourceHandle,
    ) -> Result<String> {
        let pane = self.pane(workspace, handle).await?;
        let capture = pane
            .capture_pane()
            .escape_sequences(false)
            .start(-SCROLLBACK_LINES)
            .await
            .map_err(Error::sdk("capture_pane"))?;
        Ok(String::from_utf8_lossy(&capture.stdout).to_string())
    }

    /// Close one resource, leaving sibling resources running.
    ///
    /// Note what this does *not* protect against: rmux drops a session once its
    /// last window closes, so closing the only remaining resource removes the
    /// workspace too (measured by `closing_a_workspaces_last_pane_removes_the_workspace`).
    /// The orphan sweep relies on that — it closes panes and never workspaces, and
    /// is complete only because the workspace goes with them. A caller that needs
    /// the workspace to survive has to keep a resource in it.
    pub async fn close_resource(
        &self,
        workspace: &WorkspaceName,
        handle: ResourceHandle,
    ) -> Result<()> {
        let name = self.workspace_name(workspace)?;
        // Already gone is the desired end state, but only the daemon can say so: a dead
        // connection fails, for `with_retry` to reconnect and ask again.
        let session = match self.session(&name).await {
            Ok(session) => session,
            Err(error) if error.confirms_session_absent() => return Ok(()),
            Err(error) => return Err(error),
        };
        let pane = match session.pane_by_id(handle.pane_id).await {
            Ok(pane) => pane,
            Err(source) if means_pane_gone(&source) => return Ok(()),
            Err(source) => return Err(Error::sdk("pane_by_id")(source)),
        };
        match pane.close().await {
            Ok(_) => Ok(()),
            Err(source) if means_pane_gone(&source) => Ok(()),
            Err(source) => Err(Error::sdk("close_pane")(source)),
        }
    }

    /// Remove an entire workspace and every resource in it.
    ///
    /// Reserved for task deletion: a workspace outlives the agents inside it
    /// (ADR-0012), so ordinary archive/destroy closes resources instead.
    pub async fn kill_workspace(&self, workspace: &WorkspaceName) -> Result<()> {
        let name = self.workspace_name(workspace)?;
        let session = match self.session(&name).await {
            Ok(session) => session,
            Err(error) if error.means_session_absent() => return Ok(()),
            Err(error) => return Err(error),
        };
        match session.kill().await {
            Ok(_) => Ok(()),
            Err(source) => {
                let error = Error::Sdk {
                    operation: "kill_workspace",
                    source,
                };
                if error.means_session_absent() {
                    Ok(())
                } else {
                    Err(error)
                }
            }
        }
    }

    /// Pane ids the daemon currently hosts for PlaneAI, for reconciliation.
    pub async fn live_pane_ids(&self) -> Result<HashSet<u32>> {
        Ok(self
            .live_panes()
            .await?
            .into_iter()
            .map(|pane| pane.pane_id)
            .collect())
    }

    /// Every pane the daemon hosts for PlaneAI, with the workspace holding it.
    ///
    /// [`Self::live_pane_ids`] answers "is this recorded pane still alive".  This
    /// answers the inverse — "which live panes does PlaneAI no longer account
    /// for" — which also needs the workspace, because closing a pane is addressed
    /// by workspace and handle.
    pub async fn live_panes(&self) -> Result<Vec<LivePane>> {
        let panes = self
            .rmux
            .find_panes()
            .all()
            .await
            .map_err(Error::sdk("find_panes"))?;
        Ok(panes
            .iter()
            .filter(|pane| naming::is_planeai_session(pane.session_name.as_ref()))
            .map(|pane| LivePane {
                workspace_key: pane.session_name.as_ref().to_string(),
                pane_id: pane.pane_id.as_u32(),
            })
            .collect())
    }

    /// Every workspace the daemon currently hosts for PlaneAI.
    ///
    /// Distinct from [`Self::live_panes`] because a workspace can outlive its last
    /// pane: closing a resource deliberately leaves the session standing, so an
    /// emptied workspace has no panes to discover but is still hosted.
    pub async fn live_workspaces(&self) -> Result<Vec<String>> {
        let sessions = self
            .rmux
            .find_sessions()
            .all()
            .await
            .map_err(Error::sdk("find_sessions"))?;
        Ok(sessions
            .iter()
            .map(|session| session.name.as_ref().to_string())
            .filter(|name| naming::is_planeai_session(name))
            .collect())
    }

    /// Whether a workspace currently exists on the daemon.
    ///
    /// An unreachable daemon answers "no" rather than failing, though it may hide a daemon still
    /// running: removing the last workspace stops the daemon, and callers asking this question
    /// want the state, not the transport detail.
    pub async fn workspace_exists(&self, workspace: &WorkspaceName) -> Result<bool> {
        let name = self.workspace_name(workspace)?;
        match self.rmux.find_sessions().name(name.as_ref()).all().await {
            Ok(sessions) => Ok(!sessions.is_empty()),
            Err(source) => {
                let error = Error::Sdk {
                    operation: "find_sessions",
                    source,
                };
                if error.means_session_absent() {
                    Ok(false)
                } else {
                    Err(error)
                }
            }
        }
    }

    async fn first_pane_id(&self, name: &SessionName) -> Result<PaneId> {
        let panes = self
            .rmux
            .find_panes()
            .session(name.as_ref())
            .all()
            .await
            .map_err(Error::sdk("find_panes"))?;
        panes
            .first()
            .map(|pane| pane.pane_id)
            .ok_or_else(|| Error::NoPane {
                session_id: name.as_ref().to_string(),
            })
    }

    async fn session(&self, name: &SessionName) -> Result<Session> {
        self.rmux
            .session(name.clone())
            .await
            .map_err(Error::sdk("session"))
    }

    fn workspace_name(&self, workspace: &WorkspaceName) -> Result<SessionName> {
        SessionName::new(workspace.as_str()).map_err(|source| Error::SessionName {
            session_id: workspace.as_str().to_string(),
            reason: source.to_string(),
        })
    }
}

/// The byte an agent reads as Enter.
///
/// Carriage return, not line feed. Agents run their TUI in raw mode, where the
/// terminal driver performs no CR/LF translation and Enter arrives literally as
/// `\r` (0x0D). A `\n` is accepted into the input buffer but submits nothing, so
/// the prompt sits on the input line while the send reports success.
///
/// This matches the other backends: `planeai-daemon` pushes `b'\r'` onto the
/// input payload, and tmux's separate `send-keys Enter` resolves to CR. The
/// `Enter` *key name* is what tmux translates - the newline in a shell string is
/// not the same thing, which is the assumption that made this wrong at first.
const SUBMIT_BYTE: &str = "\r";

/// How long a pane must stay silent after echoing a prompt before Enter is sent.
const SUBMIT_QUIET: Duration = Duration::from_millis(200);

/// Upper bound on waiting for that silence: a busy agent animating a spinner
/// never goes quiet, and one that does not echo never starts.
const SUBMIT_SETTLE_LIMIT: Duration = Duration::from_secs(3);

/// Pane output, reduced to what settling needs.
trait OutputChunks {
    /// Resolve `true` for each output chunk and `false` once the stream has ended.
    async fn next_chunk(&mut self) -> bool;
}

impl OutputChunks for rmux_sdk::PaneOutputStream {
    async fn next_chunk(&mut self) -> bool {
        match self.next().await {
            Ok(Some(_)) => true,
            Ok(None) => false,
            // A failed poll says nothing about the echo, so wait out the window
            // instead of sending Enter into a paste still in flight.
            Err(error) => {
                tracing::warn!(%error, "rmux output poll failed while settling a prompt");
                std::future::pending().await
            }
        }
    }
}

/// Wait until output has started and then paused for `quiet`, or `limit` passes.
///
/// Waiting for the echo to start, not just for silence, matters: an agent still
/// collecting a paste may render nothing until it is done, so silence before the
/// echo does not mean the text was ingested.
async fn wait_for_echo_to_settle(output: &mut impl OutputChunks, quiet: Duration, limit: Duration) {
    let deadline = tokio::time::Instant::now() + limit;
    let mut echoed = false;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return;
        }
        let window = if echoed {
            quiet.min(remaining)
        } else {
            remaining
        };
        match tokio::time::timeout(window, output.next_chunk()).await {
            Ok(true) => echoed = true,
            Ok(false) | Err(_) => return,
        }
    }
}

/// Whether the daemon's pipe does not exist. Only "not found" says so: a busy or guarded pipe
/// exists.
#[cfg(windows)]
fn pipe_missing(config: &RmuxConfig) -> bool {
    match config.endpoint() {
        Endpoint::WindowsPipe(pipe) => {
            std::fs::metadata(pipe).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        }
        Endpoint::UnixSocket(_) => false,
    }
}

/// Wrap a command string for the platform shell as explicit argv.
///
/// `EnsureSession::shell` would hand the string to the *user's* login shell —
/// fish or nu — where POSIX syntax is not guaranteed to parse (ADR-0012).
/// Explicit argv keeps command construction PlaneAI's decision, matching the
/// daemon's existing argv preservation.
pub fn shell_argv(command: &str) -> Vec<String> {
    if cfg!(windows) {
        vec!["cmd".to_string(), "/C".to_string(), command.to_string()]
    } else {
        vec!["/bin/sh".to_string(), "-c".to_string(), command.to_string()]
    }
}

/// Remove trailing blank lines from a pane capture.
fn trim_trailing_blank_lines(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn no_daemon_listening_is_nothing_to_connect_to_not_one_to_start() {
        let dir = tempfile::tempdir().unwrap();
        let connected = RmuxClient::connect_existing(RmuxConfig::app_private(dir.path()))
            .await
            .unwrap();
        assert!(connected.is_none());
    }

    #[test]
    fn prompts_are_submitted_with_carriage_return_not_line_feed() {
        // Raw-mode agents read Enter as a literal `\r`; a `\n` lands in the input
        // buffer without submitting, while the send still reports success.
        assert_eq!(SUBMIT_BYTE, "\r");
    }

    /// A fake pane: each entry is the delay before the next chunk. Once exhausted
    /// the pane stays silent, unless `ends` is set.
    struct Scripted {
        gaps: std::vec::IntoIter<Duration>,
        ends: bool,
    }

    fn scripted(gaps: Vec<Duration>) -> Scripted {
        Scripted {
            gaps: gaps.into_iter(),
            ends: false,
        }
    }

    impl OutputChunks for Scripted {
        async fn next_chunk(&mut self) -> bool {
            match self.gaps.next() {
                Some(gap) => {
                    tokio::time::sleep(gap).await;
                    true
                }
                None if self.ends => false,
                None => std::future::pending().await,
            }
        }
    }

    const QUIET: Duration = Duration::from_millis(200);
    const LIMIT: Duration = Duration::from_secs(3);

    #[tokio::test(start_paused = true)]
    async fn enter_waits_for_the_echo_to_go_quiet() {
        let started = tokio::time::Instant::now();
        let ms = Duration::from_millis;
        wait_for_echo_to_settle(&mut scripted(vec![ms(5), ms(50), ms(150)]), QUIET, LIMIT).await;
        // Last chunk at 205ms, then a full quiet window.
        assert_eq!(started.elapsed(), ms(405));
    }

    #[tokio::test(start_paused = true)]
    async fn silence_before_the_echo_starts_is_not_mistaken_for_ingestion() {
        // An agent collecting a paste renders nothing until the paste is complete.
        let started = tokio::time::Instant::now();
        let ms = Duration::from_millis;
        wait_for_echo_to_settle(&mut scripted(vec![ms(900)]), QUIET, LIMIT).await;
        assert_eq!(started.elapsed(), ms(1100));
    }

    #[tokio::test(start_paused = true)]
    async fn output_that_never_stops_is_bounded() {
        let started = tokio::time::Instant::now();
        let spinner = vec![Duration::from_millis(100); 100];
        wait_for_echo_to_settle(&mut scripted(spinner), QUIET, LIMIT).await;
        assert_eq!(started.elapsed(), LIMIT);
    }

    #[tokio::test(start_paused = true)]
    async fn a_pane_that_never_echoes_is_bounded() {
        let started = tokio::time::Instant::now();
        wait_for_echo_to_settle(&mut scripted(vec![]), QUIET, LIMIT).await;
        assert_eq!(started.elapsed(), LIMIT);
    }

    #[tokio::test(start_paused = true)]
    async fn an_ended_stream_stops_the_wait() {
        let started = tokio::time::Instant::now();
        let mut ended = Scripted {
            gaps: Vec::new().into_iter(),
            ends: true,
        };
        wait_for_echo_to_settle(&mut ended, QUIET, LIMIT).await;
        assert_eq!(started.elapsed(), Duration::ZERO);
    }

    #[test]
    fn commands_are_wrapped_as_explicit_argv_not_the_login_shell() {
        let argv = shell_argv("kiro-cli --resume");

        assert_eq!(argv.len(), 3);
        assert_eq!(argv[2], "kiro-cli --resume");
        #[cfg(not(windows))]
        assert_eq!(&argv[..2], &["/bin/sh".to_string(), "-c".to_string()]);
        #[cfg(windows)]
        assert_eq!(&argv[..2], &["cmd".to_string(), "/C".to_string()]);
    }

    #[test]
    fn argv_preserves_arguments_containing_spaces_and_metacharacters() {
        // The whole command stays one argv entry, so the shell it reaches applies
        // its own quoting rules exactly once.
        let argv = shell_argv("agent --prompt 'fix the bug' && echo done");
        assert_eq!(argv[2], "agent --prompt 'fix the bug' && echo done");
    }

    #[test]
    fn trailing_blank_rows_are_trimmed_so_line_counts_track_content() {
        // A capture is a fixed-height grid; keeping the padding would freeze the
        // line count and make every incremental read look like a rewrite.
        assert_eq!(trim_trailing_blank_lines("one\ntwo\n\n\n   \n"), "one\ntwo");
        assert_eq!(trim_trailing_blank_lines("only\n"), "only");
        assert_eq!(trim_trailing_blank_lines("\n\n"), "");
        assert_eq!(trim_trailing_blank_lines(""), "");
    }

    #[test]
    fn interior_blank_lines_are_preserved() {
        // Blank lines between output are real content and must not be collapsed.
        assert_eq!(trim_trailing_blank_lines("one\n\ntwo\n\n"), "one\n\ntwo");
    }

    #[test]
    fn a_resource_handle_round_trips_through_storage() {
        let handle = ResourceHandle::from_u32(17);
        assert_eq!(handle.as_u32(), 17);
    }
}
