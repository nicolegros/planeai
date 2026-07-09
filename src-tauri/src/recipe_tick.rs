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

// ─── TickContext ─────────────────────────────────────────────────────────────

/// Shared context passed to all step executors.
struct TickContext<'a> {
    conn: &'a rusqlite::Connection,
    loop_id: &'a str,
    snapshot: &'a mut RecipeSnapshot,
}

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
    let mut ctx = TickContext {
        conn,
        loop_id,
        snapshot,
    };

    let result = match step.kind.as_str() {
        STEP_SESSION_CREATE => exec_session_create(&mut ctx, &step),
        STEP_SESSION_PROMPT => exec_session_prompt(&mut ctx, &step),
        STEP_HANDOFF_WAIT => exec_handoff_wait(&mut ctx, &step),
        STEP_LOOP_STATUS => exec_loop_status(&mut ctx, &step),
        STEP_LOOP_EVENT => exec_loop_event(&mut ctx, &step),
        STEP_HUMAN_WAIT => exec_human_wait(&mut ctx, &step),
        _ => return render_error("unsupported recipe step kind"),
    };

    match result {
        Ok(tr) => render_tick_result(loop_id, &ctx.snapshot.recipe_id, tr),
        Err(msg) => render_error(&msg),
    }
}

// ─── Step Executors ──────────────────────────────────────────────────────────

fn exec_session_create(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    let role_id = step.role.as_deref().unwrap_or("default");

    let provider = ctx
        .snapshot
        .roles
        .get(role_id)
        .map(|r| r.provider.clone())
        .unwrap_or_else(|| "default".to_string());
    let isolation = ctx
        .snapshot
        .roles
        .get(role_id)
        .map(|r| r.isolation.clone())
        .unwrap_or_else(|| "worktree".to_string());

    let prompt = render_prompt(
        step.prompt.as_deref().unwrap_or(""),
        ctx.snapshot,
        ctx.loop_id,
    );
    let session_id = uuid::Uuid::new_v4().to_string();

    let params = planeai_core::loop_service::AddLoopSessionParams {
        loop_id: ctx.loop_id.to_string(),
        session_id: session_id.clone(),
        role: role_id.to_string(),
        round: ctx.snapshot.runtime.round as i64,
        provider: Some(provider.clone()),
        status: "active".to_string(),
    };
    LoopService::add_loop_session(ctx.conn, params)
        .map_err(|e| format!("failed to add loop session: {e}"))?;

    ctx.snapshot
        .runtime
        .created_session_ids
        .entry(role_id.to_string())
        .or_default()
        .push(session_id.clone());

    let _ = LoopService::update_loop_status(ctx.conn, ctx.loop_id, LoopStatus::Observing);
    let _ = LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        "recipe_step_completed",
        &serde_json::json!({
            "step_id": step.id, "kind": step.kind,
            "session_id": session_id, "role": role_id,
        }),
    );

    advance_step(ctx.snapshot, step);
    save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

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
                        short_id(&session_id).to_string(),
                        role_id.to_string(),
                        provider,
                        "active".to_string(),
                        ctx.snapshot.runtime.round.to_string(),
                        isolation,
                    ]],
                },
            ),
            field("prompt", str_val(&truncate(&prompt, 500))),
        ],
        next_actions: vec![format!(
            "wait for maker handoff, then run `planeai-cli axi loop tick {}`",
            short_id(ctx.loop_id)
        )],
    })
}

fn exec_session_prompt(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    let role_id = step.role.as_deref().unwrap_or("default");

    let session_id = ctx
        .snapshot
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

    let prompt = render_prompt(
        step.prompt.as_deref().unwrap_or(""),
        ctx.snapshot,
        ctx.loop_id,
    );

    let _ = LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        "recipe_step_completed",
        &serde_json::json!({
            "step_id": step.id, "kind": step.kind,
            "session_id": session_id, "role": role_id,
        }),
    );

    advance_step(ctx.snapshot, step);
    save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

    Ok(TickResult {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "observing".into(),
        extra: vec![
            field("session_id", str_val(short_id(&session_id))),
            field("role", str_val(role_id)),
            field("prompt", str_val(&truncate(&prompt, 500))),
        ],
        next_actions: vec![format!(
            "run `planeai-cli axi loop tick {}` to continue",
            short_id(ctx.loop_id)
        )],
    })
}

fn exec_handoff_wait(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    let role_id = step.from.as_deref().unwrap_or("default");

    let session_ids = ctx
        .snapshot
        .runtime
        .created_session_ids
        .get(role_id)
        .cloned()
        .unwrap_or_default();

    // Use LoopService to find handoff (primary: artifact query)
    let found_handoff = LoopService::find_handoff_for_sessions(ctx.conn, ctx.loop_id, &session_ids)
        .map_err(|e| format!("handoff query failed: {e}"))?;

    // Fallback: check loop_events for handoff_recorded
    let found_handoff = match found_handoff {
        Some(h) => Some(h),
        None => find_handoff_from_events(ctx.conn, ctx.loop_id, &session_ids),
    };

    match found_handoff {
        None => {
            let _ = LoopService::append_loop_event(
                ctx.conn,
                ctx.loop_id,
                "recipe_step_waiting",
                &serde_json::json!({"step_id": step.id, "waiting_for": "handoff", "role": role_id}),
            );
            save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

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
                ctx.conn,
                ctx.loop_id,
                "recipe_step_completed",
                &serde_json::json!({
                    "step_id": step.id, "kind": step.kind,
                    "handoff_status": handoff_status,
                    "session_id": session_id, "next_step": next_step,
                }),
            );

            if let Some(ref ns) = next_step {
                ctx.snapshot.runtime.current_step = ns.clone();
            } else {
                advance_step(ctx.snapshot, step);
            }
            save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

            let next_step_display = next_step.as_deref().unwrap_or("(end)");

            Ok(TickResult {
                step_id: step.id.clone(),
                step_kind: step.kind.clone(),
                status: "observing".into(),
                extra: vec![
                    field(
                        "matched_handoff",
                        Value::Object(vec![
                            field("session_id", str_val(short_id(&session_id))),
                            field("status", str_val(&handoff_status)),
                        ]),
                    ),
                    field("next_step", str_val(next_step_display)),
                ],
                next_actions: vec![format!(
                    "run `planeai-cli axi loop tick {}` to apply next step",
                    short_id(ctx.loop_id)
                )],
            })
        }
    }
}

fn exec_loop_status(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
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

    LoopService::update_loop_status(ctx.conn, ctx.loop_id, new_status.clone())
        .map_err(|e| format!("failed to update status: {e}"))?;

    let _ = LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        "recipe_step_completed",
        &serde_json::json!({"step_id": step.id, "kind": step.kind, "status": status_str}),
    );

    if !new_status.is_executor_terminal() && !new_status.is_intervention_required() {
        advance_step(ctx.snapshot, step);
    }
    save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

    let next_action = if new_status.is_executor_terminal() {
        "review the loop output before merging".to_string()
    } else if new_status.is_intervention_required() {
        "human intervention required".to_string()
    } else {
        format!(
            "run `planeai-cli axi loop tick {}` to continue",
            short_id(ctx.loop_id)
        )
    };

    Ok(TickResult {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: status_str.to_string(),
        extra: vec![field("state_changed", Value::Bool(true))],
        next_actions: vec![next_action],
    })
}

fn exec_loop_event(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    let event_kind = step.event_kind.as_deref().unwrap_or("recipe_event");

    let _ = LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        event_kind,
        &serde_json::json!({"step_id": step.id}),
    );

    advance_step(ctx.snapshot, step);
    save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

    Ok(TickResult {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "observing".into(),
        extra: vec![field("event_kind", str_val(event_kind))],
        next_actions: vec![format!(
            "run `planeai-cli axi loop tick {}` to continue",
            short_id(ctx.loop_id)
        )],
    })
}

fn exec_human_wait(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    let _ = LoopService::update_loop_status(ctx.conn, ctx.loop_id, LoopStatus::NeedsHuman);
    let _ = LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        "recipe_step_completed",
        &serde_json::json!({"step_id": step.id, "kind": step.kind}),
    );

    // Advance past human.wait so that when the human resumes, the next tick
    // progresses to the following step instead of re-executing this one.
    advance_step(ctx.snapshot, step);
    save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

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

/// Extract the first 8 characters of an ID for display.
fn short_id(id: &str) -> &str {
    &id[..std::cmp::min(8, id.len())]
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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use planeai_core::loop_recipe_service::{RecipeRuntime, RecipeSnapshot, SnapshotPolicy};
    use std::collections::BTreeMap;

    fn minimal_snapshot(
        steps: Vec<RecipeStep>,
        inputs: BTreeMap<String, String>,
    ) -> RecipeSnapshot {
        let first_step = steps.first().map(|s| s.id.clone()).unwrap_or_default();
        RecipeSnapshot {
            recipe_schema: "planeai.loop.recipe.v1".into(),
            recipe_id: "test-recipe".into(),
            recipe_source: "builtin".into(),
            recipe_path: None,
            inputs,
            runtime: RecipeRuntime {
                current_step: first_step,
                tick_count: 0,
                round: 1,
                created_session_ids: BTreeMap::new(),
            },
            policy: SnapshotPolicy {
                max_rounds: 3,
                max_ticks: 50,
                max_sessions: 5,
                merge_policy: "human".into(),
            },
            roles: BTreeMap::new(),
            steps,
            knowledge: RecipeKnowledge::default(),
            tools: RecipeTools::default(),
        }
    }

    fn make_step(id: &str, kind: &str) -> RecipeStep {
        RecipeStep {
            id: id.to_string(),
            kind: kind.to_string(),
            role: None,
            prompt: None,
            from: None,
            on: None,
            status: None,
            next: None,
            select: None,
            event_kind: None,
        }
    }

    // ─── render_prompt tests ─────────────────────────────────────────────────

    #[test]
    fn render_prompt_substitutes_input_variables() {
        let mut inputs = BTreeMap::new();
        inputs.insert("goal".to_string(), "fix the bug".to_string());
        let snapshot = minimal_snapshot(vec![], inputs);

        let result = render_prompt("Do this: {{ inputs.goal }}", &snapshot, "loop-123");
        assert_eq!(result, "Do this: fix the bug");
    }

    #[test]
    fn render_prompt_compact_syntax() {
        let mut inputs = BTreeMap::new();
        inputs.insert("goal".to_string(), "ship it".to_string());
        let snapshot = minimal_snapshot(vec![], inputs);

        let result = render_prompt("Do: {{inputs.goal}}", &snapshot, "loop-123");
        assert_eq!(result, "Do: ship it");
    }

    #[test]
    fn render_prompt_removes_absent_conditional_blocks() {
        let snapshot = minimal_snapshot(vec![], BTreeMap::new());
        let template = "before{% if inputs.missing %}HIDDEN{% endif %}after";
        let result = render_prompt(template, &snapshot, "loop-123");
        assert_eq!(result, "beforeafter");
    }

    #[test]
    fn render_prompt_keeps_present_conditional_blocks() {
        let mut inputs = BTreeMap::new();
        inputs.insert("task_key".to_string(), "PROJ-1".to_string());
        let snapshot = minimal_snapshot(vec![], inputs);

        let template = "{% if inputs.task_key %}key={{ inputs.task_key }}{% endif %}";
        let result = render_prompt(template, &snapshot, "loop-123");
        assert_eq!(result, "key=PROJ-1");
    }

    #[test]
    fn render_prompt_knowledge_files() {
        let mut snapshot = minimal_snapshot(vec![], BTreeMap::new());
        snapshot.knowledge.files = vec!["README.md".into(), "CONTEXT.md".into()];

        let result = render_prompt("Files: {{ knowledge.files }}", &snapshot, "loop-1");
        assert!(result.contains("- Read README.md"));
        assert!(result.contains("- Read CONTEXT.md"));
    }

    #[test]
    fn render_prompt_builtin_vars() {
        let snapshot = minimal_snapshot(vec![], BTreeMap::new());
        let result = render_prompt(
            "loop={{ loop.id }} recipe={{ recipe.id }}",
            &snapshot,
            "abc-123",
        );
        assert_eq!(result, "loop=abc-123 recipe=test-recipe");
    }

    // ─── advance_step tests ──────────────────────────────────────────────────

    #[test]
    fn advance_step_sequential() {
        let steps = vec![
            make_step("step1", "loop.event"),
            make_step("step2", "loop.event"),
            make_step("step3", "loop.event"),
        ];
        let mut snapshot = minimal_snapshot(steps.clone(), BTreeMap::new());
        assert_eq!(snapshot.runtime.current_step, "step1");

        advance_step(&mut snapshot, &steps[0]);
        assert_eq!(snapshot.runtime.current_step, "step2");

        advance_step(&mut snapshot, &steps[1]);
        assert_eq!(snapshot.runtime.current_step, "step3");
    }

    #[test]
    fn advance_step_explicit_next() {
        let mut step1 = make_step("step1", "loop.event");
        step1.next = Some("step3".to_string());
        let steps = vec![
            step1.clone(),
            make_step("step2", "loop.event"),
            make_step("step3", "loop.event"),
        ];
        let mut snapshot = minimal_snapshot(steps, BTreeMap::new());

        advance_step(&mut snapshot, &step1);
        assert_eq!(snapshot.runtime.current_step, "step3");
    }

    // ─── truncate tests ──────────────────────────────────────────────────────

    #[test]
    fn truncate_within_limit() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_over_limit() {
        let result = truncate("hello world", 5);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 9); // 5 chars + "..."
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate("", 10), "");
    }

    // ─── short_id tests ──────────────────────────────────────────────────────

    #[test]
    fn short_id_normal() {
        assert_eq!(short_id("abcdefghijklmnop"), "abcdefgh");
    }

    #[test]
    fn short_id_short_input() {
        assert_eq!(short_id("abc"), "abc");
    }

    #[test]
    fn short_id_exact_eight() {
        assert_eq!(short_id("12345678"), "12345678");
    }
}
