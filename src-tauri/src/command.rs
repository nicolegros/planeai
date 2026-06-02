use std::path::Path;
use std::process::Command;

use crate::config;

/// Resolve a command name to its full path, checking user-local bin directories
/// that may not be in PATH when launched from a GUI app.
pub fn resolve(cmd: &str) -> String {
    if cmd.starts_with('/') {
        return cmd.to_string();
    }
    let home = config::home_dir();
    let extra_dirs = [
        format!("{home}/.local/bin"),
        format!("{home}/.cargo/bin"),
        format!("{home}/go/bin"),
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
    ];
    for dir in &extra_dirs {
        let full = format!("{dir}/{cmd}");
        if Path::new(&full).exists() {
            return full;
        }
    }
    if let Ok(output) = Command::new("/bin/bash")
        .args(["-lc", &format!("which {cmd}")])
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return path;
            }
        }
    }
    cmd.to_string()
}
