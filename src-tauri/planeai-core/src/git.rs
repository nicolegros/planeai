use std::collections::HashSet;
use std::process::Command;

use crate::command::no_window;

/// Create a `git` Command with CREATE_NO_WINDOW on Windows.
fn git_cmd() -> Command {
    let mut cmd = Command::new("git");
    no_window(&mut cmd);
    cmd
}

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

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    /// For renames, the original path before the rename.
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FileDiff {
    pub original: String,
    pub modified: String,
    pub language: String,
}

/// Parse a git rename numstat path like `{old => new}/file.rs` or `old/path => new/path`
/// into (old_path, new_path).
fn parse_rename_path(raw: &str) -> (String, String) {
    if let Some(arrow_pos) = raw.find(" => ") {
        // Check for brace syntax: prefix{old => new}suffix
        if let Some(brace_start) = raw[..arrow_pos].rfind('{') {
            if let Some(brace_end) = raw[arrow_pos..].find('}') {
                let prefix = &raw[..brace_start];
                let old_part = &raw[brace_start + 1..arrow_pos];
                let new_part = &raw[arrow_pos + 4..arrow_pos + brace_end];
                let suffix = &raw[arrow_pos + brace_end + 1..];
                let old = format!("{prefix}{old_part}{suffix}");
                let new = format!("{prefix}{new_part}{suffix}");
                return (old.replace("//", "/"), new.replace("//", "/"));
            }
        }
        // Simple rename: old/path => new/path
        let old = raw[..arrow_pos].to_string();
        let new = raw[arrow_pos + 4..].to_string();
        return (old, new);
    }
    (raw.to_string(), raw.to_string())
}

/// Get the list of changed files between a base branch and the current working tree (or a specific head ref).
/// When `head_ref` is None: includes committed changes on the branch + uncommitted modifications + untracked files.
/// When `head_ref` is Some: shows only committed changes between base and the specified ref.
pub fn get_changed_files(
    repo_path: &str,
    base_branch: &str,
    head_ref: Option<&str>,
) -> Result<Vec<ChangedFile>, String> {
    let resolved = resolve_base_branch(repo_path, base_branch)?;

    let diff_range = match head_ref {
        Some(h) => format!("{resolved}..{h}"),
        None => resolved.clone(),
    };

    let output = git_cmd()
        .args(["diff", "--numstat", &diff_range])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let mut files: Vec<ChangedFile> = Vec::new();

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() != 3 {
            continue;
        }
        let additions = parts[0].parse::<u32>().unwrap_or(0);
        let deletions = parts[1].parse::<u32>().unwrap_or(0);
        let raw_path = parts[2];
        let (old_path, new_path) = parse_rename_path(raw_path);
        let is_rename = old_path != new_path;
        files.push(ChangedFile {
            path: new_path,
            status: String::new(),
            additions,
            deletions,
            old_path: if is_rename { Some(old_path) } else { None },
        });
    }

    // Get status for each file
    let status_output = git_cmd()
        .args(["diff", "--name-status", &diff_range])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if status_output.status.success() {
        for line in String::from_utf8_lossy(&status_output.stdout).lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 2 {
                continue;
            }
            let status = parts[0].chars().next().unwrap_or('M').to_string();
            // Renames have 3 columns: R100\told_path\tnew_path
            let path = if status == "R" && parts.len() >= 3 {
                parts[2]
            } else {
                parts[parts.len() - 1]
            };
            if let Some(f) = files.iter_mut().find(|f| f.path == path) {
                f.status = status;
            }
        }
    }

    // Include untracked files only when comparing to working tree (head_ref is None)
    if head_ref.is_none() {
        let untracked_output = git_cmd()
            .args(["ls-files", "--others", "--exclude-standard"])
            .current_dir(repo_path)
            .output()
            .map_err(|e| format!("failed to run git: {e}"))?;

        if untracked_output.status.success() {
            for line in String::from_utf8_lossy(&untracked_output.stdout).lines() {
                let path = line.trim().to_string();
                if path.is_empty() {
                    continue;
                }
                let content = std::fs::read_to_string(std::path::Path::new(repo_path).join(&path))
                    .unwrap_or_default();
                let additions = content.lines().count() as u32;
                files.push(ChangedFile {
                    path,
                    status: "A".to_string(),
                    additions,
                    deletions: 0,
                    old_path: None,
                });
            }
        }
    }

    Ok(files)
}

/// Get the original and modified content of a file for diff display.
/// When `head_ref` is None: Original = content at the base branch, Modified = current working tree content.
/// When `head_ref` is Some: Original = content at base, Modified = content at head ref.
/// For renames, `old_path` specifies the path in the base branch.
pub fn get_file_diff(
    repo_path: &str,
    base_branch: &str,
    file_path: &str,
    old_path: Option<&str>,
    head_ref: Option<&str>,
) -> Result<FileDiff, String> {
    let resolved = resolve_base_branch(repo_path, base_branch)?;
    let base_file_path = old_path.unwrap_or(file_path);
    // Get original content from base branch
    let original_output = git_cmd()
        .args(["show", &format!("{resolved}:{base_file_path}")])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    let original = if original_output.status.success() {
        String::from_utf8_lossy(&original_output.stdout).to_string()
    } else {
        String::new()
    };

    // Get modified content: from head ref if specified, otherwise from working tree
    let modified = match head_ref {
        Some(h) => {
            let output = git_cmd()
                .args(["show", &format!("{h}:{file_path}")])
                .current_dir(repo_path)
                .output()
                .map_err(|e| format!("failed to run git: {e}"))?;
            if output.status.success() {
                String::from_utf8_lossy(&output.stdout).to_string()
            } else {
                String::new()
            }
        }
        None => {
            let full_path = std::path::Path::new(repo_path).join(file_path);
            std::fs::read_to_string(&full_path).unwrap_or_default()
        }
    };

    let language = detect_language(file_path);

    Ok(FileDiff {
        original,
        modified,
        language,
    })
}

/// Get the unified diff patch for a single file. Uses native git diff which is
/// much faster than recomputing the diff in JavaScript.
/// When `head_ref` is None: diffs base against working tree.
/// When `head_ref` is Some: diffs base..head (committed only).
pub fn get_file_patch(
    repo_path: &str,
    base_branch: &str,
    file_path: &str,
    old_path: Option<&str>,
    head_ref: Option<&str>,
) -> Result<String, String> {
    let resolved = resolve_base_branch(repo_path, base_branch)?;
    let base_file_path = old_path.unwrap_or(file_path);

    let diff_range = match head_ref {
        Some(h) => format!("{resolved}..{h}"),
        None => resolved.clone(),
    };

    // Try tracked file diff first
    let output = git_cmd()
        .args([
            "diff",
            "--no-color",
            "-U3",
            &diff_range,
            "--",
            base_file_path,
        ])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    let patch = String::from_utf8_lossy(&output.stdout).to_string();

    // If empty and comparing to working tree, file might be untracked — generate a diff against /dev/null
    if patch.trim().is_empty() && head_ref.is_none() {
        let full_path = std::path::Path::new(repo_path).join(file_path);
        let content = std::fs::read_to_string(&full_path).unwrap_or_default();
        if content.is_empty() {
            return Ok(String::new());
        }
        // Build a synthetic unified diff for new files
        let lines: Vec<&str> = content.lines().collect();
        let count = lines.len();
        let mut result = format!(
            "--- /dev/null\n+++ b/{}\n@@ -0,0 +1,{} @@\n",
            file_path, count
        );
        for line in &lines {
            result.push('+');
            result.push_str(line);
            result.push('\n');
        }
        return Ok(result);
    }

    Ok(patch)
}

/// Get unified diff patches for all given files in one call.
/// Returns a vec of patch strings in the same order as the input paths.
pub fn get_all_file_patches(
    repo_path: &str,
    base_branch: &str,
    files: &[(String, Option<String>)],
    head_ref: Option<&str>,
) -> Result<Vec<String>, String> {
    files
        .iter()
        .map(|(path, old_path)| {
            get_file_patch(repo_path, base_branch, path, old_path.as_deref(), head_ref)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CommitEntry {
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
}

/// List the last N commits on the current branch.
pub fn list_commits(repo_path: &str, limit: u32) -> Result<Vec<CommitEntry>, String> {
    let output = git_cmd()
        .args(["log", "--format=%H %h %s", &format!("-{limit}")])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let mut commits = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        // Format: full_sha short_sha subject (subject may contain spaces)
        let mut parts = line.splitn(3, ' ');
        let sha = parts.next().unwrap_or("").to_string();
        let short_sha = parts.next().unwrap_or("").to_string();
        let subject = parts.next().unwrap_or("").to_string();
        if !sha.is_empty() {
            commits.push(CommitEntry {
                sha,
                short_sha,
                subject,
            });
        }
    }

    Ok(commits)
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

pub fn detect_language(file_path: &str) -> String {
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
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn configure_git_identity(path: &std::path::Path) {
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(path)
            .output()
            .unwrap();
    }

    fn git(path: &std::path::Path, args: &[&str]) {
        Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap();
    }

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        configure_git_identity(dir.path());
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
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
        Command::new("git")
            .args(["init", "--bare", "-b", "main"])
            .current_dir(remote_dir.path())
            .output()
            .unwrap();

        // Create a local repo, add remote, push a branch
        let upstream = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(upstream.path())
            .output()
            .unwrap();
        configure_git_identity(upstream.path());
        Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                remote_dir.path().to_str().unwrap(),
            ])
            .current_dir(upstream.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(upstream.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["push", "origin", "main"])
            .current_dir(upstream.path())
            .output()
            .unwrap();

        // Create the "clone" that will be our test repo
        let clone = tempfile::tempdir().unwrap();
        Command::new("git")
            .args([
                "clone",
                remote_dir.path().to_str().unwrap(),
                clone.path().to_str().unwrap(),
            ])
            .output()
            .unwrap();
        configure_git_identity(clone.path());

        // Push a new branch from upstream so clone can fetch it
        Command::new("git")
            .args(["checkout", "-b", "feat/new"])
            .current_dir(upstream.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "new feature"])
            .current_dir(upstream.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["push", "origin", "feat/new"])
            .current_dir(upstream.path())
            .output()
            .unwrap();

        (clone, remote_dir)
    }

    #[test]
    fn resolve_base_branch_remote_fetches_and_returns_origin_ref() {
        let (repo, _remote) = init_repo_with_remote();
        let result = resolve_base_branch(repo.path().to_str().unwrap(), "remote:feat/new").unwrap();
        assert_eq!(result, "origin/feat/new");
    }

    #[test]
    fn resolve_base_branch_fetches_without_remote_prefix() {
        let (repo, _remote) = init_repo_with_remote();
        let result = resolve_base_branch(repo.path().to_str().unwrap(), "feat/new").unwrap();
        assert_eq!(result, "origin/feat/new");
    }

    #[test]
    fn resolve_base_branch_remote_falls_back_to_local_when_fetch_fails() {
        let (repo, _remote) = init_repo_with_remote();
        let result =
            resolve_base_branch(repo.path().to_str().unwrap(), "remote:nonexistent-branch")
                .unwrap();
        assert_eq!(result, "nonexistent-branch");
    }

    fn init_repo_with_feature_branch() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(p)
            .output()
            .unwrap();
        configure_git_identity(p);
        fs::write(p.join("existing.txt"), "hello\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(p)
            .output()
            .unwrap();
        // Create feature branch
        Command::new("git")
            .args(["checkout", "-b", "feat"])
            .current_dir(p)
            .output()
            .unwrap();
        // Modify a file and add a new file
        fs::write(p.join("existing.txt"), "hello\nworld\n").unwrap();
        fs::write(p.join("new_file.txt"), "brand new\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "feature work"])
            .current_dir(p)
            .output()
            .unwrap();
        dir
    }

    #[test]
    fn get_changed_files_returns_modified_and_added_files() {
        let repo = init_repo_with_feature_branch();
        let files = get_changed_files(repo.path().to_str().unwrap(), "main", None).unwrap();
        assert_eq!(files.len(), 2);

        let modified = files.iter().find(|f| f.path == "existing.txt").unwrap();
        assert_eq!(modified.status, "M");
        assert_eq!(modified.additions, 1);
        assert_eq!(modified.deletions, 0);
        assert_eq!(modified.old_path, None);

        let added = files.iter().find(|f| f.path == "new_file.txt").unwrap();
        assert_eq!(added.status, "A");
        assert_eq!(added.additions, 1);
        assert_eq!(added.deletions, 0);
        assert_eq!(added.old_path, None);
    }

    #[test]
    fn get_changed_files_includes_untracked_as_added() {
        let repo = init_repo_with_feature_branch();
        // Add an untracked file (not staged or committed)
        fs::write(repo.path().join("untracked.txt"), "line1\nline2\n").unwrap();

        let files = get_changed_files(repo.path().to_str().unwrap(), "main", None).unwrap();
        let untracked = files.iter().find(|f| f.path == "untracked.txt").unwrap();
        assert_eq!(untracked.status, "A");
        assert_eq!(untracked.additions, 2);
        assert_eq!(untracked.deletions, 0);
    }

    #[test]
    fn get_file_diff_returns_original_and_modified_for_modified_file() {
        let repo = init_repo_with_feature_branch();
        let diff = get_file_diff(
            repo.path().to_str().unwrap(),
            "main",
            "existing.txt",
            None,
            None,
        )
        .unwrap();
        assert_eq!(diff.original, "hello\n");
        assert_eq!(diff.modified, "hello\nworld\n");
        assert_eq!(diff.language, "plaintext");
    }

    #[test]
    fn get_file_diff_returns_empty_original_for_new_file() {
        let repo = init_repo_with_feature_branch();
        let diff = get_file_diff(
            repo.path().to_str().unwrap(),
            "main",
            "new_file.txt",
            None,
            None,
        )
        .unwrap();
        assert_eq!(diff.original, "");
        assert_eq!(diff.modified, "brand new\n");
    }

    #[test]
    fn get_file_diff_returns_empty_modified_for_deleted_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(p)
            .output()
            .unwrap();
        configure_git_identity(p);
        fs::write(p.join("doomed.txt"), "will be deleted\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", "feat"])
            .current_dir(p)
            .output()
            .unwrap();
        fs::remove_file(p.join("doomed.txt")).unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "delete file"])
            .current_dir(p)
            .output()
            .unwrap();

        let diff = get_file_diff(p.to_str().unwrap(), "main", "doomed.txt", None, None).unwrap();
        assert_eq!(diff.original, "will be deleted\n");
        assert_eq!(diff.modified, "");
    }

    #[test]
    fn get_changed_files_detects_renamed_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(p)
            .output()
            .unwrap();
        configure_git_identity(p);
        fs::create_dir_all(p.join("src/client")).unwrap();
        fs::write(p.join("src/client/auth.rs"), "fn auth() {}\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", "feat"])
            .current_dir(p)
            .output()
            .unwrap();
        fs::create_dir_all(p.join("crates/client/src")).unwrap();
        Command::new("git")
            .args(["mv", "src/client/auth.rs", "crates/client/src/auth.rs"])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "rename"])
            .current_dir(p)
            .output()
            .unwrap();

        let files = get_changed_files(p.to_str().unwrap(), "main", None).unwrap();
        let renamed = files
            .iter()
            .find(|f| f.path == "crates/client/src/auth.rs")
            .unwrap();
        assert_eq!(renamed.status, "R");
        assert_eq!(renamed.old_path, Some("src/client/auth.rs".to_string()));
    }

    #[test]
    fn get_file_diff_uses_old_path_for_renamed_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(p)
            .output()
            .unwrap();
        configure_git_identity(p);
        fs::create_dir_all(p.join("src")).unwrap();
        fs::write(p.join("src/lib.rs"), "original content\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", "feat"])
            .current_dir(p)
            .output()
            .unwrap();
        fs::create_dir_all(p.join("crates")).unwrap();
        Command::new("git")
            .args(["mv", "src/lib.rs", "crates/lib.rs"])
            .current_dir(p)
            .output()
            .unwrap();
        fs::write(p.join("crates/lib.rs"), "modified content\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "rename+edit"])
            .current_dir(p)
            .output()
            .unwrap();

        let diff = get_file_diff(
            p.to_str().unwrap(),
            "main",
            "crates/lib.rs",
            Some("src/lib.rs"),
            None,
        )
        .unwrap();
        assert_eq!(diff.original, "original content\n");
        assert_eq!(diff.modified, "modified content\n");
    }

    #[test]
    fn parse_rename_path_brace_syntax() {
        let (old, new) = parse_rename_path("{src/client => crates/client/src}/auth.rs");
        assert_eq!(old, "src/client/auth.rs");
        assert_eq!(new, "crates/client/src/auth.rs");
    }

    #[test]
    fn parse_rename_path_arrow_syntax() {
        let (old, new) = parse_rename_path("src/repo/mod.rs => crates/persistence/src/lib.rs");
        assert_eq!(old, "src/repo/mod.rs");
        assert_eq!(new, "crates/persistence/src/lib.rs");
    }

    #[test]
    fn list_branches_deduplicates_remotes() {
        let remote1 = tempfile::tempdir().unwrap();
        git(remote1.path(), &["init", "--bare", "-b", "main"]);
        let remote2 = tempfile::tempdir().unwrap();
        git(remote2.path(), &["init", "--bare", "-b", "main"]);

        // Seed both remotes with a shared commit + a branch only on remote2
        let seed = tempfile::tempdir().unwrap();
        git(seed.path(), &["init", "-b", "main"]);
        configure_git_identity(seed.path());
        git(seed.path(), &["commit", "--allow-empty", "-m", "init"]);
        git(
            seed.path(),
            &["remote", "add", "r1", remote1.path().to_str().unwrap()],
        );
        git(seed.path(), &["push", "r1", "main"]);
        git(
            seed.path(),
            &["remote", "add", "r2", remote2.path().to_str().unwrap()],
        );
        git(seed.path(), &["push", "r2", "main"]);
        git(seed.path(), &["checkout", "-b", "feat/only-r2"]);
        git(seed.path(), &["commit", "--allow-empty", "-m", "feat"]);
        git(seed.path(), &["push", "r2", "feat/only-r2"]);

        // Clone from remote1, add remote2, fetch all
        let repo = tempfile::tempdir().unwrap();
        Command::new("git")
            .args([
                "clone",
                remote1.path().to_str().unwrap(),
                repo.path().to_str().unwrap(),
            ])
            .output()
            .unwrap();
        configure_git_identity(repo.path());
        git(
            repo.path(),
            &[
                "remote",
                "add",
                "upstream",
                remote2.path().to_str().unwrap(),
            ],
        );
        git(repo.path(), &["fetch", "--all"]);

        let result = list_branches(repo.path().to_str().unwrap()).unwrap();

        assert!(
            !result.iter().any(|b| b == "remote:main"),
            "remote:main should not appear when main exists locally: {:?}",
            result
        );
        assert_eq!(
            result
                .iter()
                .filter(|b| *b == "remote:feat/only-r2")
                .count(),
            1,
            "remote:feat/only-r2 should appear exactly once: {:?}",
            result
        );
        let unique: HashSet<&String> = result.iter().collect();
        assert_eq!(
            unique.len(),
            result.len(),
            "all entries should be unique: {:?}",
            result
        );
    }

    #[test]
    fn get_file_patch_with_head_ref_uses_ref_range() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-b", "main"]);
        configure_git_identity(p);
        fs::write(p.join("file.txt"), "base\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "init"]);
        git(p, &["checkout", "-b", "feat"]);
        fs::write(p.join("file.txt"), "changed\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "change"]);
        // Uncommitted change — should NOT appear in patch
        fs::write(p.join("file.txt"), "uncommitted\n").unwrap();

        let patch =
            get_file_patch(p.to_str().unwrap(), "main", "file.txt", None, Some("HEAD")).unwrap();
        assert!(
            patch.contains("+changed"),
            "patch should contain committed change: {}",
            patch
        );
        assert!(
            !patch.contains("+uncommitted"),
            "patch should NOT contain uncommitted change: {}",
            patch
        );
    }

    #[test]
    fn get_file_patch_with_head_ref_none_includes_uncommitted() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-b", "main"]);
        configure_git_identity(p);
        fs::write(p.join("file.txt"), "base\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "init"]);
        git(p, &["checkout", "-b", "feat"]);
        fs::write(p.join("file.txt"), "uncommitted\n").unwrap();

        let patch = get_file_patch(p.to_str().unwrap(), "main", "file.txt", None, None).unwrap();
        assert!(
            patch.contains("+uncommitted"),
            "patch should contain uncommitted change: {}",
            patch
        );
    }

    #[test]
    fn get_file_diff_with_head_ref_uses_committed_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-b", "main"]);
        configure_git_identity(p);
        fs::write(p.join("file.txt"), "base\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "init"]);
        git(p, &["checkout", "-b", "feat"]);
        fs::write(p.join("file.txt"), "committed\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "change"]);
        // Working tree has different content that should be ignored
        fs::write(p.join("file.txt"), "uncommitted\n").unwrap();

        let diff =
            get_file_diff(p.to_str().unwrap(), "main", "file.txt", None, Some("HEAD")).unwrap();
        assert_eq!(diff.original, "base\n");
        assert_eq!(diff.modified, "committed\n");
    }

    #[test]
    fn get_file_diff_with_head_ref_none_uses_working_tree() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-b", "main"]);
        configure_git_identity(p);
        fs::write(p.join("file.txt"), "base\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "init"]);
        git(p, &["checkout", "-b", "feat"]);
        fs::write(p.join("file.txt"), "working\n").unwrap();

        let diff = get_file_diff(p.to_str().unwrap(), "main", "file.txt", None, None).unwrap();
        assert_eq!(diff.original, "base\n");
        assert_eq!(diff.modified, "working\n");
    }

    #[test]
    fn get_changed_files_with_head_ref_shows_only_committed_changes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-b", "main"]);
        configure_git_identity(p);
        fs::write(p.join("file.txt"), "v1\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "init"]);
        // Create two commits on a feature branch
        git(p, &["checkout", "-b", "feat"]);
        fs::write(p.join("file.txt"), "v2\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "second"]);
        fs::write(p.join("file.txt"), "v3\n").unwrap();
        fs::write(p.join("new.txt"), "hello\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "third"]);
        // Also add an uncommitted change that should NOT appear
        fs::write(p.join("uncommitted.txt"), "wip\n").unwrap();

        // Compare main..HEAD (should show committed changes only, not uncommitted)
        let files = get_changed_files(p.to_str().unwrap(), "main", Some("HEAD")).unwrap();
        assert!(
            files.iter().any(|f| f.path == "file.txt"),
            "should include file.txt: {:?}",
            files
        );
        assert!(
            files.iter().any(|f| f.path == "new.txt"),
            "should include new.txt: {:?}",
            files
        );
        assert!(
            !files.iter().any(|f| f.path == "uncommitted.txt"),
            "should NOT include uncommitted.txt: {:?}",
            files
        );
    }

    #[test]
    fn get_changed_files_with_head_ref_none_includes_uncommitted() {
        let repo = init_repo_with_feature_branch();
        // Add an uncommitted file
        fs::write(repo.path().join("uncommitted.txt"), "wip\n").unwrap();

        let files = get_changed_files(repo.path().to_str().unwrap(), "main", None).unwrap();
        assert!(
            files.iter().any(|f| f.path == "uncommitted.txt"),
            "should include uncommitted.txt when head_ref is None: {:?}",
            files
        );
    }

    #[test]
    fn list_commits_returns_recent_commits() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-b", "main"]);
        configure_git_identity(p);
        git(p, &["commit", "--allow-empty", "-m", "first commit"]);
        git(p, &["commit", "--allow-empty", "-m", "second commit"]);
        git(p, &["commit", "--allow-empty", "-m", "third commit"]);

        let commits = list_commits(p.to_str().unwrap(), 2).unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].subject, "third commit");
        assert_eq!(commits[1].subject, "second commit");
        assert!(!commits[0].sha.is_empty());
        assert!(!commits[0].short_sha.is_empty());
        assert!(commits[0].sha.len() > commits[0].short_sha.len());
    }

    #[test]
    fn detect_default_branch_finds_main() {
        let repo = init_repo();
        let result = detect_default_branch(repo.path().to_str().unwrap()).unwrap();
        assert_eq!(result, "main");
    }

    #[test]
    fn detect_default_branch_finds_master() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        Command::new("git")
            .args(["init", "-b", "master"])
            .current_dir(p)
            .output()
            .unwrap();
        configure_git_identity(p);
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(p)
            .output()
            .unwrap();
        let result = detect_default_branch(p.to_str().unwrap()).unwrap();
        assert_eq!(result, "master");
    }
}
