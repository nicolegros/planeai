//! Integration tests for the loop service — durable loop data model.

use planeai_core::loop_run::*;
use planeai_core::loop_service::*;
use planeai_core::services::open_db_at;

fn test_db() -> rusqlite::Connection {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = open_db_at(&path).unwrap();
    std::mem::forget(dir);
    conn
}

// ─── 1. Migration idempotence ────────────────────────────────────────────────

#[test]
fn loop_migration_is_idempotent() {
    let conn = test_db();
    // Migration already ran once via open_db_at → migrate(). Run it twice more.
    LoopService::migrate(&conn).unwrap();
    LoopService::migrate(&conn).unwrap();

    // All 5 tables should exist
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('loop_runs', 'loop_sessions', 'loop_events', 'loop_artifacts', 'verifier_runs')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 5);
}

// ─── 2. Create loop → get loop ───────────────────────────────────────────────

#[test]
fn create_loop_and_get_returns_it() {
    let conn = test_db();
    let created = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: Some("PLA-42".into()),
            parent_session_id: "sess-parent".into(),
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "Fix the bug".into(),
            max_rounds: 5,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    assert_eq!(created.status, LoopStatus::Draft);
    assert_eq!(created.current_round, 0);
    assert_eq!(created.max_rounds, 5);
    assert_eq!(created.goal, "Fix the bug");

    let fetched = LoopService::get_loop(&conn, &created.id).unwrap().unwrap();
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.project_id, "proj-1");
    assert_eq!(fetched.task_key, Some("PLA-42".to_string()));
    assert_eq!(fetched.parent_session_id, "sess-parent");
    assert_eq!(fetched.strategy, LoopStrategy::new("maker-verifier"));
    assert_eq!(fetched.goal, "Fix the bug");
    assert_eq!(fetched.status, LoopStatus::Draft);
}

// ─── 3. List loops filters by project_id ─────────────────────────────────────

#[test]
fn list_loops_filters_by_project() {
    let conn = test_db();

    let make = |project_id: &str| {
        CreateLoopParams {
            project_id: project_id.into(),
            task_key: None,
            parent_session_id: "sess-1".into(),
            strategy: LoopStrategy::new("single"),
            goal: "goal".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        }
    };

    LoopService::create_loop(&conn, make("proj-a")).unwrap();
    LoopService::create_loop(&conn, make("proj-a")).unwrap();
    LoopService::create_loop(&conn, make("proj-b")).unwrap();

    let a_loops = LoopService::list_loops(&conn, "proj-a").unwrap();
    let b_loops = LoopService::list_loops(&conn, "proj-b").unwrap();
    let c_loops = LoopService::list_loops(&conn, "proj-c").unwrap();

    assert_eq!(a_loops.len(), 2);
    assert_eq!(b_loops.len(), 1);
    assert_eq!(c_loops.len(), 0);
}

// ─── 4. Update status updates status and updated_at ──────────────────────────

#[test]
fn update_loop_status_changes_status_and_updated_at() {
    let conn = test_db();
    let created = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            parent_session_id: "sess-1".into(),
            strategy: LoopStrategy::new("single"),
            goal: "do stuff".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    // Small sleep to ensure timestamp difference
    std::thread::sleep(std::time::Duration::from_millis(10));

    LoopService::update_loop_status(&conn, &created.id, LoopStatus::Running).unwrap();

    let fetched = LoopService::get_loop(&conn, &created.id).unwrap().unwrap();
    assert_eq!(fetched.status, LoopStatus::Running);
    assert!(fetched.updated_at > created.updated_at, "updated_at should advance");
    assert_eq!(fetched.finished_at, None, "running should not set finished_at");
}

#[test]
fn update_loop_status_to_terminal_sets_finished_at() {
    let conn = test_db();
    let created = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            parent_session_id: "sess-1".into(),
            strategy: LoopStrategy::new("single"),
            goal: "do stuff".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    LoopService::update_loop_status(&conn, &created.id, LoopStatus::Failed).unwrap();

    let fetched = LoopService::get_loop(&conn, &created.id).unwrap().unwrap();
    assert_eq!(fetched.status, LoopStatus::Failed);
    assert!(fetched.finished_at.is_some(), "terminal status should set finished_at");
}

// ─── 5. Add loop session → list returns them ─────────────────────────────────

#[test]
fn add_and_list_loop_sessions() {
    let conn = test_db();
    let loop_run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            parent_session_id: "sess-1".into(),
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "implement feature".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    let s1 = LoopService::add_loop_session(
        &conn,
        AddLoopSessionParams {
            loop_id: loop_run.id.clone(),
            session_id: "sess-maker".into(),
            role: "maker".into(),
            round: 1,
            provider: Some("claude".into()),
            status: "active".into(),
        },
    )
    .unwrap();

    let s2 = LoopService::add_loop_session(
        &conn,
        AddLoopSessionParams {
            loop_id: loop_run.id.clone(),
            session_id: "sess-verifier".into(),
            role: "verifier".into(),
            round: 1,
            provider: Some("kiro".into()),
            status: "active".into(),
        },
    )
    .unwrap();

    assert_eq!(s1.role, "maker");
    assert_eq!(s2.role, "verifier");

    let sessions = LoopService::list_loop_sessions(&conn, &loop_run.id).unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].session_id, "sess-maker");
    assert_eq!(sessions[1].session_id, "sess-verifier");
}

// ─── 6. Append events → list returns ordered by id ───────────────────────────

#[test]
fn append_and_list_events_ordered_by_id() {
    let conn = test_db();
    let loop_run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            parent_session_id: "sess-1".into(),
            strategy: LoopStrategy::new("single"),
            goal: "goal".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    let e1 = LoopService::append_loop_event(
        &conn,
        &loop_run.id,
        "round_started",
        &serde_json::json!({"round": 1}),
    )
    .unwrap();

    let e2 = LoopService::append_loop_event(
        &conn,
        &loop_run.id,
        "session_spawned",
        &serde_json::json!({"session_id": "s1"}),
    )
    .unwrap();

    assert!(e2.id > e1.id, "event ids should be monotonically increasing");

    let events = LoopService::list_loop_events(&conn, &loop_run.id).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].kind, "round_started");
    assert_eq!(events[1].kind, "session_spawned");
    assert_eq!(events[0].payload_json, serde_json::json!({"round": 1}));
}

// ─── 7. Add verifier run → update changes status/exit_code ───────────────────

#[test]
fn add_and_update_verifier_run() {
    let conn = test_db();
    let loop_run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            parent_session_id: "sess-1".into(),
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "goal".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    let vr = LoopService::add_verifier_run(
        &conn,
        AddVerifierRunParams {
            loop_id: loop_run.id.clone(),
            session_id: None,
            verifier_type: "command".into(),
            name: "cargo test".into(),
            command: "cargo test --workspace".into(),
        },
    )
    .unwrap();

    assert_eq!(vr.status, "pending");
    assert_eq!(vr.exit_code, None);
    assert_eq!(vr.finished_at, None);

    LoopService::update_verifier_run(
        &conn,
        &vr.id,
        "passed",
        Some(0),
        Some("/tmp/output.log"),
    )
    .unwrap();

    // Verify by reading directly (get_verifier_run not yet needed in interface)
    let row: (String, Option<i32>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT status, exit_code, output_path, finished_at FROM verifier_runs WHERE id = ?1",
            rusqlite::params![vr.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();

    assert_eq!(row.0, "passed");
    assert_eq!(row.1, Some(0));
    assert_eq!(row.2, Some("/tmp/output.log".to_string()));
    assert!(row.3.is_some(), "finished_at should be set on update");
}

// ─── 8. Add artifact ─────────────────────────────────────────────────────────

#[test]
fn add_artifact_persists() {
    let conn = test_db();
    let loop_run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            parent_session_id: "sess-1".into(),
            strategy: LoopStrategy::new("single"),
            goal: "goal".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    let artifact = LoopService::add_artifact(
        &conn,
        AddArtifactParams {
            loop_id: loop_run.id.clone(),
            session_id: Some("sess-maker".into()),
            kind: "diff".into(),
            path: Some("src/main.rs".into()),
            content_json: Some(serde_json::json!({"lines_added": 42})),
        },
    )
    .unwrap();

    assert_eq!(artifact.loop_id, loop_run.id);
    assert_eq!(artifact.kind, "diff");
    assert_eq!(artifact.path, Some("src/main.rs".to_string()));

    // Verify persistence
    let row_kind: String = conn
        .query_row(
            "SELECT kind FROM loop_artifacts WHERE id = ?1",
            rusqlite::params![artifact.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(row_kind, "diff");
}
