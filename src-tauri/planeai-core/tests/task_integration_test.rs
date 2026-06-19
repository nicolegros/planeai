//! Tests for task integration — TaskService methods, task launch resolution,
//! prompt injection, lifecycle hooks, and task/session linkage.

use planeai_core::services::{
    CreateSessionParams, ProjectService, SessionService, TaskLaunchRequest, TaskService,
    WorktreeService,
};

fn test_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    planeai_core::services::migrate(&conn).unwrap();
    conn
}

// ─── TaskService::resolve_task_prompt ────────────────────────────────────────

#[test]
fn resolve_task_prompt_uses_default_template() {
    let task = planeai_tasks::model::Task {
        key: "PLA-1".to_string(),
        title: "Fix the bug".to_string(),
        description: "It crashes on start".to_string(),
        status: planeai_tasks::model::Status::Todo,
        priority: 0,
        parent_key: None,
        blocked_by: Vec::new(),
        tags: Vec::new(),
        base_branch: "main".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let prompt = TaskService::resolve_task_prompt(&task, None);
    assert_eq!(prompt, "Fix the bug\n\nIt crashes on start");
}

#[test]
fn resolve_task_prompt_uses_custom_template() {
    let task = planeai_tasks::model::Task {
        key: "PLA-2".to_string(),
        title: "Add feature".to_string(),
        description: "New button".to_string(),
        status: planeai_tasks::model::Status::Todo,
        priority: 0,
        parent_key: None,
        blocked_by: Vec::new(),
        tags: Vec::new(),
        base_branch: "main".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let prompt = TaskService::resolve_task_prompt(&task, Some("[{key}] {title}"));
    assert_eq!(prompt, "[PLA-2] Add feature");
}

// ─── TaskService::resolve_task_launch ────────────────────────────────────────

#[test]
fn resolve_task_launch_produces_worktree_mode() {
    let config = planeai_core::session_launch::LaunchConfig {
        providers: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "test".to_string(),
                planeai_core::session_launch::ProviderConfig {
                    command: "agent".to_string(),
                    yolo_flag: Some("--yolo".to_string()),
                    prompt_command: Some("{prompt}".to_string()),
                    autonomous_prompt_template: None,
                },
            );
            m
        },
        default_provider: "test".to_string(),
        ..Default::default()
    };

    let request = TaskLaunchRequest {
        project_id: "proj-1".to_string(),
        project_name: "myproject".to_string(),
        project_path: std::env::temp_dir(),
        task_key: "PLA-5".to_string(),
        task_title: "Do thing".to_string(),
        task_description: "Details here".to_string(),
        task_base_branch: "main".to_string(),
        provider_id: Some("test".to_string()),
        auto_approve: true,
        autonomous: false,
        cols: 80,
        rows: 24,
    };

    let (resolved, wt_mode) = TaskService::resolve_task_launch(&request, &config, None).unwrap();

    // Command includes prompt + yolo
    assert!(resolved.command_label.contains("--yolo"));
    assert!(resolved.command_label.contains("Do thing"));
    assert!(resolved.prompt_was_injected);
    assert!(resolved.auto_approve_was_applied);

    // Worktree mode is Create with task-key-based branch
    match wt_mode {
        planeai_core::services::WorktreeMode::Create {
            branch_name,
            task_key,
            ..
        } => {
            assert!(branch_name.starts_with("pla-5/"));
            assert_eq!(task_key, Some("PLA-5".to_string()));
        }
        other => panic!("expected WorktreeMode::Create, got {:?}", other),
    }
}

#[test]
fn resolve_task_launch_autonomous_false_no_wrapper() {
    let config = planeai_core::session_launch::LaunchConfig {
        providers: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "test".to_string(),
                planeai_core::session_launch::ProviderConfig {
                    command: "agent".to_string(),
                    yolo_flag: None,
                    prompt_command: Some("-p {prompt}".to_string()),
                    autonomous_prompt_template: Some("AUTO: {prompt}".to_string()),
                },
            );
            m
        },
        default_provider: "test".to_string(),
        ..Default::default()
    };

    let request = TaskLaunchRequest {
        project_id: "p1".to_string(),
        project_name: "proj".to_string(),
        project_path: std::env::temp_dir(),
        task_key: "X-1".to_string(),
        task_title: "Task".to_string(),
        task_description: "Desc".to_string(),
        task_base_branch: "main".to_string(),
        provider_id: None,
        auto_approve: false,
        autonomous: false,
        cols: 80,
        rows: 24,
    };

    let (resolved, _) = TaskService::resolve_task_launch(&request, &config, None).unwrap();
    // autonomous=false means no wrapper applied
    assert!(!resolved.command_label.contains("AUTO:"));
    assert!(resolved.command_label.contains("Task"));
}

#[test]
fn resolve_task_launch_autonomous_true_applies_wrapper() {
    let config = planeai_core::session_launch::LaunchConfig {
        providers: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "test".to_string(),
                planeai_core::session_launch::ProviderConfig {
                    command: "agent".to_string(),
                    yolo_flag: None,
                    prompt_command: Some("-p {prompt}".to_string()),
                    autonomous_prompt_template: Some("AUTO: {prompt}".to_string()),
                },
            );
            m
        },
        default_provider: "test".to_string(),
        ..Default::default()
    };

    let request = TaskLaunchRequest {
        project_id: "p1".to_string(),
        project_name: "proj".to_string(),
        project_path: std::env::temp_dir(),
        task_key: "X-1".to_string(),
        task_title: "Task".to_string(),
        task_description: "Desc".to_string(),
        task_base_branch: "main".to_string(),
        provider_id: None,
        auto_approve: false,
        autonomous: true,
        cols: 80,
        rows: 24,
    };

    let (resolved, _) = TaskService::resolve_task_launch(&request, &config, None).unwrap();
    // autonomous=true applies the wrapper
    assert!(resolved.command_label.contains("AUTO:"));
}

// ─── Session record task linkage ─────────────────────────────────────────────

#[test]
fn task_launch_session_stores_task_key() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/test-proj").unwrap();

    let params = CreateSessionParams {
        id: "task-session-1".to_string(),
        project_id: project.id.clone(),
        name: "PLA-5: Fix bug".to_string(),
        backend: "daemon".to_string(),
        task_key: Some("PLA-5".to_string()),
        branch: "pla-5/abcd1234".to_string(),
        worktree_path: Some("/home/.planeai/worktrees/proj/abcd1234".to_string()),
        base_branch: Some("main".to_string()),
        auto_approve: true,
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();

    let rec = SessionService::get(&conn, "task-session-1")
        .unwrap()
        .unwrap();
    assert_eq!(rec.task_key, Some("PLA-5".to_string()));
    assert_eq!(rec.branch, "pla-5/abcd1234");
    assert_eq!(
        rec.worktree_path,
        Some("/home/.planeai/worktrees/proj/abcd1234".to_string())
    );
    assert_eq!(rec.base_branch, Some("main".to_string()));
    assert!(rec.auto_approve);
}

#[test]
fn session_status_update_preserves_task_linkage() {
    let conn = test_db();
    let project = ProjectService::ensure_project(&conn, "/tmp/test-proj2").unwrap();

    let params = CreateSessionParams {
        id: "task-session-2".to_string(),
        project_id: project.id.clone(),
        name: "X-10: Feature".to_string(),
        backend: "daemon".to_string(),
        task_key: Some("X-10".to_string()),
        ..Default::default()
    };
    SessionService::create(&conn, &params).unwrap();

    // Transition through lifecycle statuses
    SessionService::mark_exited(&conn, "task-session-2").unwrap();
    let rec = SessionService::get(&conn, "task-session-2")
        .unwrap()
        .unwrap();
    assert_eq!(rec.status, "exited");
    assert_eq!(rec.task_key, Some("X-10".to_string()));

    SessionService::set_status(&conn, "task-session-2", "destroyed").unwrap();
    let rec = SessionService::get(&conn, "task-session-2")
        .unwrap()
        .unwrap();
    assert_eq!(rec.status, "destroyed");
    assert_eq!(rec.task_key, Some("X-10".to_string()));
}

// ─── Task lifecycle hooks ────────────────────────────────────────────────────

#[test]
fn fire_lifecycle_hook_moves_task_status() {
    let db_dir = tempfile::tempdir().unwrap();
    let db_path = db_dir.path().join("test.db");

    // Create task DB
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    planeai_core::services::migrate(&conn).unwrap();
    planeai_tasks::sqlite::migrate(&conn).unwrap();
    drop(conn);

    // Create a task
    let repo =
        planeai_tasks::sqlite::SqliteRepository::open(db_path.to_str().unwrap(), "TST").unwrap();
    use planeai_tasks::provider::TaskProvider;
    let task = repo
        .create(planeai_tasks::model::CreateParams {
            title: "Test task".to_string(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(task.status, planeai_tasks::model::Status::Todo);

    // Fire lifecycle hook to move to in_progress
    TaskService::fire_lifecycle_hook(&db_path, "test", &task.key, "in_progress").unwrap();

    // Verify
    let updated = repo.get(&task.key).unwrap();
    assert_eq!(updated.status, planeai_tasks::model::Status::InProgress);

    // Fire on_complete hook
    TaskService::fire_lifecycle_hook(&db_path, "test", &task.key, "done").unwrap();
    let done = repo.get(&task.key).unwrap();
    assert_eq!(done.status, planeai_tasks::model::Status::Done);
}

// ─── WorktreeService branch naming from task ─────────────────────────────────

#[test]
fn branch_name_from_task_key_normalizes() {
    let name = WorktreeService::branch_name("PLA-5", "abcd1234");
    assert_eq!(name, "pla-5/abcd1234");
}

#[test]
fn branch_name_from_task_key_with_spaces() {
    let name = WorktreeService::branch_name("MY TASK-1", "12345678");
    assert_eq!(name, "my-task-1/12345678");
}
