//! Client for PlaneAI's private rmux daemon.
//!
//! One rmux session hosts a whole task workspace; each PlaneAI terminal resource
//! is one **window** inside it holding a single pane. The window matters: probe 2
//! in ADR-0012 showed `Pane::resize` is a no-op for a sole pane, so geometry
//! changes must address the window, and PlaneAI tabs are independently sized
//! full terminals rather than simultaneously visible splits.

use std::collections::HashSet;

use rmux_sdk::{
    EnsureSession, Pane, PaneId, PaneOutputStart, PaneOutputStream, Rmux, Session, SessionName,
    TerminalSizeSpec, Window,
};

use crate::config::{Endpoint, RmuxConfig};
use crate::error::{Error, Result};
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
    stream: PaneOutputStream,
}

impl AttachedPane {
    pub fn into_parts(self) -> (Pane, Window, PaneOutputStream) {
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
            // Prefer the bundled sidecar over anything on PATH.
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

    /// Create a terminal resource, materialising its workspace if needed.
    ///
    /// No `OwnedSession` lease is taken: probe 1 (ADR-0012) showed sessions made
    /// this way already outlive the client that created them, which is the
    /// persistence this backend exists for. A lease would only add a way to kill
    /// them by accident.
    pub async fn spawn_resource(&self, spawn: &ResourceSpawn) -> Result<ResourceHandle> {
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
                    .size(size),
            )
            .await
            .map_err(Error::sdk("ensure_session"))?;

        // The session's initial window already runs this resource's command when
        // the session was created for it, so adopt that window instead of
        // spawning a second copy.
        if session.was_created() {
            let adopted = self.first_pane_id(&name).await?;
            self.rename_window_for(&session, adopted, &spawn.pty_key)
                .await;
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
        // Start at the oldest retained output so a terminal attaching after the
        // agent has already produced output still renders it — the daemon backend
        // replays its ring buffer for the same reason.
        let stream = pane
            .output_stream_starting_at(PaneOutputStart::Oldest)
            .await
            .map_err(Error::sdk("output_stream"))?;

        Ok(AttachedPane {
            pane,
            window,
            stream,
        })
    }

    /// Write literal text to a resource's pane.
    ///
    /// Uses the `send-keys -l` path, so bytes reach the tty unmodified. Keeping
    /// this in the crate means callers never handle raw SDK handles just to send a
    /// prompt.
    pub async fn send_text(
        &self,
        workspace: &WorkspaceName,
        handle: ResourceHandle,
        text: &str,
    ) -> Result<()> {
        let name = self.workspace_name(workspace)?;
        let session = self.session(&name).await?;
        let pane = session
            .pane_by_id(handle.pane_id)
            .await
            .map_err(Error::sdk("pane_by_id"))?;
        pane.send_text(text).await.map_err(Error::sdk("send_text"))
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
        let name = self.workspace_name(workspace)?;
        let session = self.session(&name).await?;
        let pane = session
            .pane_by_id(handle.pane_id)
            .await
            .map_err(Error::sdk("pane_by_id"))?;
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

    /// Close one resource, leaving the workspace and its siblings running.
    ///
    /// Closing the workspace's last window would take the session with it, so
    /// removing an agent must not be confused with removing the workspace.
    pub async fn close_resource(
        &self,
        workspace: &WorkspaceName,
        handle: ResourceHandle,
    ) -> Result<()> {
        let name = self.workspace_name(workspace)?;
        let session = match self.session(&name).await {
            Ok(session) => session,
            Err(error) if error.means_session_absent() => return Ok(()),
            Err(error) => return Err(error),
        };
        let pane = match session.pane_by_id(handle.pane_id).await {
            Ok(pane) => pane,
            // Already gone is the desired end state.
            Err(_) => return Ok(()),
        };
        match pane.close().await {
            Ok(_) => Ok(()),
            Err(source) => {
                let error = Error::Sdk {
                    operation: "close_pane",
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

    /// Whether a workspace currently exists on the daemon.
    ///
    /// A daemon that has exited hosts nothing, so an unreachable daemon answers
    /// "no" rather than failing: removing the last workspace stops the daemon, and
    /// callers asking this question want the state, not the transport detail.
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

    /// Give the adopted initial window the resource's name, best effort: the name
    /// is for human inspection, so failing to set it must not fail the spawn.
    async fn rename_window_for(&self, session: &Session, pane_id: PaneId, pty_key: &str) {
        let Ok(panes) = self
            .rmux
            .find_panes()
            .session(session.name().as_ref())
            .all()
            .await
        else {
            return;
        };
        let Some(found) = panes.iter().find(|pane| pane.pane_id == pane_id) else {
            return;
        };
        if let Err(error) = session.window(found.window_index).rename(pty_key).await {
            tracing::debug!(%error, pty_key, "could not name rmux window");
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
