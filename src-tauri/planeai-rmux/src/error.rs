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

    /// Writing a prompt failed, possibly after part of it reached the pane.
    ///
    /// Deliberately not a transport error: retrying could type the text twice.
    #[error("rmux prompt delivery failed and may be partially typed: {source}")]
    PromptDelivery {
        #[source]
        source: rmux_sdk::RmuxError,
    },

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

    /// Whether the session may be taken as gone: the daemon said so, or the connection to it
    /// died, which may also hide a daemon still running. Only for callers that would rather not
    /// start a daemon to find out, as killing a workspace's last session stops it mid-reply;
    /// others use [`Self::confirms_session_absent`].
    pub fn means_session_absent(&self) -> bool {
        self.is_daemon_gone() || self.confirms_session_absent()
    }

    /// Whether the daemon itself said the session is gone. A dead connection does not count: an
    /// idle one is closed under a daemon still running, so only a retry can tell.
    pub fn confirms_session_absent(&self) -> bool {
        match self {
            Self::SessionNotFound { .. } | Self::NoPane { .. } | Self::ResourceGone { .. } => true,
            Self::Sdk { source, .. } => is_session_not_found(source),
            _ => false,
        }
    }

    /// Whether the connection to the daemon was lost or refused, as when it stops; unlike
    /// [`Self::is_daemon_gone`], a timed-out request to a daemon still running does not count.
    pub fn is_connection_lost(&self) -> bool {
        let source = match self {
            Self::DaemonUnavailable { source, .. } | Self::Sdk { source, .. } => source,
            _ => return false,
        };
        matches!(
            source,
            rmux_sdk::RmuxError::Transport { source, .. } if matches!(
                source.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::NotFound
                    | std::io::ErrorKind::ConnectionRefused
            )
        )
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

fn is_session_not_found(source: &rmux_sdk::RmuxError) -> bool {
    matches!(
        source,
        rmux_sdk::RmuxError::Protocol {
            source: rmux_proto::RmuxError::SessionNotFound(_),
            ..
        }
    )
}

/// Whether an SDK failure means the pane, or the session holding it, no longer exists.
pub(crate) fn means_pane_gone(source: &rmux_sdk::RmuxError) -> bool {
    matches!(source, rmux_sdk::RmuxError::PaneNotFound { .. }) || is_session_not_found(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transport() -> rmux_sdk::RmuxError {
        rmux_sdk::RmuxError::transport("send", std::io::Error::other("closed"))
    }

    #[test]
    fn a_transport_failure_is_retried() {
        assert!(Error::sdk("send_text")(transport()).is_daemon_gone());
    }

    #[test]
    fn only_the_daemon_confirms_a_session_is_gone() {
        let gone = Error::sdk("find_panes")(rmux_sdk::RmuxError::protocol(
            rmux_proto::RmuxError::SessionNotFound("w".into()),
        ));
        assert!(gone.confirms_session_absent());
        // A closed connection may hide a daemon still running it.
        let unreachable = Error::sdk("find_panes")(transport());
        assert!(unreachable.means_session_absent() && !unreachable.confirms_session_absent());
    }

    #[test]
    fn a_pane_is_gone_only_when_the_daemon_says_so() {
        let session = rmux_proto::SessionName::new("w").unwrap();
        let pane = rmux_proto::RmuxError::pane_not_found(session, rmux_proto::PaneId::new(3));
        // Built the way `Session::pane_by_id` reports a pane that closed.
        assert!(means_pane_gone(&rmux_sdk::RmuxError::protocol(pane)));
        assert!(means_pane_gone(&rmux_sdk::RmuxError::protocol(
            rmux_proto::RmuxError::SessionNotFound("w".into())
        )));
        assert!(!means_pane_gone(&transport()));
    }

    #[test]
    fn a_timed_out_request_is_not_a_lost_connection() {
        let other = Error::sdk("close_pane")(transport());
        assert!(!other.is_connection_lost());
        let eof = Error::sdk("close_pane")(rmux_sdk::RmuxError::transport(
            "read",
            std::io::Error::from(std::io::ErrorKind::UnexpectedEof),
        ));
        assert!(eof.is_connection_lost());
        let not_connected = Error::sdk("close_pane")(rmux_sdk::RmuxError::transport(
            "write",
            std::io::Error::from(std::io::ErrorKind::NotConnected),
        ));
        assert!(not_connected.is_connection_lost());
        let slow = Error::sdk("close_pane")(rmux_sdk::RmuxError::transport(
            "read",
            std::io::Error::from(std::io::ErrorKind::TimedOut),
        ));
        assert!(slow.is_daemon_gone() && !slow.is_connection_lost());
    }

    #[test]
    fn a_failed_prompt_write_is_not_retried() {
        // The text may already be in the pane; a retry could type it a second time.
        assert!(!Error::PromptDelivery {
            source: transport()
        }
        .is_daemon_gone());
    }
}
