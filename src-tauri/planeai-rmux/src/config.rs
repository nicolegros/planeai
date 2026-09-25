//! Endpoint and daemon-binary configuration for PlaneAI's private rmux daemon.
//!
//! PlaneAI never attaches to the user's own rmux server (ADR-0012): it runs its
//! own daemon on an app-private endpoint so session names cannot collide, the
//! user's `rmux.conf`/`tmux.conf` is not inherited, and cleanup can never touch
//! work PlaneAI does not own.

use std::path::{Path, PathBuf};

/// Environment variable the SDK honours to locate the daemon executable.
///
/// rmux ships `rmux-daemon` separately from the `rmux` CLI, and that is what
/// `connect_or_start` spawns. Pointing this at a bundled sidecar avoids relying
/// on the user's PATH entirely.
pub const DAEMON_BINARY_ENV: &str = "RMUX_SDK_DAEMON_BINARY";

/// Overrides the endpoint for development and tests.
pub const ENDPOINT_ENV: &str = "PLANEAI_RMUX_ENDPOINT";

/// Socket / pipe name used for PlaneAI's private daemon.
const ENDPOINT_NAME: &str = "rmux.sock";

#[cfg(windows)]
const WINDOWS_PIPE: &str = r"\\.\pipe\planeai-rmux";

/// Where PlaneAI's private rmux daemon listens, and which binary provides it.
#[derive(Debug, Clone)]
pub struct RmuxConfig {
    endpoint: Endpoint,
    daemon_binary: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    UnixSocket(PathBuf),
    WindowsPipe(String),
}

impl RmuxConfig {
    /// Resolve the app-private endpoint, honouring [`ENDPOINT_ENV`].
    ///
    /// The socket lives beside the existing daemon socket so both backends share
    /// one runtime directory and its permissions.
    pub fn app_private(runtime_dir: &Path) -> Self {
        Self {
            endpoint: Self::resolve_endpoint(runtime_dir),
            daemon_binary: None,
        }
    }

    /// Point the SDK at a bundled `rmux-daemon` sidecar instead of PATH lookup.
    pub fn with_daemon_binary(mut self, path: impl Into<PathBuf>) -> Self {
        self.daemon_binary = Some(path.into());
        self
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub fn daemon_binary(&self) -> Option<&Path> {
        self.daemon_binary.as_deref()
    }

    /// A stable label for logs and error messages.
    pub fn label(&self) -> String {
        match &self.endpoint {
            Endpoint::UnixSocket(path) => path.display().to_string(),
            Endpoint::WindowsPipe(pipe) => pipe.clone(),
        }
    }

    /// The directory that must exist before a Unix socket can be bound.
    pub fn socket_parent(&self) -> Option<&Path> {
        match &self.endpoint {
            Endpoint::UnixSocket(path) => path.parent(),
            Endpoint::WindowsPipe(_) => None,
        }
    }

    fn resolve_endpoint(runtime_dir: &Path) -> Endpoint {
        if let Some(override_value) = std::env::var_os(ENDPOINT_ENV) {
            let value = PathBuf::from(override_value);
            #[cfg(windows)]
            if value.to_string_lossy().starts_with(r"\\") {
                return Endpoint::WindowsPipe(value.to_string_lossy().into_owned());
            }
            return Endpoint::UnixSocket(value);
        }

        #[cfg(windows)]
        {
            let _ = runtime_dir;
            Endpoint::WindowsPipe(WINDOWS_PIPE.to_string())
        }
        #[cfg(not(windows))]
        {
            Endpoint::UnixSocket(runtime_dir.join(ENDPOINT_NAME))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_is_placed_in_the_runtime_directory() {
        let config = RmuxConfig::app_private(Path::new("/tmp/planeai-42"));

        #[cfg(not(windows))]
        assert_eq!(
            config.endpoint(),
            &Endpoint::UnixSocket(PathBuf::from("/tmp/planeai-42/rmux.sock"))
        );
        #[cfg(not(windows))]
        assert_eq!(config.socket_parent(), Some(Path::new("/tmp/planeai-42")));
    }

    #[test]
    fn endpoint_is_distinct_from_the_existing_daemon_socket() {
        let config = RmuxConfig::app_private(Path::new("/tmp/planeai-42"));

        // Sharing a socket with planeai-daemon would make the two backends fight
        // over the same path.
        assert_ne!(config.label(), "/tmp/planeai-42/daemon.sock");
    }

    #[test]
    fn daemon_binary_is_absent_until_a_sidecar_is_supplied() {
        let config = RmuxConfig::app_private(Path::new("/tmp/planeai-42"));
        assert_eq!(config.daemon_binary(), None);

        let config = config.with_daemon_binary("/Applications/planeai.app/rmux-daemon");
        assert_eq!(
            config.daemon_binary(),
            Some(Path::new("/Applications/planeai.app/rmux-daemon"))
        );
    }

    #[test]
    fn label_is_human_readable_for_logs() {
        let config = RmuxConfig::app_private(Path::new("/tmp/planeai-42"));
        assert!(!config.label().is_empty());
    }
}
