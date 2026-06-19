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

/// Remove worktrees from sessions that have been exited/destroyed for more than 48 hours.
/// Returns a list of errors encountered (empty = all good).
pub fn cleanup_stale_worktrees(
    conn: &rusqlite::Connection,
    remove_worktree: impl Fn(&str, &str) -> Result<(), String>,
) -> Vec<String> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::hours(48)).to_rfc3339();
    let mut errors = vec![];

    let mut stmt = match conn.prepare(
        "SELECT s.id, s.worktree_path, p.path FROM sessions s
         JOIN projects p ON p.id = s.project_id
         WHERE s.status IN ('exited', 'destroyed')
           AND s.worktree_path IS NOT NULL
           AND s.updated_at IS NOT NULL
           AND s.updated_at < ?1",
    ) {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("query: {e}"));
            return errors;
        }
    };
    let rows: Vec<(String, String, String)> =
        match stmt.query_map(rusqlite::params![cutoff], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        }) {
            Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                errors.push(format!("query: {e}"));
                return errors;
            }
        };

    for (_session_id, worktree_path, project_path) in &rows {
        if let Err(e) = remove_worktree(project_path, worktree_path) {
            errors.push(format!("session {}: {e}", _session_id));
        }
    }

    errors
}
