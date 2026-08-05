use std::process::Command;

use crate::command::{augmented_path, no_window};

pub mod branch;
pub mod commits;
pub mod diff;

#[cfg(test)]
pub(crate) mod test_util;

// Re-export all public items for backward compatibility
pub use branch::*;
pub use commits::*;
pub use diff::*;

/// Context for running git commands. When `wsl` is set, commands run inside
/// the specified WSL distro via `wsl.exe`.
#[derive(Debug, Clone, Default)]
pub struct GitContext {
    /// When set, git commands run inside this WSL distro.
    pub wsl_distro: Option<String>,
}

impl GitContext {
    /// Create a context for running git natively (no WSL).
    pub fn native() -> Self {
        Self { wsl_distro: None }
    }

    /// Create a context for running git inside a WSL distro.
    pub fn wsl(distro: &str) -> Self {
        Self {
            wsl_distro: Some(distro.to_string()),
        }
    }
}

/// Create a `git` Command with CREATE_NO_WINDOW on Windows and an augmented
/// PATH that includes conventional developer directories (e.g. /opt/homebrew/bin,
/// ~/.local/bin) so git can be found when launched from a GUI app.
fn git_cmd() -> Command {
    let mut cmd = Command::new("git");
    no_window(&mut cmd);
    cmd.env("PATH", augmented_path(&[]));
    cmd
}

/// Create a `git` Command that respects the given context.
///
/// When `ctx.wsl_distro` is set (Windows only), the command becomes:
/// `wsl.exe -d <distro> -- git <args...>`
///
/// On non-Windows, the WSL context is ignored and a native git command is returned.
pub fn git_cmd_in(ctx: &GitContext) -> Command {
    match &ctx.wsl_distro {
        #[cfg(windows)]
        Some(distro) => {
            let mut cmd = Command::new("wsl.exe");
            cmd.args(["-d", distro, "--", "git"]);
            no_window(&mut cmd);
            cmd
        }
        #[cfg(not(windows))]
        Some(_) => git_cmd(), // WSL not applicable on non-Windows
        None => git_cmd(),
    }
}

/// Run a git command inside a specific directory with WSL context.
///
/// When WSL is active, `cwd` should be a Linux path (e.g. `/home/user/project`).
/// The `--cd` flag is NOT used here — instead, `current_dir` is set on the command.
/// For WSL, this means `wsl.exe` inherits the CWD, and git operates relative to it.
///
/// For WSL paths: callers should use `wsl.exe -d <distro> --cd <linux_path> -- git ...`
/// which we handle by setting cwd on the wrapping command for non-WSL,
/// and for WSL by inserting `--cd` before the `--` separator.
pub fn git_cmd_in_dir(ctx: &GitContext, cwd: &str) -> Command {
    match &ctx.wsl_distro {
        #[cfg(windows)]
        Some(distro) => {
            let mut cmd = Command::new("wsl.exe");
            cmd.args(["-d", distro, "--cd", cwd, "--", "git"]);
            no_window(&mut cmd);
            cmd
        }
        #[cfg(not(windows))]
        Some(_) => {
            let mut cmd = git_cmd();
            cmd.current_dir(cwd);
            cmd
        }
        None => {
            let mut cmd = git_cmd();
            cmd.current_dir(cwd);
            cmd
        }
    }
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
    use std::collections::HashSet;
    use std::fs;
    use std::process::Command;

    use super::test_util::{configure_git_identity, git};

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
        let remote_dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--bare", "-b", "main"])
            .current_dir(remote_dir.path())
            .output()
            .unwrap();

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
        Command::new("git")
            .args(["checkout", "-b", "feat"])
            .current_dir(p)
            .output()
            .unwrap();
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
        git(p, &["init", "-b", "main"]);
        configure_git_identity(p);
        fs::write(p.join("doomed.txt"), "will be deleted\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "init"]);
        git(p, &["checkout", "-b", "feat"]);
        fs::remove_file(p.join("doomed.txt")).unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "delete file"]);

        let diff = get_file_diff(p.to_str().unwrap(), "main", "doomed.txt", None, None).unwrap();
        assert_eq!(diff.original, "will be deleted\n");
        assert_eq!(diff.modified, "");
    }

    #[test]
    fn get_changed_files_detects_renamed_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-b", "main"]);
        configure_git_identity(p);
        fs::create_dir_all(p.join("src/client")).unwrap();
        fs::write(p.join("src/client/auth.rs"), "fn auth() {}\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "init"]);
        git(p, &["checkout", "-b", "feat"]);
        fs::create_dir_all(p.join("crates/client/src")).unwrap();
        git(
            p,
            &["mv", "src/client/auth.rs", "crates/client/src/auth.rs"],
        );
        git(p, &["commit", "-m", "rename"]);

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
        git(p, &["init", "-b", "main"]);
        configure_git_identity(p);
        fs::create_dir_all(p.join("src")).unwrap();
        fs::write(p.join("src/lib.rs"), "original content\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "init"]);
        git(p, &["checkout", "-b", "feat"]);
        fs::create_dir_all(p.join("crates")).unwrap();
        git(p, &["mv", "src/lib.rs", "crates/lib.rs"]);
        fs::write(p.join("crates/lib.rs"), "modified content\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "rename+edit"]);

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
    fn list_branches_deduplicates_remotes() {
        let remote1 = tempfile::tempdir().unwrap();
        git(remote1.path(), &["init", "--bare", "-b", "main"]);
        let remote2 = tempfile::tempdir().unwrap();
        git(remote2.path(), &["init", "--bare", "-b", "main"]);

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
        git(p, &["checkout", "-b", "feat"]);
        fs::write(p.join("file.txt"), "v2\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "second"]);
        fs::write(p.join("file.txt"), "v3\n").unwrap();
        fs::write(p.join("new.txt"), "hello\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-m", "third"]);
        fs::write(p.join("uncommitted.txt"), "wip\n").unwrap();

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
        git(p, &["init", "-b", "master"]);
        configure_git_identity(p);
        git(p, &["commit", "--allow-empty", "-m", "init"]);
        let result = detect_default_branch(p.to_str().unwrap()).unwrap();
        assert_eq!(result, "master");
    }
}
