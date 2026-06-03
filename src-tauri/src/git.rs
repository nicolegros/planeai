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

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FileDiff {
    pub original: String,
    pub modified: String,
    pub language: String,
}

/// Get the list of changed files between a base branch and the current working tree.
/// Includes committed changes on the branch + uncommitted modifications + untracked files.
pub fn get_changed_files(repo_path: &str, base_branch: &str) -> Result<Vec<ChangedFile>, String> {
    let output = Command::new("git")
        .args(["diff", "--numstat", base_branch])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let mut files: Vec<ChangedFile> = Vec::new();

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 3 { continue; }
        let additions = parts[0].parse::<u32>().unwrap_or(0);
        let deletions = parts[1].parse::<u32>().unwrap_or(0);
        let path = parts[2].to_string();
        files.push(ChangedFile { path, status: String::new(), additions, deletions });
    }

    // Get status for each file
    let status_output = Command::new("git")
        .args(["diff", "--name-status", base_branch])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if status_output.status.success() {
        for line in String::from_utf8_lossy(&status_output.stdout).lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 2 { continue; }
            let status = parts[0].chars().next().unwrap_or('M').to_string();
            let path = parts[parts.len() - 1];
            if let Some(f) = files.iter_mut().find(|f| f.path == path) {
                f.status = status;
            }
        }
    }

    // Include untracked files as Added
    let untracked_output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if untracked_output.status.success() {
        for line in String::from_utf8_lossy(&untracked_output.stdout).lines() {
            let path = line.trim().to_string();
            if path.is_empty() { continue; }
            let content = std::fs::read_to_string(std::path::Path::new(repo_path).join(&path)).unwrap_or_default();
            let additions = content.lines().count() as u32;
            files.push(ChangedFile { path, status: "A".to_string(), additions, deletions: 0 });
        }
    }

    Ok(files)
}

/// Get the original and modified content of a file for diff display.
/// Original = content at the base branch, Modified = current working tree content.
pub fn get_file_diff(repo_path: &str, base_branch: &str, file_path: &str) -> Result<FileDiff, String> {
    // Get original content from base branch
    let original_output = Command::new("git")
        .args(["show", &format!("{base_branch}:{file_path}")])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    let original = if original_output.status.success() {
        String::from_utf8_lossy(&original_output.stdout).to_string()
    } else {
        String::new()
    };

    // Get modified content from working tree
    let full_path = std::path::Path::new(repo_path).join(file_path);
    let modified = std::fs::read_to_string(&full_path).unwrap_or_default();

    let language = detect_language(file_path);

    Ok(FileDiff { original, modified, language })
}

/// Detect the default branch of a repo (main, master, etc.).
/// Checks local branches for common names.
pub fn detect_default_branch(repo_path: &str) -> Result<String, String> {
    let output = Command::new("git")
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

fn detect_language(file_path: &str) -> String {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "json" => "json",
        "html" => "html",
        "css" => "css",
        "svelte" => "html",
        "md" => "markdown",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "py" => "python",
        "sh" => "shell",
        "sql" => "sql",
        _ => "plaintext",
    }.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::fs;

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

    fn init_repo_with_feature_branch() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        Command::new("git").args(["init"]).current_dir(p).output().unwrap();
        fs::write(p.join("existing.txt"), "hello\n").unwrap();
        Command::new("git").args(["add", "."]).current_dir(p).output().unwrap();
        Command::new("git").args(["commit", "-m", "init"]).current_dir(p).output().unwrap();
        // Create feature branch
        Command::new("git").args(["checkout", "-b", "feat"]).current_dir(p).output().unwrap();
        // Modify a file and add a new file
        fs::write(p.join("existing.txt"), "hello\nworld\n").unwrap();
        fs::write(p.join("new_file.txt"), "brand new\n").unwrap();
        Command::new("git").args(["add", "."]).current_dir(p).output().unwrap();
        Command::new("git").args(["commit", "-m", "feature work"]).current_dir(p).output().unwrap();
        dir
    }

    #[test]
    fn get_changed_files_returns_modified_and_added_files() {
        let repo = init_repo_with_feature_branch();
        let files = get_changed_files(repo.path().to_str().unwrap(), "main").unwrap();
        assert_eq!(files.len(), 2);

        let modified = files.iter().find(|f| f.path == "existing.txt").unwrap();
        assert_eq!(modified.status, "M");
        assert_eq!(modified.additions, 1);
        assert_eq!(modified.deletions, 0);

        let added = files.iter().find(|f| f.path == "new_file.txt").unwrap();
        assert_eq!(added.status, "A");
        assert_eq!(added.additions, 1);
        assert_eq!(added.deletions, 0);
    }

    #[test]
    fn get_changed_files_includes_untracked_as_added() {
        let repo = init_repo_with_feature_branch();
        // Add an untracked file (not staged or committed)
        fs::write(repo.path().join("untracked.txt"), "line1\nline2\n").unwrap();

        let files = get_changed_files(repo.path().to_str().unwrap(), "main").unwrap();
        let untracked = files.iter().find(|f| f.path == "untracked.txt").unwrap();
        assert_eq!(untracked.status, "A");
        assert_eq!(untracked.additions, 2);
        assert_eq!(untracked.deletions, 0);
    }

    #[test]
    fn get_file_diff_returns_original_and_modified_for_modified_file() {
        let repo = init_repo_with_feature_branch();
        let diff = get_file_diff(repo.path().to_str().unwrap(), "main", "existing.txt").unwrap();
        assert_eq!(diff.original, "hello\n");
        assert_eq!(diff.modified, "hello\nworld\n");
        assert_eq!(diff.language, "plaintext");
    }

    #[test]
    fn get_file_diff_returns_empty_original_for_new_file() {
        let repo = init_repo_with_feature_branch();
        let diff = get_file_diff(repo.path().to_str().unwrap(), "main", "new_file.txt").unwrap();
        assert_eq!(diff.original, "");
        assert_eq!(diff.modified, "brand new\n");
    }

    #[test]
    fn get_file_diff_returns_empty_modified_for_deleted_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        Command::new("git").args(["init"]).current_dir(p).output().unwrap();
        fs::write(p.join("doomed.txt"), "will be deleted\n").unwrap();
        Command::new("git").args(["add", "."]).current_dir(p).output().unwrap();
        Command::new("git").args(["commit", "-m", "init"]).current_dir(p).output().unwrap();
        Command::new("git").args(["checkout", "-b", "feat"]).current_dir(p).output().unwrap();
        fs::remove_file(p.join("doomed.txt")).unwrap();
        Command::new("git").args(["add", "."]).current_dir(p).output().unwrap();
        Command::new("git").args(["commit", "-m", "delete file"]).current_dir(p).output().unwrap();

        let diff = get_file_diff(p.to_str().unwrap(), "main", "doomed.txt").unwrap();
        assert_eq!(diff.original, "will be deleted\n");
        assert_eq!(diff.modified, "");
    }

    #[test]
    fn detect_default_branch_finds_main() {
        let repo = init_repo(); // init_repo creates a repo with "main" branch
        let result = detect_default_branch(repo.path().to_str().unwrap()).unwrap();
        assert_eq!(result, "main");
    }

    #[test]
    fn detect_default_branch_finds_master() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        Command::new("git").args(["init", "-b", "master"]).current_dir(p).output().unwrap();
        Command::new("git").args(["commit", "--allow-empty", "-m", "init"]).current_dir(p).output().unwrap();
        let result = detect_default_branch(p.to_str().unwrap()).unwrap();
        assert_eq!(result, "master");
    }
}
