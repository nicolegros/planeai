//! Tests for the loop transition table (pure state machine logic).

use planeai_core::handoff::HandoffStatus;
use planeai_core::loop_run::*;

// ─── 1. Happy path: canonical transitions ────────────────────────────────────

#[test]
fn start_from_draft_transitions_to_running() {
    let result = apply(&LoopStatus::Draft, &LoopTrigger::Start);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Running)));
}

#[test]
fn cancel_from_running_transitions_to_cancelled() {
    let result = apply(&LoopStatus::Running, &LoopTrigger::Cancel);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Cancelled)));
}

#[test]
fn cancel_from_observing_transitions_to_cancelled() {
    let result = apply(&LoopStatus::Observing, &LoopTrigger::Cancel);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Cancelled)));
}

#[test]
fn cancel_from_needs_human_transitions_to_cancelled() {
    let result = apply(&LoopStatus::NeedsHuman, &LoopTrigger::Cancel);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Cancelled)));
}

#[test]
fn handoff_waiting_from_running_transitions_to_observing() {
    let result = apply(&LoopStatus::Running, &LoopTrigger::HandoffWaiting);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Observing)));
}

#[test]
fn handoff_consumed_from_observing_transitions_to_running() {
    let result = apply(&LoopStatus::Observing, &LoopTrigger::HandoffConsumed);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Running)));
}

#[test]
fn gates_started_from_running_transitions_to_verifying() {
    let result = apply(&LoopStatus::Running, &LoopTrigger::GatesStarted);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Verifying)));
}

#[test]
fn round_blocked_from_running_transitions_to_blocked() {
    let result = apply(&LoopStatus::Running, &LoopTrigger::RoundBlocked);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Blocked)));
}

#[test]
fn session_limit_reached_from_running_transitions_to_needs_human() {
    let result = apply(&LoopStatus::Running, &LoopTrigger::SessionLimitReached);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::NeedsHuman)));
}

#[test]
fn max_ticks_exceeded_from_running_transitions_to_failed() {
    let result = apply(&LoopStatus::Running, &LoopTrigger::MaxTicksExceeded);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Failed)));
}

#[test]
fn human_wait_reached_from_running_transitions_to_needs_human() {
    let result = apply(&LoopStatus::Running, &LoopTrigger::HumanWaitReached);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::NeedsHuman)));
}

#[test]
fn approve_from_completed_unreviewed_transitions_to_approved() {
    let result = apply(&LoopStatus::CompletedUnreviewed, &LoopTrigger::Approve);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Approved)));
}

#[test]
fn mark_merged_from_approved_transitions_to_merged() {
    let result = apply(&LoopStatus::Approved, &LoopTrigger::MarkMerged);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Merged)));
}

#[test]
fn mark_cleaned_from_merged_transitions_to_cleaned() {
    let result = apply(&LoopStatus::Merged, &LoopTrigger::MarkCleaned);
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Cleaned)));
}

// ─── 2. Invalid transitions are rejected ─────────────────────────────────────

#[test]
fn start_from_running_is_invalid() {
    let result = apply(&LoopStatus::Running, &LoopTrigger::Start);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.from, LoopStatus::Running);
    assert_eq!(err.trigger, LoopTrigger::Start);
}

#[test]
fn cancel_from_cancelled_is_invalid() {
    let result = apply(&LoopStatus::Cancelled, &LoopTrigger::Cancel);
    assert!(result.is_err());
}

#[test]
fn cancel_from_approved_is_invalid() {
    let result = apply(&LoopStatus::Approved, &LoopTrigger::Cancel);
    assert!(result.is_err());
}

#[test]
fn cancel_from_merged_is_invalid() {
    let result = apply(&LoopStatus::Merged, &LoopTrigger::Cancel);
    assert!(result.is_err());
}

#[test]
fn cancel_from_failed_is_invalid() {
    let result = apply(&LoopStatus::Failed, &LoopTrigger::Cancel);
    assert!(result.is_err());
}

#[test]
fn gates_started_from_draft_is_invalid() {
    let result = apply(&LoopStatus::Draft, &LoopTrigger::GatesStarted);
    assert!(result.is_err());
}

#[test]
fn handoff_consumed_from_running_is_unchanged() {
    let result = apply(&LoopStatus::Running, &LoopTrigger::HandoffConsumed);
    assert_eq!(result, Ok(TransitionResult::Unchanged));
}

#[test]
fn approve_from_running_is_invalid() {
    let result = apply(&LoopStatus::Running, &LoopTrigger::Approve);
    assert!(result.is_err());
}

#[test]
fn mark_merged_from_completed_unreviewed_is_invalid() {
    let result = apply(&LoopStatus::CompletedUnreviewed, &LoopTrigger::MarkMerged);
    assert!(result.is_err());
}

// ─── 3. No-ops return Unchanged ──────────────────────────────────────────────

#[test]
fn handoff_waiting_from_observing_is_unchanged() {
    let result = apply(&LoopStatus::Observing, &LoopTrigger::HandoffWaiting);
    assert_eq!(result, Ok(TransitionResult::Unchanged));
}

#[test]
fn handoff_received_completed_from_observing_is_unchanged() {
    let result = apply(
        &LoopStatus::Observing,
        &LoopTrigger::HandoffReceived(HandoffStatus::Completed),
    );
    assert_eq!(result, Ok(TransitionResult::Unchanged));
}

#[test]
fn handoff_received_blocked_from_blocked_is_unchanged() {
    let result = apply(
        &LoopStatus::Blocked,
        &LoopTrigger::HandoffReceived(HandoffStatus::Blocked),
    );
    assert_eq!(result, Ok(TransitionResult::Unchanged));
}

// ─── 4. RecipeSetStatus allow-list ───────────────────────────────────────────

#[test]
fn recipe_set_status_observing_from_running_succeeds() {
    let result = apply(
        &LoopStatus::Running,
        &LoopTrigger::RecipeSetStatus(LoopStatus::Observing),
    );
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Observing)));
}

#[test]
fn recipe_set_status_completed_unreviewed_from_running_succeeds() {
    let result = apply(
        &LoopStatus::Running,
        &LoopTrigger::RecipeSetStatus(LoopStatus::CompletedUnreviewed),
    );
    assert_eq!(
        result,
        Ok(TransitionResult::Changed(LoopStatus::CompletedUnreviewed))
    );
}

#[test]
fn recipe_set_status_running_from_running_is_invalid() {
    // Running is NOT in the allow-list
    let result = apply(
        &LoopStatus::Running,
        &LoopTrigger::RecipeSetStatus(LoopStatus::Running),
    );
    assert!(result.is_err());
}

#[test]
fn recipe_set_status_approved_from_running_is_invalid() {
    // Approved is NOT in the allow-list
    let result = apply(
        &LoopStatus::Running,
        &LoopTrigger::RecipeSetStatus(LoopStatus::Approved),
    );
    assert!(result.is_err());
}

#[test]
fn recipe_set_status_from_observing_is_invalid() {
    // RecipeSetStatus only works from Running
    let result = apply(
        &LoopStatus::Observing,
        &LoopTrigger::RecipeSetStatus(LoopStatus::Failed),
    );
    assert!(result.is_err());
}

// ─── 5. HandoffReceived routing ──────────────────────────────────────────────

#[test]
fn handoff_received_completed_from_running_transitions_to_observing() {
    let result = apply(
        &LoopStatus::Running,
        &LoopTrigger::HandoffReceived(HandoffStatus::Completed),
    );
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Observing)));
}

#[test]
fn handoff_received_blocked_from_running_transitions_to_blocked() {
    let result = apply(
        &LoopStatus::Running,
        &LoopTrigger::HandoffReceived(HandoffStatus::Blocked),
    );
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Blocked)));
}

#[test]
fn handoff_received_needs_human_from_stale_transitions_to_needs_human() {
    let result = apply(
        &LoopStatus::Stale,
        &LoopTrigger::HandoffReceived(HandoffStatus::NeedsHuman),
    );
    assert_eq!(
        result,
        Ok(TransitionResult::Changed(LoopStatus::NeedsHuman))
    );
}

#[test]
fn handoff_received_failed_from_verifying_transitions_to_failed() {
    let result = apply(
        &LoopStatus::Verifying,
        &LoopTrigger::HandoffReceived(HandoffStatus::Failed),
    );
    assert_eq!(result, Ok(TransitionResult::Changed(LoopStatus::Failed)));
}

#[test]
fn handoff_received_from_draft_is_invalid() {
    let result = apply(
        &LoopStatus::Draft,
        &LoopTrigger::HandoffReceived(HandoffStatus::Completed),
    );
    assert!(result.is_err());
}

#[test]
fn handoff_received_from_cancelled_is_invalid() {
    let result = apply(
        &LoopStatus::Cancelled,
        &LoopTrigger::HandoffReceived(HandoffStatus::Completed),
    );
    assert!(result.is_err());
}

// ─── 6. can_tick() ───────────────────────────────────────────────────────────

#[test]
fn can_tick_running_is_true() {
    assert!(can_tick(&LoopStatus::Running));
}

#[test]
fn can_tick_observing_is_true() {
    assert!(can_tick(&LoopStatus::Observing));
}

#[test]
fn can_tick_verifying_is_true() {
    assert!(can_tick(&LoopStatus::Verifying));
}

#[test]
fn can_tick_draft_is_false() {
    assert!(!can_tick(&LoopStatus::Draft));
}

#[test]
fn can_tick_cancelled_is_false() {
    assert!(!can_tick(&LoopStatus::Cancelled));
}

#[test]
fn can_tick_needs_human_is_false() {
    assert!(!can_tick(&LoopStatus::NeedsHuman));
}

#[test]
fn can_tick_approved_is_false() {
    assert!(!can_tick(&LoopStatus::Approved));
}

// ─── 7. transition_loop() integration ────────────────────────────────────────

use planeai_core::loop_service::*;
use planeai_core::services::open_db_at;

fn test_db() -> rusqlite::Connection {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = open_db_at(&path).unwrap();
    std::mem::forget(dir);
    conn
}

fn create_draft_loop(conn: &rusqlite::Connection) -> String {
    let run = LoopService::create_loop(
        conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "test goal".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();
    run.id
}

#[test]
fn transition_loop_persists_status_and_logs_audit_event() {
    let conn = test_db();
    let loop_id = create_draft_loop(&conn);

    // Transition Draft → Running
    let result = LoopService::transition_loop(&conn, &loop_id, LoopTrigger::Start);
    assert!(result.is_ok());
    let status = result.unwrap();
    assert_eq!(status, LoopStatus::Running);

    // Verify DB persisted the new status
    let run = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    assert_eq!(run.status, LoopStatus::Running);

    // Verify an audit event was logged
    let events = LoopService::list_loop_events(&conn, &loop_id).unwrap();
    let transition_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == "status_transition")
        .collect();
    assert_eq!(transition_events.len(), 1);
    let payload = &transition_events[0].payload_json;
    assert_eq!(payload["from"], "draft");
    assert_eq!(payload["to"], "running");
    assert_eq!(payload["trigger"], "Start");
}

#[test]
fn transition_loop_unchanged_skips_db_write_and_audit() {
    let conn = test_db();
    let loop_id = create_draft_loop(&conn);

    // Move to Running first
    LoopService::transition_loop(&conn, &loop_id, LoopTrigger::Start).unwrap();

    // Move to Observing
    LoopService::transition_loop(&conn, &loop_id, LoopTrigger::HandoffWaiting).unwrap();

    let run_before = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    let events_before = LoopService::list_loop_events(&conn, &loop_id).unwrap();

    // Fire HandoffWaiting again while already Observing — should be Unchanged
    let result = LoopService::transition_loop(&conn, &loop_id, LoopTrigger::HandoffWaiting);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), LoopStatus::Observing);

    // Verify: no new event, updated_at unchanged
    let run_after = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    let events_after = LoopService::list_loop_events(&conn, &loop_id).unwrap();

    assert_eq!(run_before.updated_at, run_after.updated_at);
    assert_eq!(events_before.len(), events_after.len());
}

#[test]
fn transition_loop_invalid_returns_error_without_mutating() {
    let conn = test_db();
    let loop_id = create_draft_loop(&conn);

    // Try to cancel from Draft — but wait, Cancel IS valid from Draft.
    // Try GatesStarted from Draft — that's invalid.
    let result = LoopService::transition_loop(&conn, &loop_id, LoopTrigger::GatesStarted);
    assert!(result.is_err());

    // Verify status unchanged
    let run = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    assert_eq!(run.status, LoopStatus::Draft);

    // Verify no events logged
    let events = LoopService::list_loop_events(&conn, &loop_id).unwrap();
    assert!(
        events.iter().all(|e| e.kind != "status_transition"),
        "no transition event should be logged on invalid attempt"
    );
}

#[test]
fn transition_loop_not_found_returns_error() {
    let conn = test_db();

    let result =
        LoopService::transition_loop(&conn, "nonexistent-loop-id", LoopTrigger::Start);
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), TransitionError::NotFound(_)),
        "expected NotFound error for nonexistent loop"
    );
}
