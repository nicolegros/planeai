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

    // Indexes should exist
    let idx_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND (name LIKE 'idx_loop%' OR name LIKE 'idx_verifier%')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        idx_count >= 8,
        "expected at least 8 indexes, got {idx_count}"
    );
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
            created_by_session_id: Some("sess-parent".into()),
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "Fix the bug".into(),
            max_rounds: 5,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    assert_eq!(created.status, LoopStatus::Draft);
    assert_eq!(created.max_rounds, 5);
    assert_eq!(created.goal, "Fix the bug");

    let fetched = LoopService::get_loop(&conn, &created.id).unwrap().unwrap();
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.project_id, "proj-1");
    assert_eq!(fetched.task_key, Some("PLA-42".to_string()));
    assert_eq!(
        fetched.created_by_session_id,
        Some("sess-parent".to_string())
    );
    assert_eq!(fetched.strategy, LoopStrategy::new("maker-verifier"));
    assert_eq!(fetched.goal, "Fix the bug");
    assert_eq!(fetched.status, LoopStatus::Draft);
}

#[test]
fn create_loop_without_session_id() {
    let conn = test_db();
    let created = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("single"),
            goal: "CLI-initiated loop".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    assert_eq!(created.created_by_session_id, None);
    let fetched = LoopService::get_loop(&conn, &created.id).unwrap().unwrap();
    assert_eq!(fetched.created_by_session_id, None);
}

// ─── 3. List loops filters by project_id ─────────────────────────────────────

#[test]
fn list_loops_filters_by_project() {
    let conn = test_db();

    let make = |project_id: &str| CreateLoopParams {
        project_id: project_id.into(),
        task_key: None,
        created_by_session_id: None,
        strategy: LoopStrategy::new("single"),
        goal: "goal".into(),
        max_rounds: 3,
        policy_json: None,
        budget_json: None,
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
fn transition_loop_changes_status_and_updated_at() {
    let conn = test_db();
    let created = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("single"),
            goal: "do stuff".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(10));

    LoopService::transition_loop(&conn, &created.id, LoopTrigger::Start).unwrap();

    let fetched = LoopService::get_loop(&conn, &created.id).unwrap().unwrap();
    assert_eq!(fetched.status, LoopStatus::Running);
    assert!(
        fetched.updated_at > created.updated_at,
        "updated_at should advance"
    );
    assert_eq!(
        fetched.executor_finished_at, None,
        "running should not set executor_finished_at"
    );
}

#[test]
fn transition_to_completed_unreviewed_sets_executor_finished_at() {
    let conn = test_db();
    let created = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("single"),
            goal: "do stuff".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    // Draft → Running → CompletedUnreviewed via RecipeSetStatus
    LoopService::transition_loop(&conn, &created.id, LoopTrigger::Start).unwrap();
    LoopService::transition_loop(
        &conn,
        &created.id,
        LoopTrigger::RecipeSetStatus(LoopStatus::CompletedUnreviewed),
    )
    .unwrap();

    let fetched = LoopService::get_loop(&conn, &created.id).unwrap().unwrap();
    assert_eq!(fetched.status, LoopStatus::CompletedUnreviewed);
    assert!(
        fetched.executor_finished_at.is_some(),
        "completed_unreviewed should set executor_finished_at"
    );
}

#[test]
fn transition_to_approved_does_not_set_executor_finished_at() {
    let conn = test_db();
    let created = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("single"),
            goal: "do stuff".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    // Draft → Running → CompletedUnreviewed → Approved
    LoopService::transition_loop(&conn, &created.id, LoopTrigger::Start).unwrap();
    LoopService::transition_loop(
        &conn,
        &created.id,
        LoopTrigger::RecipeSetStatus(LoopStatus::CompletedUnreviewed),
    )
    .unwrap();
    LoopService::transition_loop(&conn, &created.id, LoopTrigger::Approve).unwrap();

    let fetched = LoopService::get_loop(&conn, &created.id).unwrap().unwrap();
    assert_eq!(fetched.status, LoopStatus::Approved);
    // executor_finished_at was set when moving to CompletedUnreviewed, not Approved
    assert!(
        fetched.executor_finished_at.is_some(),
        "executor_finished_at should have been set at CompletedUnreviewed"
    );
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
            created_by_session_id: Some("sess-1".into()),
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
            created_by_session_id: None,
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

    assert!(
        e2.id > e1.id,
        "event ids should be monotonically increasing"
    );

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
            created_by_session_id: None,
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

    LoopService::update_verifier_run(&conn, &vr.id, "passed", Some(0), Some("/tmp/output.log"))
        .unwrap();

    // Verify by reading directly
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
            created_by_session_id: None,
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

// ─── 9. Strict parsing rejects invalid status ────────────────────────────────

#[test]
fn get_loop_with_invalid_status_returns_error() {
    let conn = test_db();
    let created = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("single"),
            goal: "goal".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    // Corrupt the status directly in the DB
    conn.execute(
        "UPDATE loop_runs SET status = 'bogus_invalid' WHERE id = ?1",
        rusqlite::params![created.id],
    )
    .unwrap();

    let result = LoopService::get_loop(&conn, &created.id);
    match result {
        Err(LoopServiceError::InvalidStatus(e)) => {
            assert_eq!(e.0, "bogus_invalid");
        }
        other => panic!("expected InvalidStatus error, got {:?}", other),
    }
}

#[test]
fn list_loops_with_invalid_status_returns_error() {
    let conn = test_db();
    let created = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("single"),
            goal: "goal".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    conn.execute(
        "UPDATE loop_runs SET status = 'garbage' WHERE id = ?1",
        rusqlite::params![created.id],
    )
    .unwrap();

    let result = LoopService::list_loops(&conn, "proj-1");
    assert!(result.is_err());
}

// ─── 10. Lossy parsing still works for UI ────────────────────────────────────

#[test]
fn parse_loop_status_lossy_falls_back_to_draft() {
    let status = parse_loop_status_lossy("totally_unknown");
    assert_eq!(status, LoopStatus::Draft);
}

#[test]
fn parse_loop_status_lossy_parses_valid() {
    let status = parse_loop_status_lossy("running");
    assert_eq!(status, LoopStatus::Running);
}

// ─── 11. Regression: #269 schema migration ───────────────────────────────────

/// Helper: create a database with the exact #269 schema (parent_session_id NOT NULL, finished_at)
fn setup_269_schema() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .unwrap();
    conn.execute_batch(
        "CREATE TABLE loop_runs (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            task_key TEXT,
            parent_session_id TEXT NOT NULL,
            strategy TEXT NOT NULL,
            goal TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'draft',
            current_round INTEGER NOT NULL DEFAULT 0,
            max_rounds INTEGER NOT NULL DEFAULT 3,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            finished_at TEXT,
            policy_json TEXT,
            budget_json TEXT
        );
        CREATE TABLE loop_sessions (
            loop_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            role TEXT NOT NULL,
            round INTEGER NOT NULL,
            provider TEXT,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (loop_id, session_id)
        );
        CREATE TABLE loop_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            loop_id TEXT NOT NULL,
            ts TEXT NOT NULL,
            kind TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );
        CREATE TABLE loop_artifacts (
            id TEXT PRIMARY KEY,
            loop_id TEXT NOT NULL,
            session_id TEXT,
            kind TEXT NOT NULL,
            path TEXT,
            content_json TEXT,
            created_at TEXT NOT NULL
        );
        CREATE TABLE verifier_runs (
            id TEXT PRIMARY KEY,
            loop_id TEXT NOT NULL,
            session_id TEXT,
            verifier_type TEXT NOT NULL,
            name TEXT NOT NULL,
            command TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            exit_code INTEGER,
            output_path TEXT,
            created_at TEXT NOT NULL,
            finished_at TEXT
        );",
    )
    .unwrap();
    conn
}

#[test]
fn migrate_from_269_schema_renames_columns_before_indexes() {
    let conn = setup_269_schema();

    // Insert a row with old schema to verify data survives migration
    conn.execute(
        "INSERT INTO loop_runs (id, project_id, parent_session_id, strategy, goal, status, created_at, updated_at)
         VALUES ('loop-1', 'proj-1', 'sess-old', 'single', 'goal', 'running', '2025-01-01T00:00:00Z', '2025-01-01T00:00:00Z')",
        [],
    ).unwrap();

    LoopService::migrate(&conn).unwrap();

    // Verify columns were renamed
    let cols: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(loop_runs)").unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };

    assert!(cols.contains(&"created_by_session_id".to_string()));
    assert!(cols.contains(&"executor_finished_at".to_string()));
    assert!(!cols.contains(&"parent_session_id".to_string()));
    assert!(!cols.contains(&"finished_at".to_string()));

    // Verify data survived
    let session_id: Option<String> = conn
        .query_row(
            "SELECT created_by_session_id FROM loop_runs WHERE id = 'loop-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(session_id, Some("sess-old".to_string()));

    // Verify indexes were created (they reference the new column names)
    let idx_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND (name LIKE 'idx_loop%' OR name LIKE 'idx_verifier%')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        idx_count >= 8,
        "expected at least 8 indexes, got {idx_count}"
    );
}

#[test]
fn migrate_from_269_schema_makes_created_by_session_id_nullable() {
    let conn = setup_269_schema();
    LoopService::migrate(&conn).unwrap();

    let mut stmt = conn.prepare("PRAGMA table_info(loop_runs)").unwrap();
    let cols: Vec<(String, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?, // name
                row.get::<_, i64>(3)?,    // notnull
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let (_, notnull) = cols
        .iter()
        .find(|(name, _)| name == "created_by_session_id")
        .expect("created_by_session_id column must exist");

    assert_eq!(
        *notnull, 0,
        "created_by_session_id must be nullable after migration"
    );
}

// ─── 12. Child writes touch loop updated_at ──────────────────────────────────

#[test]
fn child_writes_advance_loop_updated_at() {
    let conn = test_db();
    let loop_run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("single"),
            goal: "goal".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    let original_updated_at = loop_run.updated_at.clone();
    std::thread::sleep(std::time::Duration::from_millis(10));

    // add_loop_session should touch
    LoopService::add_loop_session(
        &conn,
        AddLoopSessionParams {
            loop_id: loop_run.id.clone(),
            session_id: "s1".into(),
            role: "maker".into(),
            round: 1,
            provider: None,
            status: "active".into(),
        },
    )
    .unwrap();

    let after_session = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert!(
        after_session.updated_at > original_updated_at,
        "add_loop_session should advance updated_at"
    );

    std::thread::sleep(std::time::Duration::from_millis(10));

    // append_loop_event should touch
    LoopService::append_loop_event(&conn, &loop_run.id, "test_event", &serde_json::json!({}))
        .unwrap();

    let after_event = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert!(
        after_event.updated_at > after_session.updated_at,
        "append_loop_event should advance updated_at"
    );

    std::thread::sleep(std::time::Duration::from_millis(10));

    // add_artifact should touch
    LoopService::add_artifact(
        &conn,
        AddArtifactParams {
            loop_id: loop_run.id.clone(),
            session_id: None,
            kind: "patch".into(),
            path: None,
            content_json: None,
        },
    )
    .unwrap();

    let after_artifact = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert!(
        after_artifact.updated_at > after_event.updated_at,
        "add_artifact should advance updated_at"
    );

    std::thread::sleep(std::time::Duration::from_millis(10));

    // add_verifier_run + update_verifier_run should touch
    let vr = LoopService::add_verifier_run(
        &conn,
        AddVerifierRunParams {
            loop_id: loop_run.id.clone(),
            session_id: None,
            verifier_type: "command".into(),
            name: "test".into(),
            command: "cargo test".into(),
        },
    )
    .unwrap();

    let after_vr_add = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert!(
        after_vr_add.updated_at > after_artifact.updated_at,
        "add_verifier_run should advance updated_at"
    );

    std::thread::sleep(std::time::Duration::from_millis(10));

    LoopService::update_verifier_run(&conn, &vr.id, "passed", Some(0), None).unwrap();

    let after_vr_update = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert!(
        after_vr_update.updated_at > after_vr_add.updated_at,
        "update_verifier_run should advance updated_at"
    );
}

// ─── 13. Child writes fail for missing loop (orphan prevention) ──────────────

#[test]
fn add_loop_session_fails_for_missing_loop() {
    let conn = test_db();
    let result = LoopService::add_loop_session(
        &conn,
        AddLoopSessionParams {
            loop_id: "does-not-exist".into(),
            session_id: "s1".into(),
            role: "maker".into(),
            round: 1,
            provider: None,
            status: "active".into(),
        },
    );
    assert!(result.is_err(), "should fail for missing loop");

    // Verify no orphan row was left behind
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM loop_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "no orphan rows should exist");
}

#[test]
fn append_loop_event_fails_for_missing_loop() {
    let conn = test_db();
    let result = LoopService::append_loop_event(
        &conn,
        "does-not-exist",
        "round_started",
        &serde_json::json!({}),
    );
    assert!(result.is_err(), "should fail for missing loop");

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM loop_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "no orphan rows should exist");
}

#[test]
fn add_artifact_fails_for_missing_loop() {
    let conn = test_db();
    let result = LoopService::add_artifact(
        &conn,
        AddArtifactParams {
            loop_id: "does-not-exist".into(),
            session_id: None,
            kind: "diff".into(),
            path: None,
            content_json: None,
        },
    );
    assert!(result.is_err(), "should fail for missing loop");

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM loop_artifacts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "no orphan rows should exist");
}

#[test]
fn add_verifier_run_fails_for_missing_loop() {
    let conn = test_db();
    let result = LoopService::add_verifier_run(
        &conn,
        AddVerifierRunParams {
            loop_id: "does-not-exist".into(),
            session_id: None,
            verifier_type: "command".into(),
            name: "test".into(),
            command: "cargo test".into(),
        },
    );
    assert!(result.is_err(), "should fail for missing loop");

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM verifier_runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "no orphan rows should exist");
}

// ─── 14. Migration recovers from leftover temp table ─────────────────────────

#[test]
fn migrate_from_269_schema_recovers_from_leftover_loop_runs_new() {
    let conn = setup_269_schema();

    // Simulate a previously half-failed migration that left loop_runs_new behind
    conn.execute_batch("CREATE TABLE loop_runs_new (id TEXT PRIMARY KEY);")
        .unwrap();

    LoopService::migrate(&conn).unwrap();

    // Verify final schema is correct
    let mut stmt = conn.prepare("PRAGMA table_info(loop_runs)").unwrap();
    let cols: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(cols.contains(&"created_by_session_id".to_string()));
    assert!(cols.contains(&"executor_finished_at".to_string()));

    // Verify temp table was cleaned up
    let temp_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='loop_runs_new'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        temp_exists, 0,
        "loop_runs_new should not exist after migration"
    );
}

// ─── complete_verifier_run tests ─────────────────────────────────────────────

#[test]
fn complete_verifier_run_updates_row_and_appends_event() {
    let conn = test_db();
    let loop_run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "Test complete".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    let verifier = LoopService::add_verifier_run(
        &conn,
        AddVerifierRunParams {
            loop_id: loop_run.id.clone(),
            session_id: Some("sess-1".into()),
            verifier_type: "command".into(),
            name: "cargo-test".into(),
            command: "cargo test".into(),
        },
    )
    .unwrap();
    assert_eq!(verifier.status, "pending");

    let payload = serde_json::json!({
        "name": "cargo-test",
        "status": "pass",
        "exit_code": 0,
    });

    LoopService::complete_verifier_run(
        &conn,
        &verifier.id,
        "pass",
        Some(0),
        Some("/tmp/out.log"),
        &payload,
    )
    .unwrap();

    // Verify row was updated
    let row: (String, Option<i32>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT status, exit_code, output_path, finished_at FROM verifier_runs WHERE id = ?1",
            rusqlite::params![verifier.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(row.0, "pass");
    assert_eq!(row.1, Some(0));
    assert_eq!(row.2, Some("/tmp/out.log".to_string()));
    assert!(row.3.is_some(), "finished_at should be set");

    // Verify event was appended
    let events = LoopService::list_loop_events(&conn, &loop_run.id).unwrap();
    let completed_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == "verifier_completed")
        .collect();
    assert_eq!(completed_events.len(), 1);
    assert_eq!(
        completed_events[0].payload_json["name"].as_str().unwrap(),
        "cargo-test"
    );
}

#[test]
fn complete_verifier_run_fails_for_nonexistent_id() {
    let conn = test_db();
    // Create a loop so the DB has the tables
    LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("single"),
            goal: "x".into(),
            max_rounds: 1,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    let payload = serde_json::json!({"status": "pass"});
    let result = LoopService::complete_verifier_run(
        &conn,
        "nonexistent-id",
        "pass",
        Some(0),
        None,
        &payload,
    );

    assert!(result.is_err(), "should fail for nonexistent verifier run");
}

// ─── Handoff strictness ──────────────────────────────────────────────────────

#[test]
fn find_handoff_rejects_artifact_missing_schema() {
    let conn = test_db();
    let loop_run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "test".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    let session_id = "sess-maker-1";
    LoopService::add_loop_session(
        &conn,
        AddLoopSessionParams {
            loop_id: loop_run.id.clone(),
            session_id: session_id.into(),
            role: "maker".into(),
            round: 1,
            provider: None,
            status: "running".into(),
        },
    )
    .unwrap();

    // Record handoff with MISSING schema field — should be ignored
    LoopService::record_handoff(
        &conn,
        RecordHandoffParams {
            loop_id: loop_run.id.clone(),
            session_id: session_id.into(),
            artifact_path: None,
            content_json: Some(serde_json::json!({
                "status": "completed",
                "summary": "Done"
            })),
            handoff_status: "completed".into(),
            event_payload: serde_json::json!({}),
            trigger: None,
        },
    )
    .unwrap();

    let result = LoopService::find_handoff_for_sessions(
        &conn,
        &loop_run.id,
        &[session_id.to_string()],
        None,
    )
    .unwrap();

    assert_eq!(
        result, None,
        "handoff without planeai.handoff.v1 schema should be ignored"
    );
}

#[test]
fn find_handoff_rejects_artifact_with_invalid_status() {
    let conn = test_db();
    let loop_run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "test".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    let session_id = "sess-maker-2";
    LoopService::add_loop_session(
        &conn,
        AddLoopSessionParams {
            loop_id: loop_run.id.clone(),
            session_id: session_id.into(),
            role: "maker".into(),
            round: 1,
            provider: None,
            status: "running".into(),
        },
    )
    .unwrap();

    // Record handoff with valid schema but INVALID status
    LoopService::record_handoff(
        &conn,
        RecordHandoffParams {
            loop_id: loop_run.id.clone(),
            session_id: session_id.into(),
            artifact_path: None,
            content_json: Some(serde_json::json!({
                "schema": "planeai.handoff.v1",
                "status": "in_progress",
                "summary": "Still working"
            })),
            handoff_status: "in_progress".into(),
            event_payload: serde_json::json!({}),
            trigger: None,
        },
    )
    .unwrap();

    let result = LoopService::find_handoff_for_sessions(
        &conn,
        &loop_run.id,
        &[session_id.to_string()],
        None,
    )
    .unwrap();

    assert_eq!(
        result, None,
        "handoff with invalid status should be ignored"
    );
}

#[test]
fn find_handoff_accepts_valid_handoff_with_schema_and_status() {
    let conn = test_db();
    let loop_run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "test".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    let session_id = "sess-maker-3";
    LoopService::add_loop_session(
        &conn,
        AddLoopSessionParams {
            loop_id: loop_run.id.clone(),
            session_id: session_id.into(),
            role: "maker".into(),
            round: 1,
            provider: None,
            status: "running".into(),
        },
    )
    .unwrap();

    // Record handoff with valid schema AND valid status
    LoopService::record_handoff(
        &conn,
        RecordHandoffParams {
            loop_id: loop_run.id.clone(),
            session_id: session_id.into(),
            artifact_path: None,
            content_json: Some(serde_json::json!({
                "schema": "planeai.handoff.v1",
                "status": "completed",
                "summary": "All done"
            })),
            handoff_status: "completed".into(),
            event_payload: serde_json::json!({}),
            trigger: None,
        },
    )
    .unwrap();

    let result = LoopService::find_handoff_for_sessions(
        &conn,
        &loop_run.id,
        &[session_id.to_string()],
        None,
    )
    .unwrap();

    assert_eq!(
        result,
        Some((session_id.to_string(), "completed".to_string()))
    );
}

#[test]
fn add_artifact_rejects_handoff_kind() {
    let conn = test_db();
    let loop_run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "test".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    let result = LoopService::add_artifact(
        &conn,
        AddArtifactParams {
            loop_id: loop_run.id.clone(),
            session_id: Some("sess-1".into()),
            kind: "handoff".into(),
            path: None,
            content_json: Some(serde_json::json!({"status": "completed"})),
        },
    );

    assert!(
        result.is_err(),
        "add_artifact must reject kind='handoff' — use record_handoff instead"
    );
}

// ─── Status derivation cannot desync ─────────────────────────────────────────

#[test]
fn persist_snapshot_always_derives_status_from_step_pointer() {
    use planeai_core::loop_recipe::{RecipeKnowledge, RecipeStep, RecipeTools};
    use planeai_core::loop_recipe_service::{
        RecipeRuntime, RecipeSnapshot, SnapshotPolicy,
    };
    use std::collections::BTreeMap;

    let conn = test_db();
    let run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("test"),
            goal: "test derivation".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    // Manually set status to Running (simulating transition_loop(Start))
    conn.execute(
        "UPDATE loop_runs SET status = 'running' WHERE id = ?1",
        rusqlite::params![run.id],
    )
    .unwrap();

    // Build a snapshot with step pointer at a handoff.wait step
    let snapshot = RecipeSnapshot {
        recipe_schema: "planeai.loop.recipe.v1".into(),
        recipe_id: "test".into(),
        recipe_source: "test".into(),
        recipe_path: None,
        inputs: BTreeMap::new(),
        runtime: RecipeRuntime {
            current_step: "wait_step".into(),
            tick_count: 1,
            round: 1,
            created_session_ids: BTreeMap::new(),
            last_error: None,
            last_handoff_consumed_at: None,
            status_override: None,
        },
        policy: SnapshotPolicy {
            max_rounds: 3,
            max_ticks: 50,
            max_sessions: 5,
            merge_policy: "human".into(),
            auto_approve: true,
        },
        roles: BTreeMap::new(),
        steps: vec![RecipeStep {
            id: "wait_step".into(),
            kind: "handoff.wait".into(),
            role: None,
            prompt: None,
            branch: None,
            from: None,
            on: None,
            status: None,
            next: None,
            select: None,
            event_kind: None,
            gates: vec![],
        }],
        knowledge: RecipeKnowledge { files: vec![], instructions: vec![] },
        tools: RecipeTools { required: vec![], optional: vec![] },
    };

    // persist_snapshot should derive Observing from handoff.wait, overriding Running
    LoopService::persist_snapshot(&conn, &run.id, &snapshot).unwrap();
    let updated = LoopService::get_loop(&conn, &run.id).unwrap().unwrap();
    assert_eq!(
        updated.status,
        LoopStatus::Observing,
        "persist_snapshot must overwrite status regardless of previous value"
    );

    // Now manually set status to something else directly (simulating a rogue caller)
    conn.execute(
        "UPDATE loop_runs SET status = 'failed' WHERE id = ?1",
        rusqlite::params![run.id],
    )
    .unwrap();

    // persist_snapshot again — must still derive from step pointer, not keep 'failed'
    LoopService::persist_snapshot(&conn, &run.id, &snapshot).unwrap();
    let updated2 = LoopService::get_loop(&conn, &run.id).unwrap().unwrap();
    assert_eq!(
        updated2.status,
        LoopStatus::Observing,
        "persist_snapshot must always overwrite status from step pointer, never retain stale value"
    );

    // Verify status_override takes precedence over step kind
    let mut snapshot_with_override = snapshot.clone();
    snapshot_with_override.runtime.status_override = Some(LoopStatus::Blocked);
    LoopService::persist_snapshot(&conn, &run.id, &snapshot_with_override).unwrap();
    let updated3 = LoopService::get_loop(&conn, &run.id).unwrap().unwrap();
    assert_eq!(
        updated3.status,
        LoopStatus::Blocked,
        "status_override must take precedence over step-kind derivation"
    );
}

// ─── transition_loop validates derived transition ─────────────────────────────

#[test]
fn transition_loop_rejects_recipe_tick_triggers_on_recipe_driven_loops() {
    use planeai_core::loop_recipe::{RecipeKnowledge, RecipeStep, RecipeTools};
    use planeai_core::loop_recipe_service::{
        RecipeRuntime, RecipeSnapshot, SnapshotPolicy,
    };
    use std::collections::BTreeMap;

    let conn = test_db();
    let run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("test"),
            goal: "test guard".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    // Start the loop so it's in Running state
    LoopService::transition_loop(&conn, &run.id, LoopTrigger::Start).unwrap();

    // Attach a recipe snapshot (makes it recipe-driven)
    let snapshot = RecipeSnapshot {
        recipe_schema: "planeai.loop.recipe.v1".into(),
        recipe_id: "test".into(),
        recipe_source: "test".into(),
        recipe_path: None,
        inputs: BTreeMap::new(),
        runtime: RecipeRuntime {
            current_step: "step1".into(),
            tick_count: 1,
            round: 1,
            created_session_ids: BTreeMap::new(),
            last_error: None,
            last_handoff_consumed_at: None,
            status_override: None,
        },
        policy: SnapshotPolicy {
            max_rounds: 3,
            max_ticks: 50,
            max_sessions: 5,
            merge_policy: "human".into(),
            auto_approve: true,
        },
        roles: BTreeMap::new(),
        steps: vec![RecipeStep {
            id: "step1".into(),
            kind: "session.create".into(),
            role: None,
            prompt: None,
            branch: None,
            from: None,
            on: None,
            status: None,
            next: None,
            select: None,
            event_kind: None,
            gates: vec![],
        }],
        knowledge: RecipeKnowledge { files: vec![], instructions: vec![] },
        tools: RecipeTools { required: vec![], optional: vec![] },
    };
    LoopService::persist_snapshot(&conn, &run.id, &snapshot).unwrap();

    // Recipe-tick triggers (†) should be rejected on recipe-driven loops
    let result = LoopService::transition_loop(&conn, &run.id, LoopTrigger::HandoffWaiting);
    assert!(
        result.is_err(),
        "recipe-tick trigger HandoffWaiting must be rejected on recipe-driven loop"
    );

    let result2 = LoopService::transition_loop(&conn, &run.id, LoopTrigger::MaxTicksExceeded);
    assert!(
        result2.is_err(),
        "recipe-tick trigger MaxTicksExceeded must be rejected on recipe-driven loop"
    );

    // Lifecycle triggers should still work on recipe-driven loops
    let cancel_result = LoopService::transition_loop(&conn, &run.id, LoopTrigger::Cancel);
    assert!(
        cancel_result.is_ok(),
        "lifecycle trigger Cancel must still work on recipe-driven loops"
    );

    // RecipeSetStatus is a valid external operation (e.g., loop stop) — not guarded
    // (skipping here since loop is now Cancelled and RecipeSetStatus requires Running)
}

#[test]
fn persist_snapshot_marks_stale_on_unknown_step_kind() {
    use planeai_core::loop_recipe::{RecipeKnowledge, RecipeStep, RecipeTools};
    use planeai_core::loop_recipe_service::{
        RecipeRuntime, RecipeSnapshot, SnapshotPolicy,
    };
    use std::collections::BTreeMap;

    let conn = test_db();
    let run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("test"),
            goal: "test unknown kind".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    // Set to Running first
    conn.execute(
        "UPDATE loop_runs SET status = 'running' WHERE id = ?1",
        rusqlite::params![run.id],
    )
    .unwrap();

    // Snapshot with an unknown step kind
    let snapshot = RecipeSnapshot {
        recipe_schema: "planeai.loop.recipe.v1".into(),
        recipe_id: "test".into(),
        recipe_source: "test".into(),
        recipe_path: None,
        inputs: BTreeMap::new(),
        runtime: RecipeRuntime {
            current_step: "mystery".into(),
            tick_count: 1,
            round: 1,
            created_session_ids: BTreeMap::new(),
            last_error: None,
            last_handoff_consumed_at: None,
            status_override: None,
        },
        policy: SnapshotPolicy {
            max_rounds: 3,
            max_ticks: 50,
            max_sessions: 5,
            merge_policy: "human".into(),
            auto_approve: true,
        },
        roles: BTreeMap::new(),
        steps: vec![RecipeStep {
            id: "mystery".into(),
            kind: "future.unknown_kind".into(),
            role: None,
            prompt: None,
            branch: None,
            from: None,
            on: None,
            status: None,
            next: None,
            select: None,
            event_kind: None,
            gates: vec![],
        }],
        knowledge: RecipeKnowledge { files: vec![], instructions: vec![] },
        tools: RecipeTools { required: vec![], optional: vec![] },
    };

    // persist_snapshot should mark as Stale (not silently leave Running)
    LoopService::persist_snapshot(&conn, &run.id, &snapshot).unwrap();
    let updated = LoopService::get_loop(&conn, &run.id).unwrap().unwrap();
    assert_eq!(
        updated.status,
        LoopStatus::Stale,
        "unknown step kind must result in Stale status, not silently retain previous status"
    );
}
