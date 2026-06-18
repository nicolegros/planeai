//! Tests for shared domain services — verify both Iced and Tauri launch paths
//! create equivalent session records via planeai_core::services.

use planeai_core::services::*;
use std::path::PathBuf;

fn test_db() -> rusqlite::Connection {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = open_db_at(&path).unwrap();
    // Keep dir alive by leaking — tests are short-lived
    std::mem::forget(dir);
    conn
}

// ─── ProjectService ──────────────────────────────────────────────────────────

#[test]
fn ensure_project_creates_new() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/my-project").unwrap();
    assert_eq!(project.name, "my-project");
    assert_eq!(project.path, "/tmp/my-project");
    assert_eq!(project.status, "active");
    assert!(!project.id.is_empty());
}

#[test]
fn ensure_project_returns_existing() {
    let conn = test_db();
    let p1 = ProjectService::ensure_project(&conn, "/tmp/my-project").unwrap();
    let p2 = ProjectService::ensure_project(&conn, "/tmp/my-project").unwrap();
    assert_eq!(p1.id, p2.id);
}

#[test]
fn list_active_projects() {
    let conn = test_db();
    ProjectService::ensure_project(&conn, "/tmp/proj-a").unwrap();
    ProjectService::ensure_project(&conn, "/tmp/proj-b").unwrap();
    let projects = ProjectService::list_active(&conn).unwrap();
    assert_eq!(projects.len(), 2);
}

#[test]
fn get_project_by_path() {
    let conn = test_db();
    let p = ProjectService::ensure_project(&conn, "/tmp/findme").unwrap();
    let found = ProjectService::get_by_path(&conn, "/tmp/findme")
        .unwrap()
        .unwrap();
    assert_eq!(found.id, p.id);
    assert!(ProjectService::get_by_path(&conn, "/tmp/nonexistent")
        .unwrap()
        .is_none());
}

// ─── SessionService ──────────────────────────────────────────────────────────

#[test]
fn create_session_record() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/project").unwrap();
    let params = CreateSessionParams {
        id: "session-uuid-1".to_string(),
        project_id: project.id.clone(),
        name: "kiro".to_string(),
        backend: "daemon".to_string(),
        auto_approve: true,
        command: Some("kiro-cli chat".to_string()),
        cwd: Some("/tmp/project".to_string()),
        ..Default::default()
    };
    let record = SessionService::create(&conn, &params).unwrap();
    assert_eq!(record.id, "session-uuid-1");
    assert_eq!(record.project_id, project.id);
    assert_eq!(record.status, "active");
    assert_eq!(record.backend, "daemon");
    assert_eq!(record.command, Some("kiro-cli chat".to_string()));
    assert_eq!(record.cwd, Some("/tmp/project".to_string()));
}

#[test]
fn iced_and_tauri_create_equivalent_records() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/project").unwrap();

    // Simulates Iced-style launch (daemon, no branch/worktree)
    let iced_params = CreateSessionParams {
        id: "iced-uuid-1".to_string(),
        project_id: project.id.clone(),
        name: "kiro".to_string(),
        backend: "daemon".to_string(),
        auto_approve: true,
        command: Some("kiro-cli chat".to_string()),
        cwd: Some("/tmp/project".to_string()),
        ..Default::default()
    };

    // Simulates Tauri-style launch (daemon, with branch and worktree)
    let tauri_params = CreateSessionParams {
        id: "tauri-uuid-1".to_string(),
        project_id: project.id.clone(),
        name: "PLA-5: fix bug".to_string(),
        branch: "pla-5/abcd1234".to_string(),
        worktree_path: Some("/Users/me/.planeai/worktrees/project/abcd1234".to_string()),
        provider: Some("kiro".to_string()),
        backend: "daemon".to_string(),
        auto_approve: true,
        task_key: Some("PLA-5".to_string()),
        base_branch: Some("main".to_string()),
        command: Some("kiro-cli chat --trust-all-tools".to_string()),
        cwd: Some("/Users/me/.planeai/worktrees/project/abcd1234".to_string()),
        ..Default::default()
    };

    let iced_record = SessionService::create(&conn, &iced_params).unwrap();
    let tauri_record = SessionService::create(&conn, &tauri_params).unwrap();

    // Both are visible in the same project session list
    let sessions = SessionService::list_for_project(&conn, &project.id).unwrap();
    assert_eq!(sessions.len(), 2);
    assert!(sessions.iter().any(|s| s.id == "iced-uuid-1"));
    assert!(sessions.iter().any(|s| s.id == "tauri-uuid-1"));

    // Both have the required fields
    assert_eq!(iced_record.status, "active");
    assert_eq!(tauri_record.status, "active");
    assert_eq!(tauri_record.task_key, Some("PLA-5".to_string()));
    assert_eq!(tauri_record.branch, "pla-5/abcd1234");
}

#[test]
fn session_status_transitions() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/project").unwrap();
    let params = CreateSessionParams {
        id: "s1".to_string(),
        project_id: project.id.clone(),
        backend: "daemon".to_string(),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();

    // active -> exited
    SessionService::set_status(&conn, "s1", "exited").unwrap();
    let s = SessionService::get(&conn, "s1").unwrap().unwrap();
    assert_eq!(s.status, "exited");

    // exited -> active (restart)
    SessionService::set_status(&conn, "s1", "active").unwrap();
    let s = SessionService::get(&conn, "s1").unwrap().unwrap();
    assert_eq!(s.status, "active");

    // active -> destroyed (kill)
    SessionService::set_status(&conn, "s1", "destroyed").unwrap();
    let s = SessionService::get(&conn, "s1").unwrap().unwrap();
    assert_eq!(s.status, "destroyed");
}

#[test]
fn destroyed_sessions_excluded_from_active_list() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/project").unwrap();

    // Create two sessions
    SessionService::create(
        &conn,
        &CreateSessionParams {
            id: "s-active".to_string(),
            project_id: project.id.clone(),
            backend: "daemon".to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    SessionService::create(
        &conn,
        &CreateSessionParams {
            id: "s-destroyed".to_string(),
            project_id: project.id.clone(),
            backend: "daemon".to_string(),
            ..Default::default()
        },
    )
    .unwrap();

    SessionService::set_status(&conn, "s-destroyed", "destroyed").unwrap();

    let sessions = SessionService::list_for_project(&conn, &project.id).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "s-active");
}

#[test]
fn project_cwd_persisted_in_session() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/my-repo").unwrap();
    let params = CreateSessionParams {
        id: "s1".to_string(),
        project_id: project.id.clone(),
        backend: "daemon".to_string(),
        cwd: Some("/tmp/my-repo".to_string()),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();
    let s = SessionService::get(&conn, "s1").unwrap().unwrap();
    assert_eq!(s.cwd, Some("/tmp/my-repo".to_string()));
}

#[test]
fn daemon_session_id_is_the_record_id() {
    // In the shared model, the session_id passed to the daemon IS the DB record id.
    // No separate mapping needed.
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/project").unwrap();
    let session_id = uuid::Uuid::new_v4().to_string();
    let params = CreateSessionParams {
        id: session_id.clone(),
        project_id: project.id.clone(),
        backend: "daemon".to_string(),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();
    let s = SessionService::get(&conn, &session_id).unwrap().unwrap();
    assert_eq!(s.id, session_id);
}

#[test]
fn durable_log_path_linked_to_session() {
    // Verify the convention: log dir / sessions / {session_id}
    std::env::set_var("PLANEAI_SESSION_LOG_DIR", "/tmp/test-logs");
    let dir = SessionService::durable_log_dir("abc-123").unwrap();
    assert_eq!(dir, PathBuf::from("/tmp/test-logs/sessions/abc-123"));
    std::env::remove_var("PLANEAI_SESSION_LOG_DIR");
}

#[test]
fn tmux_remains_explicit_not_default() {
    // The services module always uses "daemon" as default backend.
    // tmux must be explicitly set.
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/project").unwrap();
    let params = CreateSessionParams {
        id: "s-daemon".to_string(),
        project_id: project.id.clone(),
        backend: "daemon".to_string(),
        ..Default::default()
    };
    let record = SessionService::create(&conn, &params).unwrap();
    assert_eq!(record.backend, "daemon");

    // Only explicit tmux sessions get tmux backend
    let params_tmux = CreateSessionParams {
        id: "s-tmux".to_string(),
        project_id: project.id.clone(),
        backend: "tmux".to_string(),
        ..Default::default()
    };
    let record_tmux = SessionService::create(&conn, &params_tmux).unwrap();
    assert_eq!(record_tmux.backend, "tmux");
}

// ─── WorktreeService ─────────────────────────────────────────────────────────

#[test]
fn worktree_path_uses_shared_convention() {
    let path = WorktreeService::worktree_path("myproject", "abcd1234");
    let home = std::env::var("HOME").unwrap();
    assert_eq!(
        path,
        PathBuf::from(format!("{home}/.planeai/worktrees/myproject/abcd1234"))
    );
}

#[test]
fn branch_name_from_task_key() {
    let name = WorktreeService::branch_name("PLA-5", "abcd1234");
    assert_eq!(name, "pla-5/abcd1234");
}

// ─── TaskService ─────────────────────────────────────────────────────────────

#[test]
fn session_task_key_returns_none_without_task() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/project").unwrap();
    SessionService::create(
        &conn,
        &CreateSessionParams {
            id: "s1".to_string(),
            project_id: project.id.clone(),
            backend: "daemon".to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let key = TaskService::session_task_key(&conn, "s1").unwrap();
    assert_eq!(key, None);
}

#[test]
fn session_task_key_returns_linked_task() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/project").unwrap();
    SessionService::create(
        &conn,
        &CreateSessionParams {
            id: "s2".to_string(),
            project_id: project.id.clone(),
            backend: "daemon".to_string(),
            task_key: Some("PLA-10".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    let key = TaskService::session_task_key(&conn, "s2").unwrap();
    assert_eq!(key, Some("PLA-10".to_string()));
}
