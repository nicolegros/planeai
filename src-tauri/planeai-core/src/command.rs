use std::path::Path;
use std::process::Command;

#[derive(Debug)]
pub enum CommandError {
    SpawnFailed { command: String, source: String },
    NonZeroExit { status: i32, stderr: String },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailed { command, source } => {
                write!(f, "failed to spawn '{command}': {source}")
            }
            Self::NonZeroExit { status, stderr } => {
                write!(f, "command exited with {status}: {stderr}")
            }
        }
    }
}

/// Return the shell program and args needed to execute a command string.
/// On Unix: `("/bin/sh", ["-c", cmd])`. On Windows: `("cmd", ["/C", cmd])`.
#[cfg(not(windows))]
pub fn shell_args(cmd: &str) -> (&'static str, Vec<String>) {
    ("/bin/sh", vec!["-c".to_string(), cmd.to_string()])
}

#[cfg(windows)]
pub fn shell_args(cmd: &str) -> (&'static str, Vec<String>) {
    ("cmd", vec!["/C".to_string(), cmd.to_string()])
}

/// Run a shell command string via `sh -c` (Unix) or `cmd /C` (Windows).
/// Returns stdout on success.
pub fn run_command(cmd_str: &str, cwd: &Path) -> Result<String, CommandError> {
    if cmd_str.trim().is_empty() {
        return Err(CommandError::SpawnFailed {
            command: cmd_str.to_string(),
            source: "empty command".to_string(),
        });
    }

    let output = shell_command(cmd_str)
        .current_dir(cwd)
        .output()
        .map_err(|e| CommandError::SpawnFailed {
            command: cmd_str.to_string(),
            source: e.to_string(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(CommandError::NonZeroExit {
            status: output.status.code().unwrap_or(-1),
            stderr,
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(not(windows))]
fn shell_command(cmd_str: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", cmd_str]);
    cmd
}

#[cfg(windows)]
fn shell_command(cmd_str: &str) -> Command {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", cmd_str]);
    cmd
}
