//! Unix-specific PTY command building.

use portable_pty::CommandBuilder;
use std::path::Path;

/// Build a CommandBuilder that wraps a command string in a shell.
pub fn build_command(cmd_str: &str) -> CommandBuilder {
    let mut c = CommandBuilder::new("bash");
    c.args(["-c", cmd_str]);
    c
}

/// Platform-appropriate default shell fallback.
pub fn default_shell() -> String {
    if cfg!(target_os = "macos") && Path::new("/bin/zsh").exists() {
        return "/bin/zsh".to_string();
    }
    "/bin/sh".to_string()
}
