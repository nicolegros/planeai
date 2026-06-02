use std::process::Command;

/// List local and remote branches for a git repo at the given path.
/// Remote-only branches are prefixed with "remote:" to distinguish them.
pub fn list_branches(repo_path: &str) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args(["branch", "--all", "--format=%(refname:short) %(refname)"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let mut local = Vec::new();
    let mut remote = Vec::new();
    let mut local_names = std::collections::HashSet::new();

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((short, full)) = line.split_once(' ') else { continue };
        let short = short.trim();
        if short.is_empty() || full.contains("HEAD") {
            continue;
        }
        if full.starts_with("refs/remotes/") {
            let name = short.splitn(2, '/').nth(1).unwrap_or(short);
            remote.push(name.to_string());
        } else {
            local_names.insert(short.to_string());
            local.push(short.to_string());
        }
    }

    for name in remote {
        local.push(format!("remote:{name}"));
    }

    Ok(local)
}

/// Checkout an existing branch or create a new one (optionally from a start point).
pub fn checkout_branch(repo_path: &str, branch: &str, create: bool, start_point: Option<&str>) -> Result<(), String> {
    let resolved_start = start_point.map(|s| resolve_base_branch(repo_path, s)).transpose()?;
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
    let resolved = resolve_base_branch(repo_path, base_branch)?;
    let output = Command::new("git")
        .args(["worktree", "add", "-b", new_branch, worktree_path, &resolved])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

/// Resolve a base branch reference. If prefixed with "remote:", fetches from origin and returns "origin/<name>".
pub fn resolve_base_branch(repo_path: &str, base: &str) -> Result<String, String> {
    let Some(name) = base.strip_prefix("remote:") else {
        return Ok(base.to_string());
    };

    let output = Command::new("git")
        .args(["fetch", "origin", name])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git fetch: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(format!("origin/{name}"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git").args(["init"]).current_dir(dir.path()).output().unwrap();
        Command::new("git").args(["commit", "--allow-empty", "-m", "init"]).current_dir(dir.path()).output().unwrap();
        dir
    }

    #[test]
    fn resolve_base_branch_local_returns_unchanged() {
        let repo = init_repo();
        let result = resolve_base_branch(repo.path().to_str().unwrap(), "main").unwrap();
        assert_eq!(result, "main");
    }

    fn init_repo_with_remote() -> (tempfile::TempDir, tempfile::TempDir) {
        // Create a bare "remote" repo
        let remote_dir = tempfile::tempdir().unwrap();
        Command::new("git").args(["init", "--bare"]).current_dir(remote_dir.path()).output().unwrap();

        // Create a local repo, add remote, push a branch
        let upstream = tempfile::tempdir().unwrap();
        Command::new("git").args(["init"]).current_dir(upstream.path()).output().unwrap();
        Command::new("git").args(["remote", "add", "origin", remote_dir.path().to_str().unwrap()]).current_dir(upstream.path()).output().unwrap();
        Command::new("git").args(["commit", "--allow-empty", "-m", "init"]).current_dir(upstream.path()).output().unwrap();
        Command::new("git").args(["push", "origin", "main"]).current_dir(upstream.path()).output().unwrap();

        // Create the "clone" that will be our test repo
        let clone = tempfile::tempdir().unwrap();
        Command::new("git").args(["clone", remote_dir.path().to_str().unwrap(), clone.path().to_str().unwrap()]).output().unwrap();

        // Push a new branch from upstream so clone can fetch it
        Command::new("git").args(["checkout", "-b", "feat/new"]).current_dir(upstream.path()).output().unwrap();
        Command::new("git").args(["commit", "--allow-empty", "-m", "new feature"]).current_dir(upstream.path()).output().unwrap();
        Command::new("git").args(["push", "origin", "feat/new"]).current_dir(upstream.path()).output().unwrap();

        (clone, remote_dir)
    }

    #[test]
    fn resolve_base_branch_remote_fetches_and_returns_origin_ref() {
        let (repo, _remote) = init_repo_with_remote();
        let result = resolve_base_branch(repo.path().to_str().unwrap(), "remote:feat/new").unwrap();
        assert_eq!(result, "origin/feat/new");
    }

    #[test]
    fn resolve_base_branch_remote_returns_error_when_fetch_fails() {
        let (repo, _remote) = init_repo_with_remote();
        let result = resolve_base_branch(repo.path().to_str().unwrap(), "remote:nonexistent-branch");
        assert!(result.is_err());
    }
}
