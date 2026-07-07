use super::branch::resolve_base_branch;
use super::{detect_language, git_cmd};

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

/// Build a synthetic unified diff patch for a new file (content shown as all additions).
/// When `include_git_header` is true, includes the `diff --git` preamble needed for combined patches.
fn synthetic_new_file_patch(file_path: &str, content: &str, include_git_header: bool) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let count = lines.len();
    let mut result = String::new();
    if include_git_header {
        result.push_str(&format!(
            "diff --git a/{fp} b/{fp}\nnew file mode 100644\n",
            fp = file_path
        ));
    }
    result.push_str(&format!(
        "--- /dev/null\n+++ b/{}\n@@ -0,0 +1,{} @@\n",
        file_path, count
    ));
    for line in &lines {
        result.push('+');
        result.push_str(line);
        result.push('\n');
    }
    result
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
        return Ok(synthetic_new_file_patch(file_path, &content, false));
    }

    Ok(patch)
}

/// Get a single combined unified diff patch containing all changed files.
/// Runs one `git diff` subprocess instead of N per-file calls.
/// When `head_ref` is None, diffs base against the working tree and includes
/// synthetic patches for untracked files. Both git commands run in parallel
/// via `std::thread::scope`.
/// When `head_ref` is Some, diffs base..head (committed only).
pub fn get_combined_patch(
    repo_path: &str,
    base_branch: &str,
    head_ref: Option<&str>,
) -> Result<String, String> {
    let resolved = resolve_base_branch(repo_path, base_branch)?;

    let diff_range = match head_ref {
        Some(h) => format!("{resolved}..{h}"),
        None => resolved.clone(),
    };

    // When comparing to working tree, run git diff and git ls-files in parallel
    if head_ref.is_none() {
        let (diff_result, untracked_result) = std::thread::scope(|s| {
            let diff_handle = s.spawn(|| {
                let output = git_cmd()
                    .args([
                        "diff",
                        "--no-color",
                        "-U3",
                        "--find-renames",
                        "--no-ext-diff",
                        &diff_range,
                    ])
                    .current_dir(repo_path)
                    .output()
                    .map_err(|e| format!("failed to run git: {e}"))?;

                if !output.status.success() {
                    return Err(String::from_utf8_lossy(&output.stderr).to_string());
                }
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            });

            let untracked_handle = s.spawn(|| -> Result<String, String> {
                let output = git_cmd()
                    .args(["ls-files", "--others", "--exclude-standard"])
                    .current_dir(repo_path)
                    .output()
                    .map_err(|e| format!("failed to run git ls-files: {e}"))?;

                if !output.status.success() {
                    return Ok(String::new());
                }

                let mut patches = String::new();
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let file_path = line.trim();
                    if file_path.is_empty() {
                        continue;
                    }
                    let full_path = std::path::Path::new(repo_path).join(file_path);
                    let content = match std::fs::read_to_string(&full_path) {
                        Ok(c) => c,
                        Err(_) => continue, // skip binary/unreadable files
                    };
                    if content.is_empty() {
                        continue;
                    }
                    patches.push_str(&synthetic_new_file_patch(file_path, &content, true));
                }
                Ok(patches)
            });

            (diff_handle.join().unwrap(), untracked_handle.join().unwrap())
        });

        let mut patch = diff_result?;
        patch.push_str(&untracked_result?);
        return Ok(patch);
    }

    // When head_ref is set, just run git diff (no untracked files)
    let output = git_cmd()
        .args([
            "diff",
            "--no-color",
            "-U3",
            "--find-renames",
            "--no-ext-diff",
            &diff_range,
        ])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

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

    // --- get_combined_patch tests ---

    fn git(path: &std::path::Path, args: &[&str]) {
        std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap();
    }

    fn configure_git_identity(path: &std::path::Path) {
        git(path, &["config", "user.email", "test@test.com"]);
        git(path, &["config", "user.name", "Test"]);
    }

    fn init_repo_for_combined_patch() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-b", "main"]);
        configure_git_identity(p);
        std::fs::write(p.join("file_a.txt"), "hello\n").unwrap();
        std::fs::write(p.join("file_b.txt"), "world\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "init"]);
        git(p, &["checkout", "-b", "feat"]);
        std::fs::write(p.join("file_a.txt"), "hello\nmodified a\n").unwrap();
        std::fs::write(p.join("file_b.txt"), "world\nmodified b\n").unwrap();
        std::fs::write(p.join("file_c.txt"), "brand new\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "feature work"]);
        dir
    }

    #[test]
    fn get_combined_patch_returns_all_file_diffs_in_one_string() {
        let repo = init_repo_for_combined_patch();
        let patch = get_combined_patch(repo.path().to_str().unwrap(), "main", None).unwrap();

        // Should contain diff headers for all three files
        assert!(
            patch.contains("diff --git a/file_a.txt b/file_a.txt"),
            "patch should contain file_a diff: {}",
            patch
        );
        assert!(
            patch.contains("diff --git a/file_b.txt b/file_b.txt"),
            "patch should contain file_b diff: {}",
            patch
        );
        assert!(
            patch.contains("diff --git a/file_c.txt b/file_c.txt"),
            "patch should contain file_c diff: {}",
            patch
        );

        // Should contain actual changes
        assert!(patch.contains("+modified a"), "patch: {}", patch);
        assert!(patch.contains("+modified b"), "patch: {}", patch);
        assert!(patch.contains("+brand new"), "patch: {}", patch);
    }

    #[test]
    fn get_combined_patch_includes_untracked_files_when_comparing_to_working_tree() {
        let repo = init_repo_for_combined_patch();
        // Add an untracked file (not staged, not committed)
        std::fs::write(repo.path().join("untracked.txt"), "line1\nline2\n").unwrap();

        let patch = get_combined_patch(repo.path().to_str().unwrap(), "main", None).unwrap();

        // Should include a synthetic diff for the untracked file
        assert!(
            patch.contains("diff --git a/untracked.txt b/untracked.txt"),
            "patch should contain untracked file diff header: {}",
            patch
        );
        assert!(
            patch.contains("+line1"),
            "patch should contain untracked file content: {}",
            patch
        );
        assert!(
            patch.contains("+line2"),
            "patch should contain untracked file content: {}",
            patch
        );
        // Should still contain the tracked diffs
        assert!(
            patch.contains("diff --git a/file_a.txt b/file_a.txt"),
            "patch: {}",
            patch
        );
    }

    #[test]
    fn get_combined_patch_excludes_untracked_when_head_ref_is_set() {
        let repo = init_repo_for_combined_patch();
        std::fs::write(repo.path().join("untracked.txt"), "should not appear\n").unwrap();

        let patch =
            get_combined_patch(repo.path().to_str().unwrap(), "main", Some("HEAD")).unwrap();

        assert!(
            !patch.contains("untracked.txt"),
            "patch should NOT contain untracked files when head_ref is set: {}",
            patch
        );
    }
}
