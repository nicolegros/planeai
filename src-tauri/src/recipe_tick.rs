//! Recipe tick runtime — executes one recipe step per tick.
//!
//! Each step executor returns a [`TickResult`] describing what happened.
//! The single [`render_tick_result`] function converts it to TOON output,
//! eliminating repetitive field-construction boilerplate.

use planeai_core::loop_recipe::*;
use planeai_core::loop_recipe_service::RecipeSnapshot;
use planeai_core::loop_run::LoopStatus;
use planeai_core::loop_service::LoopService;
use planeai_toon::{field, render, str_val, Field, Value};

// ─── TickResult ──────────────────────────────────────────────────────────────

/// The outcome of executing one recipe step. Converted to TOON at the boundary.
struct TickResult {
    step_id: String,
    step_kind: String,
    status: String,
    extra: Vec<Field>,
    next_actions: Vec<String>,
}

/// Render a TickResult into TOON output.
fn render_tick_result(loop_id: &str, recipe_id: &str, result: TickResult) -> (String, i32) {
    let tick_fields = vec![
        field("loop_id", str_val(loop_id)),
        field("recipe_id", str_val(recipe_id)),
        field("step_id", str_val(&result.step_id)),
        field("step_kind", str_val(&result.step_kind)),
        field("status", str_val(&result.status)),
    ];

    let mut fields = vec![field("loop_tick", Value::Object(tick_fields))];
    fields.extend(result.extra);
    fields.push(field("next_actions", Value::List(result.next_actions)));
    (render(&fields), 0)
}

fn render_error(msg: &str) -> (String, i32) {
    (render(&[field("error", str_val(msg))]), 1)
}

// ─── Main Entry ──────────────────────────────────────────────────────────────

/// Execute one recipe step for the given loop. Returns TOON output.
pub fn tick_recipe(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
) -> (String, i32) {
    // Check max_ticks
    if snapshot.runtime.tick_count >= snapshot.policy.max_ticks {
        let _ = LoopService::update_loop_status(conn, loop_id, LoopStatus::Failed);
        let _ = LoopService::append_loop_event(
            conn,
            loop_id,
            "recipe_step_failed",
            &serde_json::json!({"reason": "max_ticks exceeded"}),
        );
        return (
            render(&[
                field("error", str_val("max_ticks exceeded")),
                field(
                    "loop_tick",
                    Value::Object(vec![
                        field("loop_id", str_val(loop_id)),
                        field("status", str_val("failed")),
                    ]),
                ),
            ]),
            1,
        );
    }

    // Find current step
    let step = match find_step(&snapshot.steps, &snapshot.runtime.current_step) {
        Some(s) => s.clone(),
        None => {
            return render_error(&format!(
                "recipe step not found: {}",
                snapshot.runtime.current_step
            ));
        }
    };

    // Check if step kind is v1-executable
    if !step.is_v1_executable() {
        let help = if step.is_recognized() {
            format!(
                "step kind '{}' is recognized but not executable until a future release",
                step.kind
            )
        } else {
            format!("step kind '{}' is unknown", step.kind)
        };
        return (
            render(&[
                field("error", str_val("unsupported recipe step kind")),
                field("step_id", str_val(&step.id)),
                field("kind", str_val(&step.kind)),
                field("help", Value::List(vec![help])),
            ]),
            1,
        );
    }

    // Guard: if the loop is already in an intervention-required status,
    // don't consume another tick for steps that would set that status.
    if step.kind == STEP_HUMAN_WAIT || step.kind == STEP_LOOP_STATUS {
        if let Ok(Some(run)) = LoopService::get_loop(conn, loop_id) {
            if run.status.is_intervention_required() {
                return render_tick_result(
                    loop_id,
                    &snapshot.recipe_id,
                    TickResult {
                        step_id: step.id.clone(),
                        step_kind: step.kind.clone(),
                        status: run.status.as_str().to_string(),
                        extra: vec![],
                        next_actions: vec![
                            "human intervention required before the loop can proceed".into(),
                        ],
                    },
                );
            }
        }
    }

    // Increment tick
    snapshot.runtime.tick_count += 1;

    // Dispatch by step kind
    let result = match step.kind.as_str() {
        STEP_SESSION_CREATE => exec_session_create(conn, loop_id, snapshot, &step),
        STEP_SESSION_PROMPT => exec_session_prompt(conn, loop_id, snapshot, &step),
        STEP_HANDOFF_WAIT => exec_handoff_wait(conn, loop_id, snapshot, &step),
        STEP_LOOP_STATUS => exec_loop_status(conn, loop_id, snapshot, &step),
        STEP_LOOP_EVENT => exec_loop_event(conn, loop_id, snapshot, &step),
        STEP_HUMAN_WAIT => exec_human_wait(conn, loop_id, snapshot, &step),
        _ => return render_error("unsupported recipe step kind"),
    };

    match result {
        Ok(tr) => render_tick_result(loop_id, &snapshot.recipe_id, tr),
        Err(msg) => render_error(&msg),
    }
}

// ─── Step Executors ──────────────────────────────────────────────────────────

fn exec_session_create(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
) -> Result<TickResult, String> {
    let short_id = &loop_id[..std::cmp::min(8, loop_id.len())];
    let role_id = step.role.as_deref().unwrap_or("default");

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

    let prompt = render_prompt(step.prompt.as_deref().unwrap_or(""), snapshot, loop_id);
    let session_id = uuid::Uuid::new_v4().to_string();

    let params = planeai_core::loop_service::AddLoopSessionParams {
        loop_id: loop_id.to_string(),
        session_id: session_id.clone(),
        role: role_id.to_string(),
        round: snapshot.runtime.round as i64,
        provider: Some(provider.clone()),
        status: "active".to_string(),
    };
    LoopService::add_loop_session(conn, params)
        .map_err(|e| format!("failed to add loop session: {e}"))?;

    snapshot
        .runtime
        .created_session_ids
        .entry(role_id.to_string())
        .or_default()
        .push(session_id.clone());

    let _ = LoopService::update_loop_status(conn, loop_id, LoopStatus::Observing);
    let _ = LoopService::append_loop_event(
        conn,
        loop_id,
        "recipe_step_completed",
        &serde_json::json!({
            "step_id": step.id, "kind": step.kind,
            "session_id": session_id, "role": role_id,
        }),
    );

    advance_step(snapshot, step);
    save_snapshot(conn, loop_id, snapshot)?;

    Ok(TickResult {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "observing".into(),
        extra: vec![
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
                        provider,
                        "active".to_string(),
                        snapshot.runtime.round.to_string(),
                        isolation,
                    ]],
                },
            ),
            field("prompt", str_val(&truncate(&prompt, 500))),
        ],
        next_actions: vec![format!(
            "wait for maker handoff, then run `planeai-cli axi loop tick {short_id}`"
        )],
    })
}

fn exec_session_prompt(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
) -> Result<TickResult, String> {
    let short_id = &loop_id[..std::cmp::min(8, loop_id.len())];
    let role_id = step.role.as_deref().unwrap_or("default");

    let session_id = snapshot
        .runtime
        .created_session_ids
        .get(role_id)
        .and_then(|ids| ids.last())
        .cloned()
        .ok_or_else(|| {
            format!(
                "no session found for role '{}' — run session.create first",
                role_id
            )
        })?;

    let prompt = render_prompt(step.prompt.as_deref().unwrap_or(""), snapshot, loop_id);

    let _ = LoopService::append_loop_event(
        conn,
        loop_id,
        "recipe_step_completed",
        &serde_json::json!({
            "step_id": step.id, "kind": step.kind,
            "session_id": session_id, "role": role_id,
        }),
    );

    advance_step(snapshot, step);
    save_snapshot(conn, loop_id, snapshot)?;

    Ok(TickResult {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "observing".into(),
        extra: vec![
            field(
                "session_id",
                str_val(&session_id[..std::cmp::min(8, session_id.len())]),
            ),
            field("role", str_val(role_id)),
            field("prompt", str_val(&truncate(&prompt, 500))),
        ],
        next_actions: vec![format!(
            "run `planeai-cli axi loop tick {short_id}` to continue"
        )],
    })
}

fn exec_handoff_wait(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
) -> Result<TickResult, String> {
    let short_id = &loop_id[..std::cmp::min(8, loop_id.len())];
    let role_id = step.from.as_deref().unwrap_or("default");

    let session_ids = snapshot
        .runtime
        .created_session_ids
        .get(role_id)
        .cloned()
        .unwrap_or_default();

    // Use LoopService to find handoff (primary: artifact query)
    let found_handoff = LoopService::find_handoff_for_sessions(conn, loop_id, &session_ids)
        .map_err(|e| format!("handoff query failed: {e}"))?;

    // Fallback: check loop_events for handoff_recorded
    let found_handoff = match found_handoff {
        Some(h) => Some(h),
        None => find_handoff_from_events(conn, loop_id, &session_ids),
    };

    match found_handoff {
        None => {
            let _ = LoopService::append_loop_event(
                conn,
                loop_id,
                "recipe_step_waiting",
                &serde_json::json!({"step_id": step.id, "waiting_for": "handoff", "role": role_id}),
            );
            save_snapshot(conn, loop_id, snapshot)?;

            Ok(TickResult {
                step_id: step.id.clone(),
                step_kind: step.kind.clone(),
                status: "observing".into(),
                extra: vec![field(
                    "waiting_for",
                    Value::Object(vec![
                        field("kind", str_val("handoff")),
                        field("role", str_val(role_id)),
                    ]),
                )],
                next_actions: vec![format!(
                    "{} should record a planeai.handoff.v1 handoff",
                    role_id
                )],
            })
        }
        Some((session_id, handoff_status)) => {
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
                    "step_id": step.id, "kind": step.kind,
                    "handoff_status": handoff_status,
                    "session_id": session_id, "next_step": next_step,
                }),
            );

            if let Some(ref ns) = next_step {
                snapshot.runtime.current_step = ns.clone();
            } else {
                advance_step(snapshot, step);
            }
            save_snapshot(conn, loop_id, snapshot)?;

            let next_step_display = next_step.as_deref().unwrap_or("(end)");

            Ok(TickResult {
                step_id: step.id.clone(),
                step_kind: step.kind.clone(),
                status: "observing".into(),
                extra: vec![
                    field(
                        "matched_handoff",
                        Value::Object(vec![
                            field(
                                "session_id",
                                str_val(&session_id[..std::cmp::min(8, session_id.len())]),
                            ),
                            field("status", str_val(&handoff_status)),
                        ]),
                    ),
                    field("next_step", str_val(next_step_display)),
                ],
                next_actions: vec![format!(
                    "run `planeai-cli axi loop tick {short_id}` to apply next step"
                )],
            })
        }
    }
}

fn exec_loop_status(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
) -> Result<TickResult, String> {
    let short_id = &loop_id[..std::cmp::min(8, loop_id.len())];
    let status_str = step.status.as_deref().unwrap_or("observing");

    const ALLOWED: &[&str] = &[
        "observing",
        "completed_unreviewed",
        "blocked",
        "needs_human",
        "failed",
        "cancelled",
    ];
    if !ALLOWED.contains(&status_str) {
        return Err(format!(
            "recipe cannot set status '{}' — only {:?} are allowed",
            status_str, ALLOWED
        ));
    }

    let new_status = LoopStatus::parse(status_str)
        .ok_or_else(|| format!("unknown loop status: {}", status_str))?;

    LoopService::update_loop_status(conn, loop_id, new_status.clone())
        .map_err(|e| format!("failed to update status: {e}"))?;

    let _ = LoopService::append_loop_event(
        conn,
        loop_id,
        "recipe_step_completed",
        &serde_json::json!({"step_id": step.id, "kind": step.kind, "status": status_str}),
    );

    if !new_status.is_executor_terminal() && !new_status.is_intervention_required() {
        advance_step(snapshot, step);
    }
    save_snapshot(conn, loop_id, snapshot)?;

    let next_action = if new_status.is_executor_terminal() {
        "review the loop output before merging".to_string()
    } else if new_status.is_intervention_required() {
        "human intervention required".to_string()
    } else {
        format!("run `planeai-cli axi loop tick {short_id}` to continue")
    };

    Ok(TickResult {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: status_str.to_string(),
        extra: vec![field("state_changed", Value::Bool(true))],
        next_actions: vec![next_action],
    })
}

fn exec_loop_event(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
) -> Result<TickResult, String> {
    let short_id = &loop_id[..std::cmp::min(8, loop_id.len())];
    let event_kind = step.event_kind.as_deref().unwrap_or("recipe_event");

    let _ = LoopService::append_loop_event(
        conn,
        loop_id,
        event_kind,
        &serde_json::json!({"step_id": step.id}),
    );

    advance_step(snapshot, step);
    save_snapshot(conn, loop_id, snapshot)?;

    Ok(TickResult {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "observing".into(),
        extra: vec![field("event_kind", str_val(event_kind))],
        next_actions: vec![format!(
            "run `planeai-cli axi loop tick {short_id}` to continue"
        )],
    })
}

fn exec_human_wait(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
) -> Result<TickResult, String> {
    let _ = LoopService::update_loop_status(conn, loop_id, LoopStatus::NeedsHuman);
    let _ = LoopService::append_loop_event(
        conn,
        loop_id,
        "recipe_step_completed",
        &serde_json::json!({"step_id": step.id, "kind": step.kind}),
    );

    save_snapshot(conn, loop_id, snapshot)?;

    Ok(TickResult {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "needs_human".into(),
        extra: vec![],
        next_actions: vec!["human review required before the loop can proceed".to_string()],
    })
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn find_step<'a>(steps: &'a [RecipeStep], id: &str) -> Option<&'a RecipeStep> {
    steps.iter().find(|s| s.id == id)
}

/// Advance to the next step (explicit `next` field, or sequential).
fn advance_step(snapshot: &mut RecipeSnapshot, current: &RecipeStep) {
    if let Some(ref next) = current.next {
        snapshot.runtime.current_step = next.clone();
        return;
    }
    let idx = snapshot.steps.iter().position(|s| s.id == current.id);
    if let Some(i) = idx {
        if i + 1 < snapshot.steps.len() {
            snapshot.runtime.current_step = snapshot.steps[i + 1].id.clone();
        }
    }
}

/// Save the updated snapshot back to policy_json via LoopService.
fn save_snapshot(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &RecipeSnapshot,
) -> Result<(), String> {
    let json_val =
        serde_json::to_value(snapshot).map_err(|e| format!("failed to serialize snapshot: {e}"))?;
    LoopService::update_policy_json(conn, loop_id, &json_val)
        .map_err(|e| format!("failed to persist snapshot: {e}"))
}

/// Fallback handoff discovery from loop_events (for backward compat).
fn find_handoff_from_events(
    conn: &rusqlite::Connection,
    loop_id: &str,
    session_ids: &[String],
) -> Option<(String, String)> {
    let events = LoopService::list_loop_events(conn, loop_id).ok()?;
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
                    return Some((sid.to_string(), status.to_string()));
                }
            }
        }
    }
    None
}

/// Single-pass template rendering for recipe prompts.
///
/// Substitutes variables from a flat map, handles simple `{% if %}` conditionals
/// for absent keys, and renders knowledge files.
fn render_prompt(template: &str, snapshot: &RecipeSnapshot, loop_id: &str) -> String {
    // Build substitution map
    let mut vars: Vec<(&str, String)> = Vec::new();
    for (key, value) in &snapshot.inputs {
        vars.push((key.as_str(), value.clone()));
    }

    let mut result = template.to_string();

    // 1. Remove conditional blocks for ABSENT inputs (before substitution)
    let present_keys: std::collections::HashSet<&str> =
        snapshot.inputs.keys().map(|k| k.as_str()).collect();
    while let Some(start) = result.find("{% if inputs.") {
        let after = start + "{% if inputs.".len();
        let Some(key_end) = result[after..].find(" %}") else {
            break;
        };
        let key = result[after..after + key_end].to_string();
        let endif_tag = "{% endif %}";
        let Some(endif_offset) = result[start..].find(endif_tag) else {
            break;
        };

        if present_keys.contains(key.as_str()) {
            // Key is present: strip the if/endif delimiters, keep content
            let block_end = start + endif_offset + endif_tag.len();
            let if_close = start + "{% if inputs.".len() + key_end + " %}".len();
            let content = &result[if_close..start + endif_offset];
            let content = content.to_string();
            result = format!("{}{}{}", &result[..start], content, &result[block_end..]);
        } else {
            // Key is absent: remove the entire block
            let block_end = start + endif_offset + endif_tag.len();
            result = format!("{}{}", &result[..start], &result[block_end..]);
        }
    }

    // 2. Substitute input variables
    for (key, value) in &vars {
        let spaced = format!("{{{{ inputs.{} }}}}", key);
        let compact = format!("{{{{inputs.{}}}}}", key);
        result = result.replace(&spaced, value);
        result = result.replace(&compact, value);
    }

    // 3. Substitute built-in variables
    result = result.replace("{{ loop.id }}", loop_id);
    result = result.replace("{{loop.id}}", loop_id);
    result = result.replace("{{ recipe.id }}", &snapshot.recipe_id);
    result = result.replace("{{recipe.id}}", &snapshot.recipe_id);

    // 4. Knowledge files
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
    result = result.replace("{{knowledge.files}}", &knowledge_str);

    result
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let end = s
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= max)
        .last()
        .unwrap_or(0);
    format!("{}...", &s[..end])
}
