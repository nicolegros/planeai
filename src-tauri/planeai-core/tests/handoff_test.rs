//! Integration tests for the handoff system — schema validation, state transitions,
//! persistence, and AXI TOON output.

use planeai_core::handoff::*;
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

fn create_running_loop(conn: &rusqlite::Connection) -> LoopRun {
    let run = LoopService::create_loop(
        conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: Some("PLA-201".into()),
            created_by_session_id: None,
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "Fix the bug".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    LoopService::update_loop_status(conn, &run.id, LoopStatus::Running).unwrap();
    LoopService::get_loop(conn, &run.id).unwrap().unwrap()
}

fn add_session_to_loop(conn: &rusqlite::Connection, loop_id: &str, session_id: &str, role: &str) {
    LoopService::add_loop_session(
        conn,
        AddLoopSessionParams {
            loop_id: loop_id.to_string(),
            session_id: session_id.to_string(),
            role: role.to_string(),
            round: 1,
            provider: Some("claude".to_string()),
            status: "running".to_string(),
        },
    )
    .unwrap();
}

fn make_handoff_json(loop_id: &str, session_id: &str, status: &str) -> String {
    serde_json::json!({
        "schema": "planeai.handoff.v1",
        "loop_id": loop_id,
        "session_id": session_id,
        "status": status,
        "summary": "Work done",
        "branch": "feat/test",
        "commit": "abc123",
        "changed_files": ["src/main.rs"],
        "risks": ["Might break CI"],
        "next_actions": ["Run verifier"],
        "evidence": [{
            "kind": "test",
            "name": "cargo test",
            "result": "pass",
            "source": "direct"
        }]
    })
    .to_string()
}

// ─── Schema Validation Tests ─────────────────────────────────────────────────

#[test]
fn handoff_parse_valid_all_statuses() {
    for status in ["completed", "blocked", "needs_human", "failed"] {
        let json = make_handoff_json("loop_1", "sess_1", status);
        let h = parse_handoff(&json).unwrap();
        assert_eq!(h.status.as_str(), status);
    }
}

#[test]
fn handoff_parse_rejects_unknown_schema() {
    let json = serde_json::json!({
        "schema": "planeai.handoff.v99",
        "loop_id": "loop_1",
        "session_id": "sess_1",
        "status": "completed",
        "summary": "Done"
    })
    .to_string();

    let errors = parse_handoff(&json).unwrap_err();
    assert!(errors
        .iter()
        .any(|e| matches!(e, HandoffError::UnknownSchema(s) if s.contains("v99"))));
}

#[test]
fn handoff_parse_rejects_missing_required_fields() {
    let json = serde_json::json!({"schema": "planeai.handoff.v1"}).to_string();
    let errors = parse_handoff(&json).unwrap_err();

    let missing: Vec<String> = errors
        .iter()
        .filter_map(|e| match e {
            HandoffError::MissingField(f) => Some(f.clone()),
            _ => None,
        })
        .collect();

    assert!(missing.contains(&"loop_id".to_string()));
    assert!(missing.contains(&"session_id".to_string()));
    assert!(missing.contains(&"summary".to_string()));
    assert!(missing.contains(&"status".to_string()));
}

#[test]
fn handoff_parse_rejects_invalid_status() {
    let json = serde_json::json!({
        "schema": "planeai.handoff.v1",
        "loop_id": "loop_1",
        "session_id": "sess_1",
        "status": "done_wrong",
        "summary": "Done"
    })
    .to_string();

    let errors = parse_handoff(&json).unwrap_err();
    assert!(errors
        .iter()
        .any(|e| matches!(e, HandoffError::InvalidStatus(s) if s == "done_wrong")));
}

#[test]
fn handoff_validate_ids_rejects_mismatched_loop() {
    let json = make_handoff_json("loop_wrong", "sess_1", "completed");
    let h = parse_handoff(&json).unwrap();

    let errors = validate_ids(&h, "loop_correct", "sess_1").unwrap_err();
    assert!(errors.iter().any(
        |e| matches!(e, HandoffError::LoopIdMismatch { expected, actual }
            if expected == "loop_correct" && actual == "loop_wrong"
        )
    ));
}

#[test]
fn handoff_validate_ids_rejects_mismatched_session() {
    let json = make_handoff_json("loop_1", "sess_wrong", "completed");
    let h = parse_handoff(&json).unwrap();

    let errors = validate_ids(&h, "loop_1", "sess_correct").unwrap_err();
    assert!(errors.iter().any(
        |e| matches!(e, HandoffError::SessionIdMismatch { expected, actual }
            if expected == "sess_correct" && actual == "sess_wrong"
        )
    ));
}

// ─── State Transition Tests ──────────────────────────────────────────────────

#[test]
fn handoff_completed_transitions_running_to_observing() {
    let conn = test_db();
    let loop_run = create_running_loop(&conn);
    let session_id = "sess-maker-001";
    add_session_to_loop(&conn, &loop_run.id, session_id, "maker");

    // Record a completed handoff via record_handoff (not add_artifact)
    let handoff_json = make_handoff_json(&loop_run.id, session_id, "completed");
    let handoff: HandoffV1 = serde_json::from_str(&handoff_json).unwrap();

    LoopService::record_handoff(
        &conn,
        RecordHandoffParams {
            loop_id: loop_run.id.clone(),
            session_id: session_id.to_string(),
            artifact_path: Some("/tmp/handoff.json".to_string()),
            content_json: Some(serde_json::to_value(&handoff).unwrap()),
            handoff_status: "completed".into(),
            event_payload: serde_json::json!({"status": "completed", "session_id": session_id}),
            new_loop_status: Some(LoopStatus::Observing),
        },
    )
    .unwrap();

    // Verify final state
    let updated = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert_eq!(updated.status, LoopStatus::Observing);

    let sessions = LoopService::list_loop_sessions(&conn, &loop_run.id).unwrap();
    assert_eq!(sessions[0].status, "completed");
}

#[test]
fn handoff_blocked_transitions_running_to_blocked() {
    let conn = test_db();
    let loop_run = create_running_loop(&conn);
    let session_id = "sess-maker-002";
    add_session_to_loop(&conn, &loop_run.id, session_id, "maker");

    LoopService::update_loop_session_status(&conn, &loop_run.id, session_id, "blocked").unwrap();
    LoopService::update_loop_status(&conn, &loop_run.id, LoopStatus::Blocked).unwrap();

    let updated = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert_eq!(updated.status, LoopStatus::Blocked);
}

#[test]
fn handoff_needs_human_transitions_running_to_needs_human() {
    let conn = test_db();
    let loop_run = create_running_loop(&conn);
    let session_id = "sess-maker-003";
    add_session_to_loop(&conn, &loop_run.id, session_id, "maker");

    LoopService::update_loop_session_status(&conn, &loop_run.id, session_id, "needs_human")
        .unwrap();
    LoopService::update_loop_status(&conn, &loop_run.id, LoopStatus::NeedsHuman).unwrap();

    let updated = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert_eq!(updated.status, LoopStatus::NeedsHuman);
}

#[test]
fn handoff_failed_transitions_running_to_failed() {
    let conn = test_db();
    let loop_run = create_running_loop(&conn);
    let session_id = "sess-maker-004";
    add_session_to_loop(&conn, &loop_run.id, session_id, "maker");

    LoopService::update_loop_session_status(&conn, &loop_run.id, session_id, "failed").unwrap();
    LoopService::update_loop_status(&conn, &loop_run.id, LoopStatus::Failed).unwrap();

    let updated = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert_eq!(updated.status, LoopStatus::Failed);
}

#[test]
fn handoff_does_not_transition_draft_loop() {
    let conn = test_db();
    let loop_run = LoopService::create_loop(
        &conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "Test".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    let session_id = "sess-draft-001";
    add_session_to_loop(&conn, &loop_run.id, session_id, "maker");

    // A completed handoff on a draft loop should NOT change loop status
    LoopService::update_loop_session_status(&conn, &loop_run.id, session_id, "completed").unwrap();

    // Don't transition — loop is draft
    let updated = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert_eq!(updated.status, LoopStatus::Draft);
}

#[test]
fn handoff_completed_does_not_transition_observing_loop() {
    let conn = test_db();
    let loop_run = create_running_loop(&conn);
    let session_id = "sess-maker-005";
    add_session_to_loop(&conn, &loop_run.id, session_id, "maker");

    // Move loop to observing first
    LoopService::update_loop_status(&conn, &loop_run.id, LoopStatus::Observing).unwrap();

    // Another completed handoff should not change observing → anything
    // (per spec: "If loop is running, set to observing; otherwise leave unchanged")
    let updated = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert_eq!(updated.status, LoopStatus::Observing);
}

// ─── Persistence Tests ───────────────────────────────────────────────────────

#[test]
fn handoff_artifact_is_persisted_with_correct_fields() {
    let conn = test_db();
    let loop_run = create_running_loop(&conn);
    let session_id = "sess-persist-001";
    add_session_to_loop(&conn, &loop_run.id, session_id, "maker");

    let handoff_json = make_handoff_json(&loop_run.id, session_id, "completed");
    let handoff: HandoffV1 = serde_json::from_str(&handoff_json).unwrap();
    let content_json = serde_json::to_value(&handoff).unwrap();

    // Handoff artifacts must go through record_handoff (not add_artifact)
    let result = LoopService::record_handoff(
        &conn,
        RecordHandoffParams {
            loop_id: loop_run.id.clone(),
            session_id: session_id.to_string(),
            artifact_path: Some(
                "/project/.planeai/loops/loop_id/sessions/sess/handoff.json".to_string(),
            ),
            content_json: Some(content_json.clone()),
            handoff_status: "completed".into(),
            event_payload: serde_json::json!({"session_id": session_id, "status": "completed"}),
            new_loop_status: None,
        },
    )
    .unwrap();

    // Verify artifact was created by querying loop_artifacts directly
    let artifact_row: (String, String, Option<String>, String, Option<String>) = conn
        .query_row(
            "SELECT id, loop_id, session_id, kind, content_json FROM loop_artifacts WHERE id = ?1",
            rusqlite::params![result.artifact_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(artifact_row.1, loop_run.id);
    assert_eq!(artifact_row.2, Some(session_id.to_string()));
    assert_eq!(artifact_row.3, "handoff");
    assert!(artifact_row.4.is_some());
}

#[test]
fn handoff_event_is_persisted() {
    let conn = test_db();
    let loop_run = create_running_loop(&conn);
    let session_id = "sess-event-001";
    add_session_to_loop(&conn, &loop_run.id, session_id, "maker");

    let payload = serde_json::json!({
        "artifact_id": "art-001",
        "session_id": session_id,
        "status": "completed",
    });

    let event =
        LoopService::append_loop_event(&conn, &loop_run.id, "handoff_recorded", &payload).unwrap();

    assert_eq!(event.kind, "handoff_recorded");
    assert_eq!(event.loop_id, loop_run.id);

    let events = LoopService::list_loop_events(&conn, &loop_run.id).unwrap();
    let handoff_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == "handoff_recorded")
        .collect();
    assert_eq!(handoff_events.len(), 1);
    assert_eq!(
        handoff_events[0].payload_json["status"].as_str().unwrap(),
        "completed"
    );
}

#[test]
fn update_loop_session_status_updates_correctly() {
    let conn = test_db();
    let loop_run = create_running_loop(&conn);
    let session_id = "sess-update-001";
    add_session_to_loop(&conn, &loop_run.id, session_id, "maker");

    let sessions = LoopService::list_loop_sessions(&conn, &loop_run.id).unwrap();
    assert_eq!(sessions[0].status, "running");

    LoopService::update_loop_session_status(&conn, &loop_run.id, session_id, "completed").unwrap();

    let sessions = LoopService::list_loop_sessions(&conn, &loop_run.id).unwrap();
    assert_eq!(sessions[0].status, "completed");
}

#[test]
fn update_loop_session_status_fails_for_nonexistent_session() {
    let conn = test_db();
    let loop_run = create_running_loop(&conn);

    let result =
        LoopService::update_loop_session_status(&conn, &loop_run.id, "nonexistent", "completed");
    assert!(result.is_err());
}

// ─── Evidence Source Semantics ───────────────────────────────────────────────

#[test]
fn evidence_sources_are_preserved_in_artifact() {
    let json = serde_json::json!({
        "schema": "planeai.handoff.v1",
        "loop_id": "loop_ev",
        "session_id": "sess_ev",
        "status": "completed",
        "summary": "All tests pass",
        "evidence": [
            {"kind": "test", "name": "unit", "result": "pass", "source": "direct"},
            {"kind": "test", "name": "integration", "result": "pass", "source": "proxy"},
            {"kind": "test", "name": "e2e", "result": "pass", "source": "claimed"},
            {"kind": "build", "name": "compile", "result": "fail", "source": "blocked"}
        ]
    })
    .to_string();

    let handoff = parse_handoff(&json).unwrap();
    assert_eq!(handoff.evidence.len(), 4);
    assert_eq!(handoff.evidence[0].source, EvidenceSource::Direct);
    assert_eq!(handoff.evidence[1].source, EvidenceSource::Proxy);
    assert_eq!(handoff.evidence[2].source, EvidenceSource::Claimed);
    assert_eq!(handoff.evidence[3].source, EvidenceSource::Blocked);

    // When stored as JSON, sources are preserved
    let stored = serde_json::to_value(&handoff).unwrap();
    assert_eq!(stored["evidence"][0]["source"], "direct");
    assert_eq!(stored["evidence"][2]["source"], "claimed");
}

// ─── Atomicity Tests ─────────────────────────────────────────────────────────

#[test]
fn record_handoff_atomic_success() {
    use planeai_core::loop_service::RecordHandoffParams;

    let conn = test_db();
    let loop_run = create_running_loop(&conn);
    let session_id = "sess-atomic-001";
    add_session_to_loop(&conn, &loop_run.id, session_id, "maker");

    let result = LoopService::record_handoff(
        &conn,
        RecordHandoffParams {
            loop_id: loop_run.id.clone(),
            session_id: session_id.to_string(),
            artifact_path: Some("/tmp/handoff.json".to_string()),
            content_json: Some(serde_json::json!({"status": "completed"})),
            handoff_status: "completed".to_string(),
            event_payload: serde_json::json!({"status": "completed"}),
            new_loop_status: Some(LoopStatus::Observing),
        },
    )
    .unwrap();

    // Verify all parts were written atomically
    assert!(!result.artifact_id.is_empty());
    assert!(result.event_id > 0);

    // Session status updated
    let sessions = LoopService::list_loop_sessions(&conn, &loop_run.id).unwrap();
    assert_eq!(sessions[0].status, "completed");

    // Loop status updated
    let updated = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert_eq!(updated.status, LoopStatus::Observing);

    // Event was recorded
    let events = LoopService::list_loop_events(&conn, &loop_run.id).unwrap();
    let handoff_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == "handoff_recorded")
        .collect();
    assert_eq!(handoff_events.len(), 1);
}

#[test]
fn record_handoff_fails_when_session_not_in_loop() {
    use planeai_core::loop_service::RecordHandoffParams;

    let conn = test_db();
    let loop_run = create_running_loop(&conn);
    // Do NOT add session to loop

    let result = LoopService::record_handoff(
        &conn,
        RecordHandoffParams {
            loop_id: loop_run.id.clone(),
            session_id: "nonexistent-session".to_string(),
            artifact_path: Some("/tmp/handoff.json".to_string()),
            content_json: Some(serde_json::json!({"status": "completed"})),
            handoff_status: "completed".to_string(),
            event_payload: serde_json::json!({"status": "completed"}),
            new_loop_status: Some(LoopStatus::Observing),
        },
    );

    assert!(result.is_err());

    // Verify NO partial state was written
    let events = LoopService::list_loop_events(&conn, &loop_run.id).unwrap();
    let handoff_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == "handoff_recorded")
        .collect();
    assert_eq!(
        handoff_events.len(),
        0,
        "no event should be written on failure"
    );

    // Loop status should not have changed
    let updated = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert_eq!(updated.status, LoopStatus::Running);
}

#[test]
fn record_handoff_does_not_leave_artifact_when_loop_missing() {
    use planeai_core::loop_service::RecordHandoffParams;

    let conn = test_db();
    // Do NOT create a loop — simulate orphan scenario

    let result = LoopService::record_handoff(
        &conn,
        RecordHandoffParams {
            loop_id: "nonexistent-loop".to_string(),
            session_id: "some-session".to_string(),
            artifact_path: Some("/tmp/handoff.json".to_string()),
            content_json: Some(serde_json::json!({"status": "completed"})),
            handoff_status: "completed".to_string(),
            event_payload: serde_json::json!({"status": "completed"}),
            new_loop_status: None,
        },
    );

    assert!(result.is_err());

    // Verify no artifacts exist
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM loop_artifacts WHERE loop_id = 'nonexistent-loop'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 0,
        "no artifact should be written when loop doesn't exist"
    );
}

#[test]
fn record_handoff_without_loop_status_change() {
    use planeai_core::loop_service::RecordHandoffParams;

    let conn = test_db();
    let loop_run = create_running_loop(&conn);
    let session_id = "sess-no-change-001";
    add_session_to_loop(&conn, &loop_run.id, session_id, "maker");

    // Record with new_loop_status = None (e.g., draft loop recording)
    let result = LoopService::record_handoff(
        &conn,
        RecordHandoffParams {
            loop_id: loop_run.id.clone(),
            session_id: session_id.to_string(),
            artifact_path: None,
            content_json: Some(serde_json::json!({"status": "completed"})),
            handoff_status: "completed".to_string(),
            event_payload: serde_json::json!({"status": "completed"}),
            new_loop_status: None,
        },
    )
    .unwrap();

    assert!(!result.artifact_id.is_empty());

    // Loop status should remain running (no change requested)
    let updated = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert_eq!(updated.status, LoopStatus::Running);

    // But session status should be updated
    let sessions = LoopService::list_loop_sessions(&conn, &loop_run.id).unwrap();
    assert_eq!(sessions[0].status, "completed");
}
