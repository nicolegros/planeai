//! tmux operations routed through WSL.
//!
//! Provides the same interface as `tmux.rs` but executes tmux commands inside
//! a WSL distro via `wsl.exe`. Used on Windows when WSL is configured and tmux
//! is installed inside the distro.
//!
//! This module is compiled on all platforms but only meaningful on Windows.
//! On non-Windows, callers should use the native `tmux` module instead.

use std::process::Command;

use planeai_core::command::no_window;

/// Build a tmux Command that runs through WSL.
///
/// Returns: `wsl.exe -d <distro> -- tmux <args...>`
fn tmux_cmd(distro: &str) -> Command {
    let mut cmd = Command::new("wsl");
    cmd.args(["-d", distro, "--", "tmux"]);
    no_window(&mut cmd);
    cmd
}

/// Check if a tmux session exists inside WSL.
pub fn has_session(distro: &str, tmux_name: &str) -> bool {
    let target = format!("={}", tmux_name);
    tmux_cmd(distro)
        .args(["has-session", "-t", &target])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Kill a tmux session inside WSL.
pub fn kill_session(distro: &str, tmux_name: &str) -> Result<(), String> {
    let target = format!("={}", tmux_name);
    let output = tmux_cmd(distro)
        .args(["kill-session", "-t", &target])
        .output()
        .map_err(|e| format!("failed to run tmux via WSL: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("no such session") && !stderr.contains("can't find session") {
            return Err(stderr.to_string());
        }
    }
    Ok(())
}

/// Create a tmux session inside WSL with a custom command and environment.
///
/// Runs: `wsl -d <distro> --cd <cwd> -- tmux new-session -d -s <name> -e ... <cmd>`
pub fn create_session_with_cmd_and_path(
    distro: &str,
    tmux_name: &str,
    working_dir: &str,
    cmd: &str,
    session_id: &str,
    extra_path_dirs: &[String],
) -> Result<(), String> {
    let env_flag = format!("PLANEAI_SESSION_ID={}", session_id);
    let path_flag = format!(
        "PATH={}",
        planeai_core::command::augmented_path(extra_path_dirs)
    );

    // Use --cd to set working directory inside WSL, then run tmux new-session
    let mut wsl_cmd = Command::new("wsl");
    wsl_cmd.args(["-d", distro, "--cd", working_dir, "--", "tmux"]);
    wsl_cmd.args([
        "new-session",
        "-d",
        "-s",
        tmux_name,
        "-e",
        &env_flag,
        "-e",
        &path_flag,
        cmd,
    ]);
    no_window(&mut wsl_cmd);

    let output = wsl_cmd
        .output()
        .map_err(|e| format!("failed to run tmux via WSL: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    Ok(())
}

/// Send literal text to a tmux session inside WSL, followed by Enter.
pub fn send_keys(distro: &str, tmux_name: &str, text: &str) -> Result<(), String> {
    let output = tmux_cmd(distro)
        .args(["send-keys", "-t", tmux_name, "-l", text])
        .output()
        .map_err(|e| format!("failed to run tmux via WSL: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let output = tmux_cmd(distro)
        .args(["send-keys", "-t", tmux_name, "Enter"])
        .output()
        .map_err(|e| format!("failed to run tmux via WSL: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_cmd_builds_correct_command() {
        // Smoke test — just verifies it doesn't panic
        let _cmd = tmux_cmd("Ubuntu");
    }

    #[test]
    fn has_session_returns_false_for_nonexistent() {
        // On non-Windows (or no WSL), this should just return false
        let result = has_session("NonexistentDistro", "nonexistent-session");
        assert!(!result);
    }
}
