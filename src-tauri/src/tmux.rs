use std::process::Command;

fn tmux_bin() -> &'static str {
    static BIN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BIN.get_or_init(|| {
        // Check common Homebrew paths first, then fall back to bare name
        for path in ["/opt/homebrew/bin/tmux", "/usr/local/bin/tmux"] {
            if std::path::Path::new(path).exists() {
                return path.to_string();
            }
        }
        "tmux".to_string()
    })
}

/// List local branches for a git repo at the given path.
pub fn list_branches(repo_path: &str) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args(["branch", "--list", "--format=%(refname:short)"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let branches = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Ok(branches)
}

/// Checkout an existing branch or create a new one.
pub fn checkout_branch(repo_path: &str, branch: &str, create: bool) -> Result<(), String> {
    let args = if create {
        vec!["checkout", "-b", branch]
    } else {
        vec!["checkout", branch]
    };

    let output = Command::new("git")
        .args(&args)
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

/// Create a git worktree with a new branch off a base branch.
pub fn worktree_add(repo_path: &str, worktree_path: &str, new_branch: &str, base_branch: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["worktree", "add", "-b", new_branch, worktree_path, base_branch])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

/// Remove a git worktree forcefully.
pub fn worktree_remove(repo_path: &str, worktree_path: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["worktree", "remove", "--force", worktree_path])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("is not a working tree") {
            return Err(stderr.to_string());
        }
    }
    Ok(())
}

/// Generate a tmux session name: planeai-<project>-<8hex>
pub fn session_name(project_name: &str) -> String {
    let hex: String = uuid::Uuid::new_v4().to_string().replace('-', "")[..8].to_string();
    let sanitized = project_name.replace(' ', "-");
    format!("planeai-{}-{}", sanitized, hex)
}

/// Build the tmux command args to create a new session running kiro-cli.
pub fn build_new_session_args(tmux_name: &str, working_dir: &str, auto_approve: bool) -> Vec<String> {
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
    Command::new(tmux_bin())
        .args(["has-session", "-t", tmux_name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Kill a tmux session.
pub fn kill_session(tmux_name: &str) -> Result<(), String> {
    let output = Command::new(tmux_bin())
        .args(["kill-session", "-t", tmux_name])
        .output()
        .map_err(|e| format!("failed to run tmux: {e}"))?;

    if !output.status.success() {
        // Ignore "no such session" errors (already dead)
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("no such session") {
            return Err(stderr.to_string());
        }
    }
    Ok(())
}

/// Create a tmux session with remain-on-exit and launch kiro-cli.
pub fn create_session(tmux_name: &str, working_dir: &str, auto_approve: bool) -> Result<(), String> {
    let args = build_new_session_args(tmux_name, working_dir, auto_approve);
    let output = Command::new(tmux_bin())
        .args(&args)
        .output()
        .map_err(|e| format!("failed to run tmux: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    // Set remain-on-exit so scrollback is preserved after kiro exits
    let _ = Command::new(tmux_bin())
        .args(["set-option", "-t", tmux_name, "remain-on-exit", "on"])
        .output();

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
}
