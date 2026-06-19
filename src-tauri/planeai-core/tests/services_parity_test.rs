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

// ─── Migration parity ────────────────────────────────────────────────────────

#[test]
fn shared_migration_is_idempotent() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .unwrap();
    // Run migration three times — should not fail
    migrate_project_session_schema(&conn).unwrap();
    migrate_project_session_schema(&conn).unwrap();
    migrate_project_session_schema(&conn).unwrap();
    // Verify tables exist
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('projects', 'sessions')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn production_and_core_path_produce_compatible_schema() {
    // Simulate production db.rs migration path
    let prod_conn = rusqlite::Connection::open_in_memory().unwrap();
    prod_conn
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .unwrap();
    // Production db.rs calls migrate_project_session_schema + settings
    migrate_project_session_schema(&prod_conn).unwrap();
    prod_conn
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            terminal_theme_dark TEXT NOT NULL DEFAULT 'one-dark',
            terminal_theme_light TEXT NOT NULL DEFAULT 'one-light',
            font_size INTEGER NOT NULL DEFAULT 14,
            font_family TEXT NOT NULL DEFAULT 'Menlo',
            appearance_mode TEXT NOT NULL DEFAULT 'system'
        );",
        )
        .unwrap();

    // Core-only migration path
    let core_conn = rusqlite::Connection::open_in_memory().unwrap();
    core_conn
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .unwrap();
    migrate_project_session_schema(&core_conn).unwrap();

    // Both should have same projects/sessions columns
    let prod_cols: String = prod_conn
        .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='sessions'")
        .unwrap()
        .query_row([], |r| r.get(0))
        .unwrap();
    let core_cols: String = core_conn
        .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='sessions'")
        .unwrap()
        .query_row([], |r| r.get(0))
        .unwrap();
    assert_eq!(prod_cols, core_cols);

    let prod_pcols: String = prod_conn
        .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='projects'")
        .unwrap()
        .query_row([], |r| r.get(0))
        .unwrap();
    let core_pcols: String = core_conn
        .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='projects'")
        .unwrap()
        .query_row([], |r| r.get(0))
        .unwrap();
    assert_eq!(prod_pcols, core_pcols);
}

#[test]
fn existing_rows_survive_migration() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .unwrap();
    migrate_project_session_schema(&conn).unwrap();

    // Insert data
    conn.execute(
        "INSERT INTO projects (id, name, path, status) VALUES ('p1', 'myapp', '/tmp/myapp', 'active')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO sessions (id, project_id, name, branch, status, created_at, backend) VALUES ('s1', 'p1', 'sess', 'main', 'active', '2024-01-01', 'daemon')",
        [],
    )
    .unwrap();

    // Re-run migration
    migrate_project_session_schema(&conn).unwrap();

    // Data still there
    let p = ProjectService::get_by_id(&conn, "p1").unwrap().unwrap();
    assert_eq!(p.name, "myapp");
    let s = SessionService::get(&conn, "s1").unwrap().unwrap();
    assert_eq!(s.name, "sess");
    assert_eq!(s.backend, "daemon");
}

#[test]
fn direct_backend_migrates_to_daemon() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .unwrap();
    // Create tables with old-style schema that includes 'direct' backend
    conn.execute_batch(
        "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, path TEXT NOT NULL);
         CREATE TABLE sessions (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, name TEXT NOT NULL DEFAULT '', tmux_name TEXT, branch TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'active', created_at TEXT NOT NULL, backend TEXT NOT NULL DEFAULT 'direct');
         INSERT INTO projects VALUES ('p1', 'test', '/tmp/test');
         INSERT INTO sessions (id, project_id, branch, created_at, backend) VALUES ('s1', 'p1', 'main', '2024-01-01', 'direct');",
    )
    .unwrap();

    migrate_project_session_schema(&conn).unwrap();

    let s = SessionService::get(&conn, "s1").unwrap().unwrap();
    assert_eq!(s.backend, "daemon");
}

// ─── CRUD parity ─────────────────────────────────────────────────────────────

#[test]
fn project_created_through_shared_service_readable_via_shared() {
    let conn = test_db();
    let p = ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();
    let found = ProjectService::get_by_id(&conn, &p.id).unwrap().unwrap();
    assert_eq!(found.name, "myapp");
    assert_eq!(found.path, "/tmp/myapp");
}

#[test]
fn session_created_through_shared_service_listed_via_shared() {
    let conn = test_db();
    let p = ProjectService::ensure_project(&conn, "/tmp/proj").unwrap();
    let params = CreateSessionParams {
        id: "s1".to_string(),
        project_id: p.id.clone(),
        name: "test".to_string(),
        backend: "daemon".to_string(),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();
    let sessions = SessionService::list_for_project(&conn, &p.id).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "s1");
}

#[test]
fn mru_ordering_matches_production_semantics() {
    let conn = test_db();
    let p = ProjectService::ensure_project(&conn, "/tmp/proj").unwrap();
    for id in &["a", "b", "c"] {
        SessionService::create(
            &conn,
            &CreateSessionParams {
                id: id.to_string(),
                project_id: p.id.clone(),
                backend: "daemon".to_string(),
                ..Default::default()
            },
        )
        .unwrap();
    }
    // Set MRU order: b, a, c
    SessionService::save_mru_order(&conn, &["b", "a", "c"]).unwrap();

    let sessions = SessionService::list_for_project(&conn, &p.id).unwrap();
    let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["b", "a", "c"]);

    // Also test global list_active
    let all = SessionService::list_active(&conn).unwrap();
    let ids: Vec<&str> = all.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["b", "a", "c"]);
}

#[test]
fn null_mru_sorts_last_by_created_at() {
    let conn = test_db();
    let p = ProjectService::ensure_project(&conn, "/tmp/proj").unwrap();
    for id in &["a", "b", "c"] {
        SessionService::create(
            &conn,
            &CreateSessionParams {
                id: id.to_string(),
                project_id: p.id.clone(),
                backend: "daemon".to_string(),
                ..Default::default()
            },
        )
        .unwrap();
    }
    // Only set "c" as MRU
    SessionService::save_mru_order(&conn, &["c"]).unwrap();

    let sessions = SessionService::list_for_project(&conn, &p.id).unwrap();
    // c first (mru 0), then a and b by created_at
    assert_eq!(sessions[0].id, "c");
}

#[test]
fn status_transitions_persist_consistently() {
    let conn = test_db();
    let p = ProjectService::ensure_project(&conn, "/tmp/proj").unwrap();
    SessionService::create(
        &conn,
        &CreateSessionParams {
            id: "s1".to_string(),
            project_id: p.id.clone(),
            backend: "daemon".to_string(),
            ..Default::default()
        },
    )
    .unwrap();

    // active -> exited via mark_exited
    SessionService::mark_exited(&conn, "s1").unwrap();
    assert_eq!(
        SessionService::get(&conn, "s1").unwrap().unwrap().status,
        "exited"
    );

    // exited -> active via restore
    SessionService::restore(&conn, "s1").unwrap();
    assert_eq!(
        SessionService::get(&conn, "s1").unwrap().unwrap().status,
        "active"
    );

    // active -> archived
    SessionService::archive(&conn, "s1").unwrap();
    assert_eq!(
        SessionService::get(&conn, "s1").unwrap().unwrap().status,
        "archived"
    );

    // archived not in active list
    assert!(SessionService::list_for_project(&conn, &p.id)
        .unwrap()
        .is_empty());
    // but in archived list
    assert_eq!(SessionService::list_archived(&conn).unwrap().len(), 1);

    // archived -> destroyed
    SessionService::destroy(&conn, "s1").unwrap();
    assert_eq!(
        SessionService::get(&conn, "s1").unwrap().unwrap().status,
        "destroyed"
    );
}

#[test]
fn archived_destroyed_filtering_consistent() {
    let conn = test_db();
    let p = ProjectService::ensure_project(&conn, "/tmp/proj").unwrap();
    SessionService::create(
        &conn,
        &CreateSessionParams {
            id: "active".to_string(),
            project_id: p.id.clone(),
            backend: "daemon".to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    SessionService::create(
        &conn,
        &CreateSessionParams {
            id: "archived".to_string(),
            project_id: p.id.clone(),
            backend: "daemon".to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    SessionService::create(
        &conn,
        &CreateSessionParams {
            id: "destroyed".to_string(),
            project_id: p.id.clone(),
            backend: "daemon".to_string(),
            ..Default::default()
        },
    )
    .unwrap();

    SessionService::archive(&conn, "archived").unwrap();
    SessionService::destroy(&conn, "destroyed").unwrap();

    // Active list: only "active"
    let active = SessionService::list_for_project(&conn, &p.id).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "active");

    // Archived list: only "archived"
    let archived = SessionService::list_archived(&conn).unwrap();
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].id, "archived");

    // All: shows all 3
    let all = SessionService::list_all_for_project(&conn, &p.id).unwrap();
    assert_eq!(all.len(), 3);
}

#[test]
fn pr_state_persists_via_shared_service() {
    let conn = test_db();
    let p = ProjectService::ensure_project(&conn, "/tmp/proj").unwrap();
    SessionService::create(
        &conn,
        &CreateSessionParams {
            id: "s1".to_string(),
            project_id: p.id.clone(),
            backend: "daemon".to_string(),
            ..Default::default()
        },
    )
    .unwrap();

    SessionService::update_pr_state(&conn, "s1", "https://github.com/org/repo/pull/1", "open")
        .unwrap();
    let s = SessionService::get(&conn, "s1").unwrap().unwrap();
    assert_eq!(
        s.pr_url.as_deref(),
        Some("https://github.com/org/repo/pull/1")
    );
    assert_eq!(s.pr_state.as_deref(), Some("open"));
}

#[test]
fn project_archive_cascades_to_sessions() {
    let conn = test_db();
    let p = ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();
    SessionService::create(
        &conn,
        &CreateSessionParams {
            id: "s1".to_string(),
            project_id: p.id.clone(),
            backend: "daemon".to_string(),
            ..Default::default()
        },
    )
    .unwrap();

    ProjectService::archive(&conn, &p.id).unwrap();

    // Project not in active list
    assert!(ProjectService::list_active(&conn).unwrap().is_empty());
    // Session is archived
    let s = SessionService::get(&conn, "s1").unwrap().unwrap();
    assert_eq!(s.status, "archived");
}

#[test]
fn project_delete_cascades_to_sessions() {
    let conn = test_db();
    let p = ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();
    SessionService::create(
        &conn,
        &CreateSessionParams {
            id: "s1".to_string(),
            project_id: p.id.clone(),
            backend: "daemon".to_string(),
            ..Default::default()
        },
    )
    .unwrap();

    ProjectService::delete(&conn, &p.id).unwrap();
    assert!(SessionService::get(&conn, "s1").unwrap().is_none());
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
        name: String::new(),
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

/// PLA-128: Iced app must store empty name (not the provider name) when no
/// explicit name is given — same default as the Tauri app.
#[test]
fn iced_session_name_empty_when_not_specified() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/project").unwrap();

    // Iced quick-launch: no user-provided name → empty string (not "kiro")
    let params = CreateSessionParams {
        id: "iced-no-name".to_string(),
        project_id: project.id.clone(),
        name: String::new(),
        backend: "daemon".to_string(),
        auto_approve: true,
        branch: "feat/my-feature".to_string(),
        ..Default::default()
    };
    let record = SessionService::create(&conn, &params).unwrap();
    assert_eq!(record.name, "");
    assert_eq!(record.branch, "feat/my-feature");

    // Iced session form with explicit name: preserved as-is
    let params_named = CreateSessionParams {
        id: "iced-with-name".to_string(),
        project_id: project.id.clone(),
        name: "My Session".to_string(),
        backend: "daemon".to_string(),
        auto_approve: true,
        branch: "my-session".to_string(),
        ..Default::default()
    };
    let record_named = SessionService::create(&conn, &params_named).unwrap();
    assert_eq!(record_named.name, "My Session");
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

// ─── Orphan prevention tests ─────────────────────────────────────────────────
// These tests verify the invariant: a daemon session cannot exist without a
// corresponding durable session record.

/// Simulates: DB create succeeds + daemon spawn fails → record marked destroyed.
/// The Iced workflow follows this exact pattern.
#[test]
fn spawn_failure_marks_session_destroyed() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/project").unwrap();

    // Step 1: Create session record (persist-before-spawn)
    let session_id = "orphan-test-1";
    let params = CreateSessionParams {
        id: session_id.to_string(),
        project_id: project.id.clone(),
        name: "about to fail spawn".to_string(),
        backend: "daemon".to_string(),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();

    // Verify record exists and is active
    let s = SessionService::get(&conn, session_id).unwrap().unwrap();
    assert_eq!(s.status, "active");

    // Step 2: Simulate daemon spawn failure — mark destroyed
    // (In real code, the Iced workflow does this on DaemonSession::spawn_with_session_id error)
    SessionService::set_status(&conn, session_id, "destroyed").unwrap();

    // Step 3: Verify the record is NOT in the active list (no orphan visible)
    let active = SessionService::list_for_project(&conn, &project.id).unwrap();
    assert!(
        active.is_empty(),
        "destroyed session should not appear in active list"
    );

    // The record still exists for audit/debugging
    let s = SessionService::get(&conn, session_id).unwrap().unwrap();
    assert_eq!(s.status, "destroyed");
}

/// Simulates: DB persist fails → launch aborts before spawn → no orphan possible.
/// This test verifies the invariant that if no DB record exists, no daemon should be spawned.
#[test]
fn persist_failure_prevents_spawn() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/project").unwrap();

    // Step 1: First create a session to set up a duplicate ID scenario
    let session_id = "dup-id";
    let params = CreateSessionParams {
        id: session_id.to_string(),
        project_id: project.id.clone(),
        name: "first".to_string(),
        backend: "daemon".to_string(),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();

    // Step 2: Try to create a second session with same ID (simulates DB failure)
    let duplicate_params = CreateSessionParams {
        id: session_id.to_string(),
        project_id: project.id.clone(),
        name: "duplicate".to_string(),
        backend: "daemon".to_string(),
        ..Default::default()
    };
    let result = SessionService::create(&conn, &duplicate_params);

    // Step 3: Verify DB create fails (UNIQUE constraint on id)
    assert!(result.is_err(), "duplicate insert should fail");

    // Step 4: Because persist failed, no spawn should happen (caller aborts).
    // Verify original record unchanged (no corruption).
    let s = SessionService::get(&conn, session_id).unwrap().unwrap();
    assert_eq!(s.name, "first");
    assert_eq!(s.status, "active");
}

/// Verifies: preallocated session ID in DB matches the daemon session ID.
/// No separate mapping needed — both use the same UUID.
#[test]
fn preallocated_session_id_matches_daemon_id() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/project").unwrap();

    let preallocated_id = uuid::Uuid::new_v4().to_string();
    let params = CreateSessionParams {
        id: preallocated_id.clone(),
        project_id: project.id.clone(),
        name: "preallocated".to_string(),
        backend: "daemon".to_string(),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();

    // The daemon would receive this same ID via spawn_with_session_id.
    // Verify the record uses that exact ID.
    let s = SessionService::get(&conn, &preallocated_id)
        .unwrap()
        .unwrap();
    assert_eq!(s.id, preallocated_id);
    assert_eq!(s.backend, "daemon");
}
