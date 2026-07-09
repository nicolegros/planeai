//! Recipe tick runtime — executes one recipe step per tick.
//!
//! Supports v1 step kinds: session.create, session.prompt, handoff.wait,
//! loop.status, loop.event, human.wait.

use planeai_core::loop_recipe::*;
use planeai_core::loop_recipe_service::RecipeSnapshot;
use planeai_core::loop_run::LoopStatus;
use planeai_core::loop_service::LoopService;
use planeai_toon::{field, render, str_val, Value};

/// Execute one recipe step for the given loop. Returns TOON output.
pub fn tick_recipe(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
) -> (String, i32) {
    let _short_id = &loop_id[..std::cmp::min(8, loop_id.len())];

    // Check max_ticks
    if snapshot.runtime.tick_count >= snapshot.policy.max_ticks {
        // Transition to failed
        let _ = LoopService::update_loop_status(conn, loop_id, LoopStatus::Failed);
        let _ = LoopService::append_loop_event(
            conn,
            loop_id,
            "recipe_step_failed",
            &serde_json::json!({"reason": "max_ticks exceeded"}),
        );
        let fields = vec![
            field("error", str_val("max_ticks exceeded")),
            field(
                "loop_tick",
                Value::Object(vec![
                    field("loop_id", str_val(loop_id)),
                    field("status", str_val("failed")),
                ]),
            ),
        ];
        return (render(&fields), 1);
    }

    // Find current step
    let step = match find_step(&snapshot.steps, &snapshot.runtime.current_step) {
        Some(s) => s.clone(),
        None => {
            let fields = vec![field(
                "error",
                str_val(&format!(
                    "recipe step not found: {}",
                    snapshot.runtime.current_step
                )),
            )];
            return (render(&fields), 1);
        }
    };

    // Check if step kind is v1-executable
    if !step.is_v1_executable() {
        let mut help = Vec::new();
        if step.is_recognized() {
            help.push(format!(
                "step kind '{}' is recognized but not executable until a future release",
                step.kind
            ));
        }
        let fields = vec![
            field("error", str_val("unsupported recipe step kind")),
            field("step_id", str_val(&step.id)),
            field("kind", str_val(&step.kind)),
            field("help", Value::List(help)),
        ];
        return (render(&fields), 1);
    }

    // Guard: if the current step is human.wait and the loop is already in
    // NeedsHuman status, return immediately without consuming a tick.
    if step.kind == STEP_HUMAN_WAIT {
        if let Ok(Some(run)) = LoopService::get_loop(conn, loop_id) {
            if run.status == LoopStatus::NeedsHuman {
                let fields = vec![
                    field(
                        "loop_tick",
                        Value::Object(vec![
                            field("loop_id", str_val(loop_id)),
                            field("recipe_id", str_val(&snapshot.recipe_id)),
                            field("step_id", str_val(&step.id)),
                            field("step_kind", str_val(&step.kind)),
                            field("status", str_val("needs_human")),
                        ]),
                    ),
                    field(
                        "next_actions",
                        Value::List(vec![
                            "human review required before the loop can proceed".to_string(),
                        ]),
                    ),
                ];
                return (render(&fields), 0);
            }
        }
    }

    // Increment tick
    snapshot.runtime.tick_count += 1;

    // Dispatch by step kind
    match step.kind.as_str() {
        STEP_SESSION_CREATE => exec_session_create(conn, loop_id, snapshot, &step),
        STEP_SESSION_PROMPT => exec_session_prompt(conn, loop_id, snapshot, &step),
        STEP_HANDOFF_WAIT => exec_handoff_wait(conn, loop_id, snapshot, &step),
        STEP_LOOP_STATUS => exec_loop_status(conn, loop_id, snapshot, &step),
        STEP_LOOP_EVENT => exec_loop_event(conn, loop_id, snapshot, &step),
        STEP_HUMAN_WAIT => exec_human_wait(conn, loop_id, snapshot, &step),
        _ => {
            let fields = vec![
                field("error", str_val("unsupported recipe step kind")),
                field("step_id", str_val(&step.id)),
                field("kind", str_val(&step.kind)),
            ];
            (render(&fields), 1)
        }
    }
}

// ─── Step Executors ──────────────────────────────────────────────────────────

fn exec_session_create(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
) -> (String, i32) {
    let short_id = &loop_id[..std::cmp::min(8, loop_id.len())];
    let role_id = step.role.as_deref().unwrap_or("default");

    // Get role details (clone to avoid borrow conflicts)
    let provider = snapshot
        .roles
        .get(role_id)
        .map(|r| r.provider.clone())
        .unwrap_or_else(|| "default".to_string());
    let isolation = snapshot
        .roles
        .get(role_id)
        .map(|r| r.isolation.clone())
        .unwrap_or_else(|| "worktree".to_string());

    // Render prompt
    let prompt = render_prompt(step.prompt.as_deref().unwrap_or(""), snapshot, loop_id);

    // Record the session creation (we generate an ID; actual session spawn is
    // done by the orchestrator/daemon — here we just register the intent)
    let session_id = uuid::Uuid::new_v4().to_string();

    // Add loop_session
    let params = planeai_core::loop_service::AddLoopSessionParams {
        loop_id: loop_id.to_string(),
        session_id: session_id.clone(),
        role: role_id.to_string(),
        round: snapshot.runtime.round as i64,
        provider: Some(provider.to_string()),
        status: "active".to_string(),
    };
    if let Err(e) = LoopService::add_loop_session(conn, params) {
        return (emit_error(&format!("failed to add loop session: {e}")), 1);
    }

    // Track in snapshot
    snapshot
        .runtime
        .created_session_ids
        .entry(role_id.to_string())
        .or_default()
        .push(session_id.clone());

    // Transition to observing
    let _ = LoopService::update_loop_status(conn, loop_id, LoopStatus::Observing);

    // Append event
    let _ = LoopService::append_loop_event(
        conn,
        loop_id,
        "recipe_step_completed",
        &serde_json::json!({
            "step_id": step.id,
            "kind": step.kind,
            "session_id": session_id,
            "role": role_id,
        }),
    );

    // Advance to next step
    advance_step(snapshot, step);

    // Save updated snapshot
    if let Err(e) = save_snapshot(conn, loop_id, snapshot) {
        return (emit_error(&e), 1);
    }

    let fields = vec![
        field(
            "loop_tick",
            Value::Object(vec![
                field("loop_id", str_val(loop_id)),
                field("recipe_id", str_val(&snapshot.recipe_id)),
                field("step_id", str_val(&step.id)),
                field("step_kind", str_val(&step.kind)),
                field("status", str_val("observing")),
            ]),
        ),
        field(
            "created_sessions",
            Value::Table {
                columns: vec![
                    "id".into(),
                    "role".into(),
                    "provider".into(),
                    "status".into(),
                    "round".into(),
                    "isolation".into(),
                ],
                rows: vec![vec![
                    session_id[..std::cmp::min(8, session_id.len())].to_string(),
                    role_id.to_string(),
                    provider.to_string(),
                    "active".to_string(),
                    snapshot.runtime.round.to_string(),
                    isolation.to_string(),
                ]],
            },
        ),
        field("prompt", str_val(&truncate(&prompt, 500))),
        field(
            "next_actions",
            Value::List(vec![format!(
                "wait for maker handoff, then run `planeai-cli axi loop tick {short_id}`"
            )]),
        ),
    ];
    (render(&fields), 0)
}

fn exec_session_prompt(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
) -> (String, i32) {
    let short_id = &loop_id[..std::cmp::min(8, loop_id.len())];
    let role_id = step.role.as_deref().unwrap_or("default");

    // Find the latest session for this role
    let session_ids = snapshot
        .runtime
        .created_session_ids
        .get(role_id)
        .cloned()
        .unwrap_or_default();

    let session_id = match session_ids.last() {
        Some(id) => id.clone(),
        None => {
            return (
                emit_error(&format!(
                    "no session found for role '{}' — run session.create first",
                    role_id
                )),
                1,
            );
        }
    };

    let prompt = render_prompt(step.prompt.as_deref().unwrap_or(""), snapshot, loop_id);

    // Append event
    let _ = LoopService::append_loop_event(
        conn,
        loop_id,
        "recipe_step_completed",
        &serde_json::json!({
            "step_id": step.id,
            "kind": step.kind,
            "session_id": session_id,
            "role": role_id,
        }),
    );

    // Advance to next step
    advance_step(snapshot, step);
    if let Err(e) = save_snapshot(conn, loop_id, snapshot) {
        return (emit_error(&e), 1);
    }

    let fields = vec![
        field(
            "loop_tick",
            Value::Object(vec![
                field("loop_id", str_val(loop_id)),
                field("recipe_id", str_val(&snapshot.recipe_id)),
                field("step_id", str_val(&step.id)),
                field("step_kind", str_val(&step.kind)),
                field("status", str_val("observing")),
                field("session_id", str_val(&session_id[..std::cmp::min(8, session_id.len())])),
                field("role", str_val(role_id)),
            ]),
        ),
        field("prompt", str_val(&truncate(&prompt, 500))),
        field(
            "next_actions",
            Value::List(vec![format!(
                "run `planeai-cli axi loop tick {short_id}` to continue"
            )]),
        ),
    ];
    (render(&fields), 0)
}

fn exec_handoff_wait(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
) -> (String, i32) {
    let short_id = &loop_id[..std::cmp::min(8, loop_id.len())];
    let role_id = step.from.as_deref().unwrap_or("default");

    // Check for handoff artifacts for this role's sessions
    let session_ids = snapshot
        .runtime
        .created_session_ids
        .get(role_id)
        .cloned()
        .unwrap_or_default();

    // Query loop_artifacts for handoff kind matching any of this role's sessions
    let mut found_handoff: Option<(String, String)> = None; // (session_id, status)
    for sid in &session_ids {
        let query = conn.prepare(
            "SELECT content_json FROM loop_artifacts WHERE loop_id = ?1 AND session_id = ?2 AND kind = 'handoff' ORDER BY created_at DESC LIMIT 1"
        );
        if let Ok(mut stmt) = query {
            if let Ok(Some(json_str)) = stmt.query_row(rusqlite::params![loop_id, sid], |row| {
                row.get::<_, Option<String>>(0)
            }) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    if let Some(status) = val.get("status").and_then(|v| v.as_str()) {
                        found_handoff = Some((sid.clone(), status.to_string()));
                        break;
                    }
                }
            }
        }
    }

    // Also check loop_events for handoff_recorded
    if found_handoff.is_none() {
        if let Ok(events) = LoopService::list_loop_events(conn, loop_id) {
            for event in events.iter().rev() {
                if event.kind == "handoff_recorded" {
                    if let Some(sid) = event
                        .payload_json
                        .get("session_id")
                        .and_then(|v| v.as_str())
                    {
                        if session_ids.iter().any(|s| s == sid) {
                            let status = event
                                .payload_json
                                .get("handoff_status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("completed");
                            found_handoff = Some((sid.to_string(), status.to_string()));
                            break;
                        }
                    }
                }
            }
        }
    }

    match found_handoff {
        None => {
            // No handoff yet — stay on this step, return waiting
            let _ = LoopService::append_loop_event(
                conn,
                loop_id,
                "recipe_step_waiting",
                &serde_json::json!({"step_id": step.id, "waiting_for": "handoff", "role": role_id}),
            );
            if let Err(e) = save_snapshot(conn, loop_id, snapshot) {
                return (emit_error(&e), 1);
            }

            let fields = vec![
                field(
                    "loop_tick",
                    Value::Object(vec![
                        field("loop_id", str_val(loop_id)),
                        field("recipe_id", str_val(&snapshot.recipe_id)),
                        field("step_id", str_val(&step.id)),
                        field("step_kind", str_val(&step.kind)),
                        field("status", str_val("observing")),
                    ]),
                ),
                field(
                    "waiting_for",
                    Value::Object(vec![
                        field("kind", str_val("handoff")),
                        field("role", str_val(role_id)),
                    ]),
                ),
                field(
                    "next_actions",
                    Value::List(vec![format!(
                        "{} should record a planeai.handoff.v1 handoff",
                        role_id
                    )]),
                ),
            ];
            (render(&fields), 0)
        }
        Some((session_id, handoff_status)) => {
            // Handoff found — map status to next step via `on` mapping
            let next_step = step
                .on
                .as_ref()
                .and_then(|m| m.get(&handoff_status))
                .cloned();

            let _ = LoopService::append_loop_event(
                conn,
                loop_id,
                "recipe_step_completed",
                &serde_json::json!({
                    "step_id": step.id,
                    "kind": step.kind,
                    "handoff_status": handoff_status,
                    "session_id": session_id,
                    "next_step": next_step,
                }),
            );

            // Advance to mapped next step
            if let Some(ref ns) = next_step {
                snapshot.runtime.current_step = ns.clone();
            } else {
                advance_step(snapshot, step);
            }
            if let Err(e) = save_snapshot(conn, loop_id, snapshot) {
                return (emit_error(&e), 1);
            }

            let next_step_display = next_step.as_deref().unwrap_or("(end)");

            let fields = vec![
                field(
                    "loop_tick",
                    Value::Object(vec![
                        field("loop_id", str_val(loop_id)),
                        field("recipe_id", str_val(&snapshot.recipe_id)),
                        field("step_id", str_val(&step.id)),
                        field("step_kind", str_val(&step.kind)),
                        field("status", str_val("observing")),
                    ]),
                ),
                field(
                    "matched_handoff",
                    Value::Object(vec![
                        field("session_id", str_val(&session_id[..std::cmp::min(8, session_id.len())])),
                        field("status", str_val(&handoff_status)),
                    ]),
                ),
                field("next_step", str_val(next_step_display)),
                field(
                    "next_actions",
                    Value::List(vec![format!(
                        "run `planeai-cli axi loop tick {short_id}` to apply next step"
                    )]),
                ),
            ];
            (render(&fields), 0)
        }
    }
}

fn exec_loop_status(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
) -> (String, i32) {
    let short_id = &loop_id[..std::cmp::min(8, loop_id.len())];
    let status_str = step.status.as_deref().unwrap_or("observing");

    // Only allow recipe-safe statuses
    let allowed = [
        "observing",
        "completed_unreviewed",
        "blocked",
        "needs_human",
        "failed",
        "cancelled",
    ];
    if !allowed.contains(&status_str) {
        return (
            emit_error(&format!(
                "recipe cannot set status '{}' — only {:?} are allowed",
                status_str, allowed
            )),
            1,
        );
    }

    let new_status = match LoopStatus::parse(status_str) {
        Some(s) => s,
        None => {
            return (
                emit_error(&format!("unknown loop status: {}", status_str)),
                1,
            );
        }
    };

    if let Err(e) = LoopService::update_loop_status(conn, loop_id, new_status.clone()) {
        return (emit_error(&format!("failed to update status: {e}")), 1);
    }

    let _ = LoopService::append_loop_event(
        conn,
        loop_id,
        "recipe_step_completed",
        &serde_json::json!({"step_id": step.id, "kind": step.kind, "status": status_str}),
    );

    // Do not advance further for terminal statuses
    if !new_status.is_executor_terminal() && !new_status.is_intervention_required() {
        advance_step(snapshot, step);
    }
    if let Err(e) = save_snapshot(conn, loop_id, snapshot) {
        return (emit_error(&e), 1);
    }

    let next_action = if new_status.is_executor_terminal() {
        "review the loop output before merging".to_string()
    } else if new_status.is_intervention_required() {
        "human intervention required".to_string()
    } else {
        format!("run `planeai-cli axi loop tick {short_id}` to continue")
    };

    let fields = vec![
        field(
            "loop_tick",
            Value::Object(vec![
                field("loop_id", str_val(loop_id)),
                field("recipe_id", str_val(&snapshot.recipe_id)),
                field("step_id", str_val(&step.id)),
                field("step_kind", str_val(&step.kind)),
                field("status", str_val(status_str)),
            ]),
        ),
        field("state_changed", Value::Bool(true)),
        field("next_actions", Value::List(vec![next_action])),
    ];
    (render(&fields), 0)
}

fn exec_loop_event(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
) -> (String, i32) {
    let short_id = &loop_id[..std::cmp::min(8, loop_id.len())];
    let event_kind = step.event_kind.as_deref().unwrap_or("recipe_event");

    let _ = LoopService::append_loop_event(
        conn,
        loop_id,
        event_kind,
        &serde_json::json!({"step_id": step.id}),
    );

    advance_step(snapshot, step);
    if let Err(e) = save_snapshot(conn, loop_id, snapshot) {
        return (emit_error(&e), 1);
    }

    let fields = vec![
        field(
            "loop_tick",
            Value::Object(vec![
                field("loop_id", str_val(loop_id)),
                field("recipe_id", str_val(&snapshot.recipe_id)),
                field("step_id", str_val(&step.id)),
                field("step_kind", str_val(&step.kind)),
                field("event_kind", str_val(event_kind)),
            ]),
        ),
        field(
            "next_actions",
            Value::List(vec![format!(
                "run `planeai-cli axi loop tick {short_id}` to continue"
            )]),
        ),
    ];
    (render(&fields), 0)
}

fn exec_human_wait(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
) -> (String, i32) {
    let _ = LoopService::update_loop_status(conn, loop_id, LoopStatus::NeedsHuman);
    let _ = LoopService::append_loop_event(
        conn,
        loop_id,
        "recipe_step_completed",
        &serde_json::json!({"step_id": step.id, "kind": step.kind}),
    );

    // Do not advance — human must intervene
    if let Err(e) = save_snapshot(conn, loop_id, snapshot) {
        return (emit_error(&e), 1);
    }

    let fields = vec![
        field(
            "loop_tick",
            Value::Object(vec![
                field("loop_id", str_val(loop_id)),
                field("recipe_id", str_val(&snapshot.recipe_id)),
                field("step_id", str_val(&step.id)),
                field("step_kind", str_val(&step.kind)),
                field("status", str_val("needs_human")),
            ]),
        ),
        field(
            "next_actions",
            Value::List(vec![
                "human review required before the loop can proceed".to_string()
            ]),
        ),
    ];
    (render(&fields), 0)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn find_step<'a>(steps: &'a [RecipeStep], id: &str) -> Option<&'a RecipeStep> {
    steps.iter().find(|s| s.id == id)
}

/// Advance to the next step (sequential ordering in steps vec).
fn advance_step(snapshot: &mut RecipeSnapshot, current: &RecipeStep) {
    // If step has explicit `next`, use it
    if let Some(ref next) = current.next {
        snapshot.runtime.current_step = next.clone();
        return;
    }
    // Otherwise advance sequentially
    let idx = snapshot.steps.iter().position(|s| s.id == current.id);
    if let Some(i) = idx {
        if i + 1 < snapshot.steps.len() {
            snapshot.runtime.current_step = snapshot.steps[i + 1].id.clone();
        }
        // else: stay on current (terminal)
    }
}

/// Save the updated snapshot back to policy_json.
fn save_snapshot(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &RecipeSnapshot,
) -> Result<(), String> {
    let json_str = serde_json::to_string(snapshot)
        .map_err(|e| format!("failed to serialize snapshot: {e}"))?;
    conn.execute(
        "UPDATE loop_runs SET policy_json = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![json_str, chrono::Utc::now().to_rfc3339(), loop_id],
    )
    .map_err(|e| format!("failed to persist snapshot: {e}"))?;
    Ok(())
}

/// Simple template rendering for recipe prompts.
fn render_prompt(template: &str, snapshot: &RecipeSnapshot, loop_id: &str) -> String {
    let mut result = template.to_string();

    // Replace simple variables
    if let Some(goal) = snapshot.inputs.get("goal") {
        result = result.replace("{{ inputs.goal }}", goal);
    }
    if let Some(task_key) = snapshot.inputs.get("task_key") {
        result = result.replace("{{ inputs.task_key }}", task_key);
        // Handle conditional
        result = result.replace("{% if inputs.task_key %}", "");
        result = result.replace("{% endif %}", "");
    } else {
        // Remove conditional block
        while let Some(start) = result.find("{% if inputs.task_key %}") {
            if let Some(end) = result[start..].find("{% endif %}") {
                let remove_end = start + end + "{% endif %}".len();
                result = format!("{}{}", &result[..start], &result[remove_end..]);
            } else {
                break;
            }
        }
    }

    result = result.replace("{{ loop.id }}", loop_id);
    result = result.replace("{{ recipe.id }}", &snapshot.recipe_id);

    // Knowledge files
    let knowledge_str = if snapshot.knowledge.files.is_empty() {
        "(none)".to_string()
    } else {
        snapshot
            .knowledge
            .files
            .iter()
            .map(|f| format!("- Read {}", f))
            .collect::<Vec<_>>()
            .join("\n")
    };
    result = result.replace("{{ knowledge.files }}", &knowledge_str);

    result
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = s
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= max)
            .last()
            .unwrap_or(0);
        format!("{}...", &s[..end])
    }
}

fn emit_error(msg: &str) -> String {
    render(&[field("error", str_val(msg))])
}
