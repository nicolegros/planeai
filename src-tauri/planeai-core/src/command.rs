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

/// Build a PATH string suitable for spawning user CLI tools from a GUI app.
///
/// GUI apps on macOS/Linux inherit a minimal PATH that excludes user-local
/// directories like ~/.cargo/bin. This function prepends conventional developer
/// directories and any user-configured extra dirs to the inherited PATH.
///
/// Priority (highest to lowest):
/// 1. `PLANEAI_EXTRA_PATH` env var (colon-separated dirs; overrides config)
/// 2. `config_dirs` (from config file's `extra_path_dirs`)
/// 3. Conventional developer directories
/// 4. Inherited system PATH
pub fn augmented_path(config_dirs: &[String]) -> String {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();

    let sep = if cfg!(windows) { ";" } else { ":" };
    let system_path = std::env::var("PATH").unwrap_or_default();

    let extra_from_env = std::env::var("PLANEAI_EXTRA_PATH").ok();
    let user_dirs: Vec<&str> = match &extra_from_env {
        Some(val) => val.split(sep).filter(|s| !s.is_empty()).collect(),
        None => config_dirs.iter().map(|s| s.as_str()).collect(),
    };

    let conventional = conventional_dirs(&home);

    let mut parts: Vec<&str> = Vec::new();
    parts.extend(user_dirs);
    parts.extend(conventional.iter().map(|s| s.as_str()));
    parts.push(&system_path);
    parts.join(sep)
}

#[cfg(not(windows))]
fn conventional_dirs(home: &str) -> Vec<String> {
    vec![
        format!("{home}/.local/bin"),
        format!("{home}/.cargo/bin"),
        format!("{home}/go/bin"),
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
    ]
}

#[cfg(windows)]
fn conventional_dirs(home: &str) -> Vec<String> {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| format!("{home}\\AppData\\Roaming"));
    let localappdata =
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| format!("{home}\\AppData\\Local"));
    vec![
        format!("{appdata}\\npm"),
        format!("{home}\\.cargo\\bin"),
        format!("{localappdata}\\Programs"),
    ]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes tests that read/write PLANEAI_EXTRA_PATH env var.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn no_guardrails_path_in_conventional_dirs() {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default();
        let dirs = conventional_dirs(&home);
        for dir in &dirs {
            assert!(
                !dir.contains("guardrails"),
                "conventional dirs should not include guardrails: {dir}"
            );
        }
    }

    #[test]
    fn inherited_path_is_preserved() {
        let system_path = std::env::var("PATH").unwrap_or_default();
        let path = augmented_path(&[]);
        assert!(
            path.contains(&system_path),
            "inherited PATH must be preserved"
        );
    }

    #[test]
    fn conventional_dirs_added() {
        let path = augmented_path(&[]);
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default();
        let marker = if cfg!(windows) {
            format!("{home}\\.cargo\\bin")
        } else {
            format!("{home}/.cargo/bin")
        };
        assert!(
            path.contains(&marker),
            "conventional dir should appear in PATH"
        );
    }

    #[test]
    fn config_provided_extra_dirs_are_added() {
        let _lock = ENV_MUTEX.lock().unwrap();
        if std::env::var("PLANEAI_EXTRA_PATH").is_ok() {
            return;
        }
        let path = augmented_path(&["/custom/shims".to_string()]);
        assert!(
            path.contains("/custom/shims"),
            "config dirs should be in PATH: {path}"
        );
    }

    #[test]
    fn config_dirs_come_before_conventional() {
        let _lock = ENV_MUTEX.lock().unwrap();
        if std::env::var("PLANEAI_EXTRA_PATH").is_ok() {
            return;
        }
        let path = augmented_path(&["/my/custom/bin".to_string()]);
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default();
        let custom_pos = path.find("/my/custom/bin").unwrap();
        let conventional = if cfg!(windows) {
            format!("{home}\\.cargo\\bin")
        } else {
            format!("{home}/.cargo/bin")
        };
        let conv_pos = path.find(&conventional).unwrap();
        assert!(
            custom_pos < conv_pos,
            "config dirs should come before conventional dirs"
        );
    }

    #[test]
    fn env_overrides_config() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let marker = "/planeai_test_env_override_8f3a";
        unsafe {
            std::env::set_var("PLANEAI_EXTRA_PATH", marker);
        }
        let path = augmented_path(&["/config/bin".to_string()]);
        unsafe {
            std::env::remove_var("PLANEAI_EXTRA_PATH");
        }
        assert!(path.contains(marker), "env should be in PATH: {path}");
        assert!(
            !path.contains("/config/bin"),
            "config dirs should NOT be in PATH when env overrides: {path}"
        );
    }

    #[test]
    fn system_path_appended_at_end() {
        let system_path = std::env::var("PATH").unwrap_or_default();
        let path = augmented_path(&[]);
        assert!(
            path.ends_with(&system_path),
            "system PATH should be at the end"
        );
    }
}
