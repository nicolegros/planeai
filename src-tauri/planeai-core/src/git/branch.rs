use std::collections::HashSet;

use super::git_cmd;

/// List local and remote branches for a git repo at the given path.
/// Remote-only branches are prefixed with "remote:" to distinguish them.
pub fn list_branches(repo_path: &str) -> Result<Vec<String>, String> {
    let output = git_cmd()
        .args(["branch", "--all", "--format=%(refname:short) %(refname)"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((short, full)) = line.split_once(' ') else {
            continue;
        };
        let short = short.trim();
        if short.is_empty() || full.contains("HEAD") {
            continue;
        }
        if full.starts_with("refs/remotes/") {
            let name = short.split_once('/').map(|x| x.1).unwrap_or(short);
            if seen.insert(name.to_string()) {
                result.push(format!("remote:{name}"));
            }
        } else {
            seen.insert(short.to_string());
            result.push(short.to_string());
        }
    }

    Ok(result)
}

/// Checkout an existing branch or create a new one (optionally from a start point).
pub fn checkout_branch(
    repo_path: &str,
    branch: &str,
    create: bool,
    start_point: Option<&str>,
) -> Result<(), String> {
    let resolved_start = start_point
        .map(|s| resolve_base_branch(repo_path, s))
        .transpose()?;
    let mut args = if create {
        vec!["checkout", "-b", branch]
    } else {
        vec!["checkout", branch]
    };
    if let Some(ref base) = resolved_start {
        if create {
            args.push(base);
        }
    }

    let output = git_cmd()
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
pub fn worktree_add(
    repo_path: &str,
    worktree_path: &str,
    new_branch: &str,
    base_branch: &str,
) -> Result<(), String> {
    let resolved = resolve_base_branch(repo_path, base_branch)?;
    let output = git_cmd()
        .args([
            "worktree",
            "add",
            "-b",
            new_branch,
            worktree_path,
            &resolved,
        ])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

/// Resolve a base branch reference.
/// Always attempts to fetch the branch from origin first and uses "origin/<name>".
/// Falls back to the local branch name if origin fetch fails.
/// Accepts an optional "remote:" prefix for backward compatibility (stripped before use).
pub fn resolve_base_branch(repo_path: &str, base: &str) -> Result<String, String> {
    let name = base.strip_prefix("remote:").unwrap_or(base);

    let output = git_cmd()
        .args(["fetch", "origin", name])
        .current_dir(repo_path)
        .output();

    match output {
        Ok(o) if o.status.success() => Ok(format!("origin/{name}")),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            eprintln!(
                "[warn] git fetch origin {name} failed, using local ref: {}",
                stderr.trim()
            );
            Ok(name.to_string())
        }
        Err(e) => {
            eprintln!("[warn] git fetch origin {name} failed, using local ref: {e}");
            Ok(name.to_string())
        }
    }
}

/// Remove a git worktree forcefully.
pub fn worktree_remove(repo_path: &str, worktree_path: &str) -> Result<(), String> {
    let output = git_cmd()
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

/// Detect the default branch of a repo (main, master, etc.).
/// Checks local branches for common names.
pub fn detect_default_branch(repo_path: &str) -> Result<String, String> {
    let output = git_cmd()
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    for candidate in &["main", "master"] {
        if stdout.lines().any(|l| l.trim() == *candidate) {
            return Ok(candidate.to_string());
        }
    }

    Err("could not detect default branch".to_string())
}
