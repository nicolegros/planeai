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

/// Point a linked worktree back at `repo_path`, e.g. after the main checkout moved.
/// Fails without changes when the worktree belongs to another repository.
pub fn worktree_repair(repo_path: &str, worktree_path: &str) -> Result<(), String> {
    let output = git_cmd()
        .args(["worktree", "repair", worktree_path])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
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

/// The worktree path a session records for `worktree_path`: None when it is the repo's
/// main checkout, so the session is treated as a checkout session.
pub fn linked_worktree_path(repo_path: &str, worktree_path: &str) -> Option<String> {
    let canonical = |path: &str| std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
    (canonical(repo_path) != canonical(worktree_path)).then(|| worktree_path.to_string())
}

/// Find the worktree path where a given branch is checked out.
/// Returns None if the branch is not checked out in any worktree.
pub fn find_worktree_for_branch(repo_path: &str, branch: &str) -> Option<String> {
    let output = git_cmd()
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut current_path: Option<String> = None;

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.to_string());
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            if b == branch {
                return current_path;
            }
        } else if line.is_empty() {
            current_path = None;
        }
    }

    None
}

fn local_branch_exists(repo_path: &str, branch: &str) -> bool {
    git_cmd()
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .current_dir(repo_path)
        .status()
        .is_ok_and(|status| status.success())
}

/// Add a worktree that checks out an existing branch, or a remote one git can track.
fn worktree_add_existing(repo_path: &str, worktree_path: &str, branch: &str) -> Result<(), String> {
    let output = git_cmd()
        .args(["worktree", "add", worktree_path, branch])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

/// Where a new session should check out its branch.
#[derive(Debug, Clone, Copy)]
pub enum CheckoutTarget<'a> {
    /// Switch the repo's main checkout to the branch.
    Main { base: Option<&'a str> },
    /// Add a worktree at `path` for the branch.
    NewWorktree { path: &'a str, base: &'a str },
}

/// The checkout a session runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCheckout {
    /// Another checkout already had the branch, so the session runs there.
    Reused {
        path: String,
    },
    Main {
        created_branch: bool,
    },
    NewWorktree {
        path: String,
        created_branch: bool,
    },
}

/// Check out `branch` for a new session. A branch some other checkout already holds is
/// reused there, since git allows a branch in one worktree at a time. `create` asks for a
/// new branch, and is ignored when the branch already exists locally.
pub fn check_out_for_session(
    repo_path: &str,
    branch: &str,
    create: bool,
    target: CheckoutTarget,
) -> Result<SessionCheckout, String> {
    if let Some(path) = find_worktree_for_branch(repo_path, branch) {
        let is_main = linked_worktree_path(repo_path, &path).is_none();
        if !(is_main && matches!(target, CheckoutTarget::Main { .. })) {
            return Ok(SessionCheckout::Reused { path });
        }
    }
    let created_branch = create && !local_branch_exists(repo_path, branch);
    match target {
        CheckoutTarget::Main { base } => {
            checkout_branch(repo_path, branch, created_branch, base)?;
            Ok(SessionCheckout::Main { created_branch })
        }
        CheckoutTarget::NewWorktree { path, base } => {
            if created_branch {
                worktree_add(repo_path, path, branch, base)?;
            } else {
                worktree_add_existing(repo_path, path, branch)?;
            }
            Ok(SessionCheckout::NewWorktree {
                path: path.to_string(),
                created_branch,
            })
        }
    }
}
