use std::path::Path;
use std::process::Command;

use crate::config;

/// Build a PATH string with user-local bin directories prepended.
/// Delegates to planeai_core::command::augmented_path with config dirs.
#[cfg(test)]
pub fn augmented_path(config_dirs: &[String]) -> String {
    planeai_core::command::augmented_path(config_dirs)
}

/// Resolve a command name to its full path, checking user-local bin directories
/// that may not be in PATH when launched from a GUI app.
#[cfg(not(windows))]
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

/// Resolve a command name on Windows, probing .cmd/.exe extensions and
/// common install directories that may not be in PATH for GUI apps.
#[cfg(windows)]
pub fn resolve(cmd: &str) -> String {
    // Already a full path with extension — return as-is
    if Path::new(cmd).is_absolute() && Path::new(cmd).exists() {
        return cmd.to_string();
    }

    let extensions = [".cmd", ".exe", ".bat"];
    let home = config::home_dir();
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| format!("{home}\\AppData\\Roaming"));
    let localappdata =
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| format!("{home}\\AppData\\Local"));

    let extra_dirs = [
        format!("{appdata}\\npm"),
        format!("{home}\\.cargo\\bin"),
        format!("{localappdata}\\Programs"),
    ];

    for dir in &extra_dirs {
        // Check with extensions first (copilot.cmd, claude.exe, etc.)
        for ext in &extensions {
            let full = format!("{dir}\\{cmd}{ext}");
            if Path::new(&full).exists() {
                return full;
            }
        }
        // Check without extension (already has one, or is a native .exe)
        let full = format!("{dir}\\{cmd}");
        if Path::new(&full).exists() {
            return full;
        }
    }

    // Fallback: use where.exe to search PATH
    if let Ok(output) = {
        let mut where_cmd = Command::new("where.exe");
        where_cmd.arg(cmd);
        planeai_core::command::no_window(&mut where_cmd);
        where_cmd.output()
    } {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !path.is_empty() {
                return path;
            }
        }
    }

    cmd.to_string()
}

/// When WSL is the target environment, skip Windows-side resolution. The command
/// will run inside the WSL distro where its own PATH handles resolution naturally.
/// Returns the command name unchanged.
#[allow(dead_code)]
pub fn resolve_for_wsl(cmd: &str) -> String {
    cmd.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn augmented_path_includes_conventional_dirs() {
        let path = augmented_path(&[]);
        let home = config::home_dir();
        if cfg!(windows) {
            assert!(
                path.contains(&format!("{home}\\.cargo\\bin")),
                "PATH should include .cargo\\bin, got: {path}"
            );
        } else {
            assert!(
                path.contains(&format!("{home}/.local/bin")),
                "PATH should include .local/bin, got: {path}"
            );
        }
    }

    #[test]
    fn augmented_path_uses_platform_separator() {
        let path = augmented_path(&[]);
        let sep = if cfg!(windows) { ";" } else { ":" };
        assert!(path.contains(sep), "PATH should use platform separator");
    }
}
