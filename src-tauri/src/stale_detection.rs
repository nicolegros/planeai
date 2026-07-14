//! Stale detection and session observation for loop runs.
//!
//! Deterministic, tick-driven — no background scheduler. Runs on each
//! explicit `loop tick` call before step dispatch.

use planeai_core::loop_recipe_service::{RecipeSnapshot, SessionObservation};
use planeai_core::loop_run::{LoopStatus, LoopTrigger};
use planeai_core::loop_service::LoopService;
use planeai_toon::{field, str_val};

use crate::recipe_tick::{render_tick_result, short_id, TickResult};

/// Check if the loop is stale. Returns TOON output if stale, None otherwise.
pub fn check_stale(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &RecipeSnapshot,
) -> Option<(String, i32)> {
    let stale_ms = snapshot.policy.stale_after_ms.filter(|&ms| ms > 0)?;

    let last_activity = snapshot
        .runtime
        .last_activity_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))?;

    let now = chrono::Utc::now();
    let elapsed_ms = (now - last_activity).num_milliseconds();
    if elapsed_ms <= 0 || (elapsed_ms as u64) < stale_ms {
        return None;
    }

    if let Err(e) = LoopService::transition_loop(
        conn,
        loop_id,
        LoopTrigger::RecipeSetStatus(LoopStatus::Stale),
    ) {
        tracing::warn!(loop_id = %short_id(loop_id), error = %e, "check_stale: failed to transition loop to Stale");
    }
    if let Err(e) = LoopService::append_loop_event(
        conn,
        loop_id,
        "loop_stale_detected",
        &serde_json::json!({
            "stale_after_ms": stale_ms,
            "last_activity_at": snapshot.runtime.last_activity_at,
        }),
    ) {
        tracing::warn!(loop_id = %short_id(loop_id), error = %e, "check_stale: failed to append event");
    }

    Some(render_tick_result(
        loop_id,
        &snapshot.recipe_id,
        TickResult {
            step_id: snapshot.runtime.current_step.clone(),
            step_kind: "(stale_detected)".into(),
            status: "stale".into(),
            extra: vec![
                field("stale_after_ms", str_val(&stale_ms.to_string())),
                field(
                    "last_activity_at",
                    str_val(
                        snapshot
                            .runtime
                            .last_activity_at
                            .as_deref()
                            .unwrap_or("(never)"),
                    ),
                ),
            ],
            next_actions: intervention_next_actions(&LoopStatus::Stale, short_id(loop_id)),
        },
    ))
}

/// Observe loop sessions using bounded DB queries. Emits heartbeat events
/// and refreshes `last_activity_at` when new session-referencing events are
/// found since the last cursor. Returns `true` if any state was mutated.
pub fn observe_sessions(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
) -> bool {
    let all_session_ids: Vec<String> = snapshot
        .runtime
        .created_session_ids
        .values()
        .flatten()
        .cloned()
        .collect();

    if all_session_ids.is_empty() {
        return false;
    }

    let mut mutated = false;
    let mut had_activity = false;

    for session_id in &all_session_ids {
        let prev_cursor = snapshot
            .runtime
            .session_observations
            .get(session_id)
            .and_then(|o| o.last_cursor);

        // First observation: seed cursor without emitting heartbeat.
        let cursor = match prev_cursor {
            Some(c) => c,
            None => {
                let latest = LoopService::latest_event_id(conn, loop_id).unwrap_or(None);
                snapshot.runtime.session_observations.insert(
                    session_id.clone(),
                    SessionObservation {
                        last_cursor: latest,
                    },
                );
                mutated = true;
                continue;
            }
        };

        // Bounded query: count new events for this session since cursor
        let (new_count, max_id) =
            LoopService::count_session_events_since(conn, loop_id, session_id, cursor)
                .unwrap_or((0, None));

        if new_count > 0 {
            // Only refresh activity if the heartbeat event is successfully persisted
            if LoopService::append_loop_event(
                conn,
                loop_id,
                "loop_heartbeat",
                &serde_json::json!({
                    "session_id": session_id,
                    "new_events": new_count,
                }),
            )
            .is_ok()
            {
                had_activity = true;
            } else {
                tracing::warn!(loop_id = %short_id(loop_id), session_id = %&session_id[..8], "observe_sessions: failed to append heartbeat event");
            }
        }

        // Advance cursor
        snapshot.runtime.session_observations.insert(
            session_id.clone(),
            SessionObservation {
                last_cursor: max_id.or(Some(cursor)),
            },
        );
        mutated = true;
    }

    if had_activity {
        refresh_activity(snapshot);
    }

    mutated
}

/// Refresh `last_activity_at` to mark meaningful progress.
pub fn refresh_activity(snapshot: &mut RecipeSnapshot) {
    snapshot.runtime.last_activity_at = Some(chrono::Utc::now().to_rfc3339());
}

/// TOON next_actions for intervention-required loops.
/// Stale loops get actionable guidance; other states get a generic message.
pub fn intervention_next_actions(status: &LoopStatus, loop_short_id: &str) -> Vec<String> {
    if *status == LoopStatus::Stale {
        vec![
            "inspect session output for progress".into(),
            "prompt worker to continue".into(),
            format!("stop loop: `planeai-cli axi loop stop {}`", loop_short_id),
            "mark blocked if external dependency is stalling".into(),
        ]
    } else {
        vec!["loop requires human intervention before it can proceed".into()]
    }
}
