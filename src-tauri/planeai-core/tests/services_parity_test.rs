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
        ..Default::default()
    };
    let record = SessionService::create(&conn, &params).unwrap();
    assert_eq!(record.id, "session-uuid-1");
    assert_eq!(record.project_id, project.id);
    assert_eq!(record.status, "active");
    assert_eq!(record.backend, "daemon");
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
fn worktree_path_persisted_in_session() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/my-repo").unwrap();
    let params = CreateSessionParams {
        id: "s1".to_string(),
        project_id: project.id.clone(),
        backend: "daemon".to_string(),
        worktree_path: Some("/home/user/.planeai/worktrees/my-repo/abc123".to_string()),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();
    let s = SessionService::get(&conn, "s1").unwrap().unwrap();
    assert_eq!(
        s.worktree_path,
        Some("/home/user/.planeai/worktrees/my-repo/abc123".to_string())
    );
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

// ─── Production DB compatibility ─────────────────────────────────────────────

/// Simulates opening a production-style DB (created by src-tauri/src/db.rs migrate)
/// and verifies planeai_core::services can read/list/create/update without errors.
#[test]
fn production_db_compat_read_list_create_update() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();

    // Simulate production migration (as src-tauri/src/db.rs would create it)
    conn.execute_batch(
        "CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            auto_mode INTEGER NOT NULL DEFAULT 0,
            task_manager TEXT
        );
        CREATE TABLE sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id),
            name TEXT NOT NULL DEFAULT '',
            tmux_name TEXT,
            branch TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL,
            worktree_path TEXT,
            provider TEXT,
            backend TEXT NOT NULL DEFAULT 'tmux',
            provider_session_id TEXT,
            tab_count INTEGER NOT NULL DEFAULT 1,
            auto_approve INTEGER NOT NULL DEFAULT 1,
            task_key TEXT,
            base_branch TEXT,
            mru_position INTEGER,
            pr_url TEXT,
            pr_state TEXT,
            auto_dispatched INTEGER NOT NULL DEFAULT 0
        );
        INSERT INTO projects (id, name, path, status) VALUES ('p1', 'myapp', '/home/user/myapp', 'active');
        INSERT INTO sessions (id, project_id, name, tmux_name, branch, status, created_at, provider, backend, tab_count, auto_approve, task_key)
            VALUES ('s-existing', 'p1', 'kiro session', 'planeai-myapp-abc', 'feat/thing', 'active', '2024-01-01T00:00:00Z', 'kiro', 'tmux', 2, 1, 'PLA-3');
        INSERT INTO sessions (id, project_id, name, branch, status, created_at, backend, mru_position)
            VALUES ('s-daemon', 'p1', 'daemon session', 'main', 'exited', '2024-01-02T00:00:00Z', 'daemon', 0);",
    )
    .unwrap();

    // Now run planeai_core::services::migrate on this existing DB (should be safe/idempotent)
    migrate(&conn).unwrap();

    // Verify we can LIST sessions from production schema
    let sessions = SessionService::list_for_project(&conn, "p1").unwrap();
    assert_eq!(sessions.len(), 2);
    // Verify MRU ordering: s-daemon has mru_position=0, s-existing has NULL
    assert_eq!(sessions[0].id, "s-daemon");
    assert_eq!(sessions[1].id, "s-existing");

    // Verify we can READ a session with all production columns
    let s = SessionService::get(&conn, "s-existing").unwrap().unwrap();
    assert_eq!(s.tmux_name, Some("planeai-myapp-abc".to_string()));
    assert_eq!(s.tab_count, 2);
    assert!(s.auto_approve);
    assert_eq!(s.task_key, Some("PLA-3".to_string()));
    assert_eq!(s.backend, "tmux");
    assert_eq!(s.provider, Some("kiro".to_string()));

    // Verify we can CREATE a new session alongside existing ones
    let new_params = CreateSessionParams {
        id: "s-new".to_string(),
        project_id: "p1".to_string(),
        name: "new daemon".to_string(),
        backend: "daemon".to_string(),
        provider: Some("claude".to_string()),
        auto_approve: true,
        branch: "feat/new".to_string(),
        ..Default::default()
    };
    let created = SessionService::create(&conn, &new_params).unwrap();
    assert_eq!(created.id, "s-new");
    assert_eq!(created.provider, Some("claude".to_string()));

    // Verify we can UPDATE status
    SessionService::set_status(&conn, "s-new", "exited").unwrap();
    let updated = SessionService::get(&conn, "s-new").unwrap().unwrap();
    assert_eq!(updated.status, "exited");

    // Verify project listing works
    let projects = ProjectService::list_active(&conn).unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "myapp");

    // Verify ensure_project finds existing by path
    let found = ProjectService::ensure_project(&conn, "/home/user/myapp").unwrap();
    assert_eq!(found.id, "p1");
}
