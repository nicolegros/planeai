//! Shared worktree cleanup logic — usable by both Tauri and Iced.
//!
//! The production `cleanup.rs` in the Tauri crate wires these to real git/fs ops.
//! Iced calls the same logic with the same ops.

use std::path::Path;

/// Clean up a worktree and its branch. Errors are collected, not fatal.
/// Steps: git worktree remove → fs remove_dir_all → git branch -D.
pub fn cleanup_worktree(
    project_path: &str,
    worktree_path: &str,
    branch: Option<&str>,
) -> Vec<String> {
    let mut errors = vec![];

    // Step 1: git worktree remove --force
    if let Err(e) = crate::git::worktree_remove(project_path, worktree_path) {
        errors.push(format!("worktree remove: {e}"));
    }

    // Step 2: fs fallback remove
    if Path::new(worktree_path).exists() {
        if let Err(e) = std::fs::remove_dir_all(worktree_path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                errors.push(format!("remove dir: {e}"));
            }
        }
    }

    // Step 3: delete the branch
    if let Some(branch) = branch {
        if !branch.is_empty() {
            if let Err(e) = delete_branch(project_path, branch) {
                errors.push(format!("branch delete: {e}"));
            }
        }
    }

    errors
}

fn delete_branch(repo_path: &str, branch: &str) -> Result<(), String> {
    let output = std::process::Command::new("git")
        .args(["branch", "-D", branch])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("not found") {
            return Err(stderr.to_string());
        }
    }
    Ok(())
}
