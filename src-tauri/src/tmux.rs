use std::process::Command;

pub fn tmux_bin() -> &'static str {
    static BIN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BIN.get_or_init(|| crate::command::resolve("tmux"))
}

/// Generate a tmux session name: planeai-<project>-<8hex>
pub fn session_name(project_name: &str) -> String {
    let hex: String = uuid::Uuid::new_v4().to_string().replace('-', "")[..8].to_string();
    let sanitized = project_name.replace(' ', "-").replace(['.', ':'], "");
    format!("planeai-{}-{}", sanitized, hex)
}

/// Build the tmux command args to create a new session running kiro-cli.
#[allow(dead_code)]
pub fn build_new_session_args(
    tmux_name: &str,
    working_dir: &str,
    auto_approve: bool,
) -> Vec<String> {
    let cmd = if auto_approve {
        "kiro-cli chat --trust-all-tools".to_string()
    } else {
        "kiro-cli chat".to_string()
    };
    vec![
        "new-session".to_string(),
        "-d".to_string(),
        "-s".to_string(),
        tmux_name.to_string(),
        "-c".to_string(),
        working_dir.to_string(),
        cmd,
    ]
}

/// Check if a tmux session exists.
pub fn has_session(tmux_name: &str) -> bool {
    // Use '=' prefix for exact match to avoid tmux interpreting dots as separators
    let target = format!("={}", tmux_name);
    Command::new(tmux_bin())
        .args(["has-session", "-t", &target])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Kill a tmux session.
pub fn kill_session(tmux_name: &str) -> Result<(), String> {
    let target = format!("={}", tmux_name);
    let output = Command::new(tmux_bin())
        .args(["kill-session", "-t", &target])
        .output()
        .map_err(|e| format!("failed to run tmux: {e}"))?;

    if !output.status.success() {
        // Ignore "no such session" / "can't find session" errors (already dead)
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("no such session") && !stderr.contains("can't find session") {
            return Err(stderr.to_string());
        }
    }
    Ok(())
}

/// Create a tmux session with remain-on-exit and launch kiro-cli.
#[allow(dead_code)]
pub fn create_session(
    tmux_name: &str,
    working_dir: &str,
    auto_approve: bool,
    session_id: &str,
) -> Result<(), String> {
    let cmd = if auto_approve {
        "kiro-cli chat --trust-all-tools".to_string()
    } else {
        "kiro-cli chat".to_string()
    };
    create_session_with_cmd(tmux_name, working_dir, &cmd, session_id)
}

/// Create a tmux session with a custom command.
pub fn create_session_with_cmd(
    tmux_name: &str,
    working_dir: &str,
    cmd: &str,
    session_id: &str,
) -> Result<(), String> {
    create_session_with_cmd_and_path(tmux_name, working_dir, cmd, session_id, &[])
}

/// Create a tmux session with a custom command and extra PATH directories.
pub fn create_session_with_cmd_and_path(
    tmux_name: &str,
    working_dir: &str,
    cmd: &str,
    session_id: &str,
    extra_path_dirs: &[String],
) -> Result<(), String> {
    let env_flag = format!("PLANEAI_SESSION_ID={}", session_id);
    let path_flag = format!("PATH={}", planeai_core::command::augmented_path(extra_path_dirs));
    let args = vec![
        "new-session",
        "-d",
        "-s",
        tmux_name,
        "-c",
        working_dir,
        "-e",
        &env_flag,
        "-e",
        &path_flag,
        cmd,
    ];
    let output = Command::new(tmux_bin())
        .args(&args)
        .output()
        .map_err(|e| format!("failed to run tmux: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    Ok(())
}

/// Send literal text to a tmux session, followed by Enter.
pub fn send_keys(tmux_name: &str, text: &str) -> Result<(), String> {
    // Send literal text (no key-name interpretation)
    let output = Command::new(tmux_bin())
        .args(["send-keys", "-t", tmux_name, "-l", text])
        .output()
        .map_err(|e| format!("failed to run tmux: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    // Send Enter separately
    let output = Command::new(tmux_bin())
        .args(["send-keys", "-t", tmux_name, "Enter"])
        .output()
        .map_err(|e| format!("failed to run tmux: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_name_format() {
        let name = session_name("myapp");
        assert!(name.starts_with("planeai-myapp-"));
        assert_eq!(name.len(), "planeai-myapp-".len() + 8);
    }

    #[test]
    fn test_build_new_session_args() {
        let args = build_new_session_args("planeai-myapp-abc12345", "/tmp/myapp", true);
        assert_eq!(args[0], "new-session");
        assert_eq!(args[1], "-d");
        assert_eq!(args[2], "-s");
        assert_eq!(args[3], "planeai-myapp-abc12345");
        assert_eq!(args[4], "-c");
        assert_eq!(args[5], "/tmp/myapp");
        assert_eq!(args[6], "kiro-cli chat --trust-all-tools");

        let args_no = build_new_session_args("planeai-myapp-abc12345", "/tmp/myapp", false);
        assert_eq!(args_no[6], "kiro-cli chat");
    }

    #[test]
    fn tmux_bin_resolves_via_command_resolve() {
        let bin = tmux_bin();
        let resolved = crate::command::resolve("tmux");
        // tmux_bin() should return the same result as command::resolve("tmux")
        assert_eq!(bin, resolved);
    }

    #[test]
    fn tmux_bin_returns_absolute_path_when_installed() {
        let bin = tmux_bin();
        // If tmux is installed anywhere on this system, resolve must return an absolute path.
        // A bare "tmux" means it wasn't found — acceptable in CI without tmux.
        if bin != "tmux" {
            assert!(bin.starts_with('/'), "expected absolute path, got: {bin}");
            assert!(
                std::path::Path::new(bin).exists(),
                "resolved path does not exist: {bin}"
            );
        }
    }
}
