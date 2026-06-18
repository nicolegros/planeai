//! Default shell resolution shared across all PTY session sources.

use std::path::Path;

/// Resolve the user's default shell.
///
/// Priority:
/// 1. Explicit shell parameter (if Some)
/// 2. `$SHELL` env var (if set and file exists)
/// 3. Platform fallback: `/bin/zsh` on macOS, `/bin/sh` on Linux
pub fn resolve_default_shell(explicit: Option<&str>) -> String {
    resolve_shell_inner(explicit, std::env::var("SHELL").ok().as_deref())
}

fn resolve_shell_inner(explicit: Option<&str>, shell_env: Option<&str>) -> String {
    if let Some(s) = explicit {
        return s.to_string();
    }
    if let Some(s) = shell_env {
        if Path::new(s).exists() {
            return s.to_string();
        }
    }
    platform_fallback()
}

fn platform_fallback() -> String {
    if cfg!(target_os = "macos") && Path::new("/bin/zsh").exists() {
        return "/bin/zsh".to_string();
    }
    "/bin/sh".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_shell_wins() {
        assert_eq!(
            resolve_shell_inner(Some("/usr/bin/fish"), Some("/bin/zsh")),
            "/usr/bin/fish"
        );
    }

    #[test]
    fn shell_env_used_when_valid() {
        assert_eq!(resolve_shell_inner(None, Some("/bin/sh")), "/bin/sh");
    }

    #[test]
    fn invalid_shell_env_falls_back() {
        let result = resolve_shell_inner(None, Some("/nonexistent/path"));
        assert_ne!(result, "/nonexistent/path");
        assert!(result == "/bin/zsh" || result == "/bin/sh");
    }

    #[test]
    fn no_shell_env_falls_back() {
        let result = resolve_shell_inner(None, None);
        assert!(result == "/bin/zsh" || result == "/bin/sh");
    }

    #[test]
    fn never_returns_bash_as_fallback() {
        let result = resolve_shell_inner(None, Some("/nonexistent"));
        assert_ne!(result, "bash");
        assert_ne!(result, "/bin/bash");
    }
}
