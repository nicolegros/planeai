//! Tests for the loop transition table (pure state machine logic + DB integration).

use planeai_core::handoff::HandoffStatus;
use planeai_core::loop_run::*;

// ─── 1. Table-driven transition tests ────────────────────────────────────────

/// Each entry: (label, from_status, trigger, expected_outcome)
/// Expected: Ok(Changed(status)) | Ok(Unchanged) | Err
enum Expected {
    Changed(LoopStatus),
    Unchanged,
    Rejected,
}

const fn changed(s: LoopStatus) -> Expected {
    Expected::Changed(s)
}
const UNCHANGED: Expected = Expected::Unchanged;
const REJECTED: Expected = Expected::Rejected;

macro_rules! transition_cases {
    ($($name:ident: ($from:expr, $trigger:expr) => $expected:expr;)*) => {
        $(
            #[test]
            fn $name() {
                let result = apply(&$from, &$trigger);
                match $expected {
                    Expected::Changed(ref target) => {
                        assert_eq!(result, Ok(TransitionResult::Changed(target.clone())),
                            "expected Changed({:?})", target);
                    }
                    Expected::Unchanged => {
                        assert_eq!(result, Ok(TransitionResult::Unchanged));
                    }
                    Expected::Rejected => {
                        assert!(result.is_err(), "expected Err, got {:?}", result);
                    }
                }
            }
        )*
    };
}

transition_cases! {
    // ─── Start ───────────────────────────────────────────────────────────────
    start_from_draft: (LoopStatus::Draft, LoopTrigger::Start) => changed(LoopStatus::Running);
    start_from_running_rejected: (LoopStatus::Running, LoopTrigger::Start) => REJECTED;
    start_from_cancelled_rejected: (LoopStatus::Cancelled, LoopTrigger::Start) => REJECTED;

    // ─── Cancel ──────────────────────────────────────────────────────────────
    cancel_from_draft: (LoopStatus::Draft, LoopTrigger::Cancel) => changed(LoopStatus::Cancelled);
    cancel_from_running: (LoopStatus::Running, LoopTrigger::Cancel) => changed(LoopStatus::Cancelled);
    cancel_from_observing: (LoopStatus::Observing, LoopTrigger::Cancel) => changed(LoopStatus::Cancelled);
    cancel_from_verifying: (LoopStatus::Verifying, LoopTrigger::Cancel) => changed(LoopStatus::Cancelled);
    cancel_from_blocked: (LoopStatus::Blocked, LoopTrigger::Cancel) => changed(LoopStatus::Cancelled);
    cancel_from_needs_human: (LoopStatus::NeedsHuman, LoopTrigger::Cancel) => changed(LoopStatus::Cancelled);
    cancel_from_stale: (LoopStatus::Stale, LoopTrigger::Cancel) => changed(LoopStatus::Cancelled);
    cancel_from_cancelled_rejected: (LoopStatus::Cancelled, LoopTrigger::Cancel) => REJECTED;
    cancel_from_failed_rejected: (LoopStatus::Failed, LoopTrigger::Cancel) => REJECTED;
    cancel_from_approved_rejected: (LoopStatus::Approved, LoopTrigger::Cancel) => REJECTED;
    cancel_from_merged_rejected: (LoopStatus::Merged, LoopTrigger::Cancel) => REJECTED;
    cancel_from_cleaned_rejected: (LoopStatus::Cleaned, LoopTrigger::Cancel) => REJECTED;

    // ─── HandoffReceived routing ─────────────────────────────────────────────
    handoff_received_completed_from_running: (LoopStatus::Running, LoopTrigger::HandoffReceived(HandoffStatus::Completed)) => changed(LoopStatus::Observing);
    handoff_received_completed_from_observing_unchanged: (LoopStatus::Observing, LoopTrigger::HandoffReceived(HandoffStatus::Completed)) => UNCHANGED;
    handoff_received_blocked_from_running: (LoopStatus::Running, LoopTrigger::HandoffReceived(HandoffStatus::Blocked)) => changed(LoopStatus::Blocked);
    handoff_received_blocked_from_blocked_unchanged: (LoopStatus::Blocked, LoopTrigger::HandoffReceived(HandoffStatus::Blocked)) => UNCHANGED;
    handoff_received_needs_human_from_stale: (LoopStatus::Stale, LoopTrigger::HandoffReceived(HandoffStatus::NeedsHuman)) => changed(LoopStatus::NeedsHuman);
    handoff_received_failed_from_verifying: (LoopStatus::Verifying, LoopTrigger::HandoffReceived(HandoffStatus::Failed)) => changed(LoopStatus::Failed);
    handoff_received_from_draft_rejected: (LoopStatus::Draft, LoopTrigger::HandoffReceived(HandoffStatus::Completed)) => REJECTED;
    handoff_received_from_cancelled_rejected: (LoopStatus::Cancelled, LoopTrigger::HandoffReceived(HandoffStatus::Completed)) => REJECTED;

    // ─── RecipeSetStatus allow-list ──────────────────────────────────────────
    recipe_set_observing: (LoopStatus::Running, LoopTrigger::RecipeSetStatus(LoopStatus::Observing)) => changed(LoopStatus::Observing);
    recipe_set_completed: (LoopStatus::Running, LoopTrigger::RecipeSetStatus(LoopStatus::CompletedUnreviewed)) => changed(LoopStatus::CompletedUnreviewed);
    recipe_set_cancelled: (LoopStatus::Running, LoopTrigger::RecipeSetStatus(LoopStatus::Cancelled)) => changed(LoopStatus::Cancelled);
    recipe_set_running_rejected: (LoopStatus::Running, LoopTrigger::RecipeSetStatus(LoopStatus::Running)) => REJECTED;
    recipe_set_approved_allowed: (LoopStatus::Running, LoopTrigger::RecipeSetStatus(LoopStatus::Approved)) => changed(LoopStatus::Approved);
    recipe_set_from_observing_rejected: (LoopStatus::Observing, LoopTrigger::RecipeSetStatus(LoopStatus::Failed)) => REJECTED;

    // ─── Post-review lifecycle ───────────────────────────────────────────────
    approve_from_completed: (LoopStatus::CompletedUnreviewed, LoopTrigger::Approve) => changed(LoopStatus::Approved);
    approve_from_running_rejected: (LoopStatus::Running, LoopTrigger::Approve) => REJECTED;
    mark_merged_from_approved: (LoopStatus::Approved, LoopTrigger::MarkMerged) => changed(LoopStatus::Merged);
    mark_merged_from_completed_rejected: (LoopStatus::CompletedUnreviewed, LoopTrigger::MarkMerged) => REJECTED;
    mark_cleaned_from_merged: (LoopStatus::Merged, LoopTrigger::MarkCleaned) => changed(LoopStatus::Cleaned);
}

// ─── 2. Property tests (structural invariants) ──────────────────────────────

const ALL_STATUSES: &[LoopStatus] = &[
    LoopStatus::Draft,
    LoopStatus::Running,
    LoopStatus::Observing,
    LoopStatus::Verifying,
    LoopStatus::CompletedUnreviewed,
    LoopStatus::Blocked,
    LoopStatus::NeedsHuman,
    LoopStatus::Stale,
    LoopStatus::Failed,
    LoopStatus::Cancelled,
    LoopStatus::Approved,
    LoopStatus::Merged,
    LoopStatus::Cleaned,
];

/// Terminal states (executor + lifecycle) reject all triggers except the
/// lifecycle-forward chain.
#[test]
fn terminal_states_reject_all_non_lifecycle_triggers() {
    let terminal = [
        LoopStatus::Failed,
        LoopStatus::Cancelled,
        LoopStatus::Approved,
        LoopStatus::Merged,
        LoopStatus::Cleaned,
    ];
    let non_lifecycle_triggers: Vec<LoopTrigger> = vec![
        LoopTrigger::Start,
        LoopTrigger::Cancel,
        LoopTrigger::HandoffReceived(HandoffStatus::Completed),
        LoopTrigger::RecipeSetStatus(LoopStatus::Observing),
    ];

    for status in &terminal {
        for trigger in &non_lifecycle_triggers {
            let result = apply(status, trigger);
            assert!(
                result.is_err(),
                "terminal state {:?} should reject {:?}, but got {:?}",
                status,
                trigger,
                result
            );
        }
    }
}

/// CompletedUnreviewed only allows Approve (not Cancel, not Start, etc.)
#[test]
fn completed_unreviewed_only_allows_approve() {
    let triggers: Vec<LoopTrigger> = vec![
        LoopTrigger::Start,
        LoopTrigger::RecipeSetStatus(LoopStatus::Running),
    ];

    for trigger in &triggers {
        let result = apply(&LoopStatus::CompletedUnreviewed, trigger);
        assert!(
            result.is_err(),
            "CompletedUnreviewed should reject {:?}, but got {:?}",
            trigger,
            result
        );
    }
}

/// Cancel is valid from every non-terminal state.
#[test]
fn cancel_valid_from_all_non_terminal_states() {
    let non_terminal = ALL_STATUSES
        .iter()
        .filter(|s| !s.is_executor_terminal() && !s.is_lifecycle_terminal())
        .collect::<Vec<_>>();

    for status in non_terminal {
        let result = apply(status, &LoopTrigger::Cancel);
        assert_eq!(
            result,
            Ok(TransitionResult::Changed(LoopStatus::Cancelled)),
            "Cancel should be valid from {:?}",
            status
        );
    }
}

// ─── 3. can_tick() ───────────────────────────────────────────────────────────

#[test]
fn can_tick_only_for_active_execution_states() {
    let tickable = [
        LoopStatus::Running,
        LoopStatus::Observing,
        LoopStatus::Verifying,
    ];
    for status in ALL_STATUSES {
        let expected = tickable.contains(status);
        assert_eq!(
            can_tick(status),
            expected,
            "can_tick({:?}) should be {}",
            status,
            expected
        );
    }
}

// ─── 4. InvalidTransition error carries context ──────────────────────────────

#[test]
fn invalid_transition_error_carries_from_and_trigger() {
    let err = apply(&LoopStatus::Running, &LoopTrigger::Start).unwrap_err();
    assert_eq!(err.from, LoopStatus::Running);
    assert_eq!(err.trigger, LoopTrigger::Start);
    // Display includes both
    let msg = err.to_string();
    assert!(msg.contains("running"), "should mention from status: {msg}");
    assert!(msg.contains("Start"), "should mention trigger: {msg}");
}

// ─── 5. Integration: transition_loop() with real DB ──────────────────────────

use planeai_core::loop_service::*;
use planeai_core::services::open_db_at;

fn test_db() -> (rusqlite::Connection, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = open_db_at(&path).unwrap();
    (conn, dir)
}

fn create_draft_loop(conn: &rusqlite::Connection) -> String {
    LoopService::create_loop(
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
    .unwrap()
    .id
}

#[test]
fn transition_loop_persists_status_and_logs_audit_event() {
    let (conn, _dir) = test_db();
    let loop_id = create_draft_loop(&conn);

    let status = LoopService::transition_loop(&conn, &loop_id, LoopTrigger::Start).unwrap();
    assert_eq!(status, LoopStatus::Running);

    let run = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    assert_eq!(run.status, LoopStatus::Running);

    let events = LoopService::list_loop_events(&conn, &loop_id).unwrap();
    let audit = events
        .iter()
        .find(|e| e.kind == "status_transition")
        .unwrap();
    assert_eq!(audit.payload_json["from"], "draft");
    assert_eq!(audit.payload_json["to"], "running");
}

#[test]
fn transition_loop_unchanged_skips_db_write() {
    let (conn, _dir) = test_db();
    let loop_id = create_draft_loop(&conn);

    LoopService::transition_loop(&conn, &loop_id, LoopTrigger::Start).unwrap();
    // Move to Observing via HandoffReceived(Completed)
    LoopService::transition_loop(
        &conn,
        &loop_id,
        LoopTrigger::HandoffReceived(HandoffStatus::Completed),
    )
    .unwrap();

    let events_before = LoopService::list_loop_events(&conn, &loop_id)
        .unwrap()
        .len();

    // No-op: already Observing
    let status = LoopService::transition_loop(
        &conn,
        &loop_id,
        LoopTrigger::HandoffReceived(HandoffStatus::Completed),
    )
    .unwrap();
    assert_eq!(status, LoopStatus::Observing);

    let events_after = LoopService::list_loop_events(&conn, &loop_id)
        .unwrap()
        .len();
    assert_eq!(events_before, events_after, "no new event on unchanged");
}

#[test]
fn transition_loop_invalid_does_not_mutate() {
    let (conn, _dir) = test_db();
    let loop_id = create_draft_loop(&conn);

    let result = LoopService::transition_loop(&conn, &loop_id, LoopTrigger::Approve);
    assert!(result.is_err());

    let run = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    assert_eq!(run.status, LoopStatus::Draft);
}

#[test]
fn transition_loop_not_found_returns_error() {
    let (conn, _dir) = test_db();

    let result = LoopService::transition_loop(&conn, "nonexistent", LoopTrigger::Start);
    assert!(matches!(result.unwrap_err(), TransitionError::NotFound(_)));
}
