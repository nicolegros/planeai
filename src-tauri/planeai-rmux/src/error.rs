//! Typed errors for the rmux backend.

/// Failures surfaced by this crate. The Tauri layer converts these to strings at
/// the command boundary; inside the crate they stay typed so callers can react to
/// the cases that matter — notably a daemon that has shut down.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The session name derived from a PlaneAI session id was rejected by rmux.
    #[error("invalid rmux session name for {session_id}: {reason}")]
    SessionName { session_id: String, reason: String },

    /// The daemon could not be reached or started.
    #[error("rmux daemon unavailable at {endpoint}: {source}")]
    DaemonUnavailable {
        endpoint: String,
        #[source]
        source: rmux_sdk::RmuxError,
    },

    /// A session PlaneAI expected to exist is not known to the daemon.
    #[error("rmux session not found: {session_id}")]
    SessionNotFound { session_id: String },

    /// The session exists but has no pane to attach to.
    #[error("rmux session {session_id} has no pane")]
    NoPane { session_id: String },

    /// The pane PlaneAI recorded for a resource is no longer on the daemon.
    ///
    /// Pane ids are renewable handles, not durable identity: a daemon restart
    /// invalidates every one of them, so this is an expected state that means
    /// "respawn", not a fault.
    #[error("rmux pane %{pane_id} is gone")]
    ResourceGone { pane_id: u32 },

    /// Any other SDK failure, carrying the operation for context.
    #[error("rmux {operation} failed: {source}")]
    Sdk {
        operation: &'static str,
        #[source]
        source: rmux_sdk::RmuxError,
    },
}

impl Error {
    pub(crate) fn sdk(operation: &'static str) -> impl FnOnce(rmux_sdk::RmuxError) -> Self {
        move |source| Self::Sdk { operation, source }
    }

    /// Whether the failure means the session no longer exists.
    ///
    /// Both a daemon that has exited and a session the daemon has forgotten leave
    /// nothing to act on, so callers that were removing the session have already
    /// achieved their goal.
    pub fn means_session_absent(&self) -> bool {
        if self.is_daemon_gone() {
            return true;
        }
        match self {
            Self::SessionNotFound { .. } | Self::NoPane { .. } | Self::ResourceGone { .. } => true,
            Self::Sdk { source, .. } => matches!(
                source,
                rmux_sdk::RmuxError::Protocol {
                    source: rmux_proto::RmuxError::SessionNotFound(_),
                    ..
                }
            ),
            _ => false,
        }
    }

    /// Whether the failure means the daemon is gone and a retry should restart it.
    ///
    /// Killing the last rmux session terminates the daemon and removes its
    /// socket, so a closed transport is an expected, recoverable condition
    /// rather than a fault.
    pub fn is_daemon_gone(&self) -> bool {
        let source = match self {
            Self::DaemonUnavailable { .. } => return true,
            Self::Sdk { source, .. } => source,
            _ => return false,
        };
        matches!(source, rmux_sdk::RmuxError::Transport { .. })
    }
}

pub type Result<T> = std::result::Result<T, Error>;
