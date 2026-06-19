//! Tests for WorktreeService and WorktreeMode shared logic.

use std::path::PathBuf;
use std::process::Command;

use planeai_core::services::*;

fn test_db() -> rusqlite::Connection {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = open_db_at(&path).unwrap();
    std::mem::forget(dir);
    conn
}

fn configure_git(path: &std::path::Path) {
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

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    configure_git(dir.path());
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    dir
}

// ─── Worktree root calculation ───────────────────────────────────────────────

#[test]
fn worktree_root_uses_home_dir() {
    let root = WorktreeService::worktree_root("myproject");
    let home = std::env::var("HOME").unwrap();
    assert_eq!(
        root,
        PathBuf::from(home).join(".planeai/worktrees/myproject")
    );
}

#[test]
fn worktree_root_sanitizes_with_project_name() {
    let root = WorktreeService::worktree_root("my-project");
    assert!(root.to_string_lossy().contains("my-project"));
}

// ─── Worktree path calculation ───────────────────────────────────────────────

#[test]
fn worktree_path_is_root_plus_short_id() {
    let path = WorktreeService::worktree_path("proj", "abcd1234");
    let root = WorktreeService::worktree_root("proj");
    assert_eq!(path, root.join("abcd1234"));
}

// ─── Short ID derivation ─────────────────────────────────────────────────────

#[test]
fn short_id_extracts_first_8_hex_chars() {
    let sid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    let short = WorktreeService::short_id(sid);
    assert_eq!(short, "a1b2c3d4");
}

#[test]
fn short_id_strips_dashes() {
    let sid = "12345678-9abc-def0-1234-567890abcdef";
    let short = WorktreeService::short_id(sid);
    // "123456789abcdef012345678..." first 8 = "12345678"
    assert_eq!(short.len(), 8);
    assert!(!short.contains('-'));
}

// ─── Branch name generation ──────────────────────────────────────────────────

#[test]
fn branch_name_lowercases_and_joins_short_id() {
    let name = WorktreeService::branch_name("PLA-42", "abcd1234");
    assert_eq!(name, "pla-42/abcd1234");
}

#[test]
fn branch_name_replaces_spaces_with_dashes() {
    let name = WorktreeService::branch_name("My Task", "12345678");
    assert_eq!(name, "my-task/12345678");
}

// ─── Branch name validation ──────────────────────────────────────────────────

#[test]
fn validate_branch_name_accepts_valid() {
    assert!(WorktreeService::validate_branch_name("feat/my-feature").is_ok());
    assert!(WorktreeService::validate_branch_name("fix-bug-123").is_ok());
    assert!(WorktreeService::validate_branch_name("pla-42/abcd1234").is_ok());
}

#[test]
fn validate_branch_name_rejects_empty() {
    let err = WorktreeService::validate_branch_name("").unwrap_err();
    assert!(err.contains("empty"));
}

#[test]
fn validate_branch_name_rejects_double_dot() {
    let err = WorktreeService::validate_branch_name("feat..bar").unwrap_err();
    assert!(err.contains(".."));
}

#[test]
fn validate_branch_name_rejects_spaces() {
    let err = WorktreeService::validate_branch_name("feat bar").unwrap_err();
    assert!(err.contains("spaces"));
}

#[test]
fn validate_branch_name_rejects_control_chars() {
    let err = WorktreeService::validate_branch_name("feat\x00bar").unwrap_err();
    assert!(err.contains("control"));
}

#[test]
fn validate_branch_name_rejects_leading_dash() {
    let err = WorktreeService::validate_branch_name("-feature").unwrap_err();
    assert!(err.contains("'-'"));
}

#[test]
fn validate_branch_name_rejects_trailing_dot() {
    let err = WorktreeService::validate_branch_name("feature.").unwrap_err();
    assert!(err.contains("'.'"));
}

#[test]
fn validate_branch_name_rejects_trailing_slash() {
    let err = WorktreeService::validate_branch_name("feature/").unwrap_err();
    assert!(err.contains("'/'"));
}

// ─── Existing worktree detection ─────────────────────────────────────────────

#[test]
fn worktree_exists_returns_false_for_nonexistent() {
    assert!(!WorktreeService::worktree_exists(
        "nonexistent_project_xyz",
        "deadbeef"
    ));
}

// ─── Session record stores worktree fields ───────────────────────────────────

#[test]
fn session_record_stores_worktree_path() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/test-wt-session").unwrap();
    let params = CreateSessionParams {
        id: "sess-wt-1".to_string(),
        project_id: project.id.clone(),
        name: "worktree session".to_string(),
        backend: "daemon".to_string(),
        branch: "feat/branch".to_string(),
        worktree_path: Some("/tmp/wt/path".to_string()),
        task_key: Some("PLA-99".to_string()),
        base_branch: Some("main".to_string()),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();
    let rec = SessionService::get(&conn, "sess-wt-1").unwrap().unwrap();
    assert_eq!(rec.worktree_path, Some("/tmp/wt/path".to_string()));
    assert_eq!(rec.branch, "feat/branch");
    assert_eq!(rec.task_key, Some("PLA-99".to_string()));
    assert_eq!(rec.base_branch, Some("main".to_string()));
}

#[test]
fn session_record_stores_branch_name() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/test-branch-rec").unwrap();
    let params = CreateSessionParams {
        id: "sess-br-1".to_string(),
        project_id: project.id.clone(),
        name: "branch session".to_string(),
        backend: "daemon".to_string(),
        branch: "pla-42/deadbeef".to_string(),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();
    let rec = SessionService::get(&conn, "sess-br-1").unwrap().unwrap();
    assert_eq!(rec.branch, "pla-42/deadbeef");
}

// ─── WorktreeMode::None resolves to project cwd ─────────────────────────────

#[test]
fn resolve_worktree_none_uses_project_path() {
    let dir = tempfile::tempdir().unwrap();
    let result = WorktreeService::resolve_worktree(
        &WorktreeMode::None,
        "proj",
        dir.path(),
        "00000000-0000-0000-0000-000000000000",
        "main",
    )
    .unwrap();
    assert_eq!(result.cwd, dir.path().to_path_buf());
    assert!(result.worktree_path.is_none());
    assert!(result.branch_name.is_empty());
}

// ─── WorktreeMode::Existing resolves to given path ──────────────────────────

#[test]
fn resolve_worktree_existing_uses_given_path() {
    let dir = tempfile::tempdir().unwrap();
    let result = WorktreeService::resolve_worktree(
        &WorktreeMode::Existing {
            path: dir.path().to_path_buf(),
            branch_name: Some("feat/existing".to_string()),
        },
        "proj",
        dir.path(),
        "00000000-0000-0000-0000-000000000000",
        "main",
    )
    .unwrap();
    assert_eq!(result.cwd, dir.path().to_path_buf());
    assert_eq!(result.branch_name, "feat/existing");
    assert!(result.worktree_path.is_some());
}

#[test]
fn resolve_worktree_existing_fails_for_missing_path() {
    let dir = tempfile::tempdir().unwrap();
    let err = WorktreeService::resolve_worktree(
        &WorktreeMode::Existing {
            path: PathBuf::from("/nonexistent/path/xyz"),
            branch_name: None,
        },
        "proj",
        dir.path(),
        "00000000-0000-0000-0000-000000000000",
        "main",
    )
    .unwrap_err();
    assert!(err.contains("does not exist"));
}

// ─── WorktreeMode::Create creates a worktree ────────────────────────────────

#[test]
fn resolve_worktree_create_makes_worktree_on_disk() {
    let repo = init_repo();
    // Use a unique project name to avoid path collisions between test runs
    let unique = format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    );
    let session_id = format!("{unique}12-3456-7890-abcd-ef1234567890");
    let project_name = format!("test-wt-{}", &unique);
    let result = WorktreeService::resolve_worktree(
        &WorktreeMode::Create {
            base_project_path: repo.path().to_path_buf(),
            branch_name: "feat/test-wt".to_string(),
            task_key: None,
        },
        &project_name,
        repo.path(),
        &session_id,
        "main",
    )
    .unwrap();
    // Should have created the worktree directory
    assert!(result.cwd.is_dir());
    assert!(result.worktree_path.is_some());
    assert_eq!(result.branch_name, "feat/test-wt");
    assert_eq!(result.base_branch, Some("main".to_string()));
    // Cleanup
    let _ = std::fs::remove_dir_all(&result.cwd);
}

#[test]
fn resolve_worktree_create_rejects_invalid_branch() {
    let repo = init_repo();
    let err = WorktreeService::resolve_worktree(
        &WorktreeMode::Create {
            base_project_path: repo.path().to_path_buf(),
            branch_name: "".to_string(),
            task_key: None,
        },
        "proj",
        repo.path(),
        "00000000-0000-0000-0000-000000000000",
        "main",
    )
    .unwrap_err();
    assert!(err.contains("empty"));
}

// ─── Tauri and Iced produce same worktree path ──────────────────────────────

#[test]
fn tauri_and_iced_worktree_paths_match() {
    let session_id = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    let project_name = "myproject";

    // Iced path via WorktreeService
    let short_id = WorktreeService::short_id(session_id);
    let iced_path = WorktreeService::worktree_path(project_name, &short_id);

    // Tauri/production path (same logic: worktree_root / project_name / short_id)
    let prod_short_id = &session_id.replace('-', "")[..8];
    let home = std::env::var("HOME").unwrap();
    let prod_path = PathBuf::from(&home)
        .join(".planeai/worktrees")
        .join(project_name)
        .join(prod_short_id);

    assert_eq!(iced_path, prod_path);
    assert_eq!(short_id, prod_short_id);
}

// ─── Tauri and Iced produce same branch name for auto-dispatch ──────────────

#[test]
fn tauri_and_iced_branch_names_match() {
    let session_id = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    let task_key = "PLA-42";

    // Iced
    let short_id = WorktreeService::short_id(session_id);
    let iced_branch = WorktreeService::branch_name(task_key, &short_id);

    // Production (session.rs dispatch)
    let prod_short_id = &session_id.replace('-', "")[..8];
    let prod_branch = format!(
        "{}/{}",
        task_key.to_lowercase().replace(' ', "-"),
        prod_short_id
    );

    assert_eq!(iced_branch, prod_branch);
}

// ─── Worktree creation failure prevents daemon spawn ─────────────────────────

#[test]
fn worktree_creation_failure_is_returned_as_error() {
    let dir = tempfile::tempdir().unwrap();
    // Not a git repo — worktree_add should fail
    let err = WorktreeService::resolve_worktree(
        &WorktreeMode::Create {
            base_project_path: dir.path().to_path_buf(),
            branch_name: "feat/will-fail".to_string(),
            task_key: None,
        },
        "proj",
        dir.path(),
        "00000000-0000-0000-0000-000000000000",
        "main",
    )
    .unwrap_err();
    // Should contain git error
    assert!(!err.is_empty());
}

// ─── DB persist failure prevents daemon spawn ────────────────────────────────

#[test]
fn db_persist_failure_blocks_session_creation() {
    let conn = test_db();
    // Create a project
    let project = ProjectService::ensure_project(&conn, "/tmp/test-persist-fail").unwrap();

    // Insert a session to cause a PRIMARY KEY conflict on re-insert
    let params = CreateSessionParams {
        id: "duplicate-id".to_string(),
        project_id: project.id.clone(),
        name: "first".to_string(),
        backend: "daemon".to_string(),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();

    // Second insert with same id should fail
    let params2 = CreateSessionParams {
        id: "duplicate-id".to_string(),
        project_id: project.id.clone(),
        name: "second".to_string(),
        backend: "daemon".to_string(),
        ..Default::default()
    };
    let err = SessionService::create(&conn, &params2);
    assert!(err.is_err());
}

// ─── Daemon spawn failure marks session destroyed ────────────────────────────

#[test]
fn marking_session_destroyed_after_spawn_failure() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/test-destroy-mark").unwrap();
    let params = CreateSessionParams {
        id: "sess-spawn-fail".to_string(),
        project_id: project.id.clone(),
        name: "will-fail-spawn".to_string(),
        backend: "daemon".to_string(),
        worktree_path: Some("/tmp/wt".to_string()),
        branch: "feat/x".to_string(),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();

    // Simulate spawn failure — mark destroyed
    SessionService::set_status(&conn, "sess-spawn-fail", "destroyed").unwrap();
    let rec = SessionService::get(&conn, "sess-spawn-fail")
        .unwrap()
        .unwrap();
    assert_eq!(rec.status, "destroyed");
}

// ─── has_active_checkout ─────────────────────────────────────────────────────

#[test]
fn has_active_checkout_detects_null_worktree_path() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/test-checkout-detect").unwrap();

    // No sessions → false
    assert!(!SessionService::has_active_checkout(&conn, &project.id).unwrap());

    // Worktree session → still false
    let params = CreateSessionParams {
        id: "sess-wt".to_string(),
        project_id: project.id.clone(),
        backend: "daemon".to_string(),
        worktree_path: Some("/tmp/wt".to_string()),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();
    assert!(!SessionService::has_active_checkout(&conn, &project.id).unwrap());

    // Checkout session (NULL worktree_path) → true
    let params2 = CreateSessionParams {
        id: "sess-checkout".to_string(),
        project_id: project.id.clone(),
        backend: "daemon".to_string(),
        ..Default::default()
    };
    SessionService::create(&conn, &params2).unwrap();
    assert!(SessionService::has_active_checkout(&conn, &project.id).unwrap());
}
