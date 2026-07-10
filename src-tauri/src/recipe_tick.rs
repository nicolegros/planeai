//! Recipe tick runtime — executes one recipe step per tick.
//!
//! Each step executor returns a [`TickResult`] describing what happened.
//! The single [`render_tick_result`] function converts it to TOON output,
//! eliminating repetitive field-construction boilerplate.

use planeai_core::loop_recipe::*;
use planeai_core::loop_recipe_service::RecipeSnapshot;
use planeai_core::loop_run::{LoopStatus, LoopTrigger};
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
    tracing::info!(
        loop_id = %short_id(loop_id),
        recipe_id = %snapshot.recipe_id,
        tick_count = snapshot.runtime.tick_count,
        current_step = %snapshot.runtime.current_step,
        round = snapshot.runtime.round,
        "tick_recipe: starting tick"
    );

    // Top-level terminal guard: do not execute any step if the loop is terminal.
    if let Ok(Some(run)) = LoopService::get_loop(conn, loop_id) {
        if run.status.is_executor_terminal() {
            tracing::warn!(loop_id = %short_id(loop_id), status = %run.status.as_str(), "tick_recipe: loop is terminal, cannot tick");
            return (
                render(&[field(
                    "error",
                    str_val(&format!(
                        "loop {} is in terminal status '{}' — cannot execute steps",
                        short_id(loop_id),
                        run.status.as_str()
                    )),
                )]),
                1,
            );
        }
        // Also guard intervention-required statuses (blocked, needs_human, stale)
        if run.status.is_intervention_required() {
            tracing::warn!(loop_id = %short_id(loop_id), status = %run.status.as_str(), "tick_recipe: loop requires intervention, cannot tick");
            return render_tick_result(
                loop_id,
                &snapshot.recipe_id,
                TickResult {
                    step_id: snapshot.runtime.current_step.clone(),
                    step_kind: "(guarded)".into(),
                    status: run.status.as_str().to_string(),
                    extra: vec![],
                    next_actions: vec![
                        "loop requires human intervention before it can proceed".into()
                    ],
                },
            );
        }
    }

    // Check max_ticks
    if snapshot.runtime.tick_count >= snapshot.policy.max_ticks {
        tracing::error!(loop_id = %short_id(loop_id), tick_count = snapshot.runtime.tick_count, max_ticks = snapshot.policy.max_ticks, "tick_recipe: max_ticks exceeded, failing loop");
        let _ = LoopService::transition_loop(conn, loop_id, LoopTrigger::MaxTicksExceeded);
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

    // Increment tick
    snapshot.runtime.tick_count += 1;

    tracing::info!(
        loop_id = %short_id(loop_id),
        step_id = %step.id,
        step_kind = %step.kind,
        tick_count = snapshot.runtime.tick_count,
        "tick_recipe: executing step"
    );

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
        STEP_ROUND_NEXT => exec_round_next(&mut ctx, &step),
        STEP_GATES_RUN => exec_gates_run(&mut ctx, &step),
        _ => return render_error("unsupported recipe step kind"),
    };

    match result {
        Ok(ref tr) => {
            tracing::info!(
                loop_id = %short_id(loop_id),
                step_id = %tr.step_id,
                step_kind = %tr.step_kind,
                status = %tr.status,
                "tick_recipe: step completed"
            );
            render_tick_result(loop_id, &ctx.snapshot.recipe_id, result.unwrap())
        }
        Err(ref msg) => {
            tracing::error!(
                loop_id = %short_id(loop_id),
                step_id = %step.id,
                step_kind = %step.kind,
                error = %msg,
                "tick_recipe: step failed"
            );
            render_error(msg)
        }
    }
}

// ─── Step Executors ──────────────────────────────────────────────────────────

fn exec_session_create(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    let role_id = step.role.as_deref().unwrap_or("default");

    // 1. Check max_sessions
    let existing_sessions =
        LoopService::list_loop_sessions(ctx.conn, ctx.loop_id).unwrap_or_default();
    if existing_sessions.len() as u32 >= ctx.snapshot.policy.max_sessions {
        LoopService::transition_loop(ctx.conn, ctx.loop_id, LoopTrigger::SessionLimitReached).ok();
        LoopService::append_loop_event(
            ctx.conn,
            ctx.loop_id,
            "recipe_runtime_limit_reached",
            &serde_json::json!({"step_id": step.id, "limit": "max_sessions"}),
        )
        .ok();
        save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;
        return Ok(TickResult {
            step_id: step.id.clone(),
            step_kind: step.kind.clone(),
            status: "needs_human".into(),
            extra: vec![field("limit", str_val("max_sessions"))],
            next_actions: vec!["max_sessions reached — cannot create more sessions".to_string()],
        });
    }

    // 2. Resolve role from recipe
    let role = ctx.snapshot.roles.get(role_id).cloned();
    let provider = role
        .as_ref()
        .map(|r| r.provider.clone())
        .unwrap_or_else(|| "default".to_string());
    let isolation = role
        .as_ref()
        .map(|r| r.isolation.clone())
        .unwrap_or_else(|| "worktree".to_string());

    // "default" means "use the user's configured default provider" — pass None
    // so build_session_plan falls through to env.config.default_provider.
    let provider_opt = if provider == "default" {
        None
    } else {
        Some(provider.clone())
    };

    // 3. Resolve project from loop
    let loop_run = LoopService::get_loop(ctx.conn, ctx.loop_id)
        .map_err(|e| format!("failed to load loop: {e}"))?
        .ok_or_else(|| "loop not found".to_string())?;

    let project = crate::db::get_project(ctx.conn, &loop_run.project_id)
        .map_err(|e| format!("failed to resolve project: {e}"))?
        .ok_or_else(|| format!("project not found: {}", loop_run.project_id))?;

    // 4. Build branch name for the session
    let round = ctx.snapshot.runtime.round;
    let branch_name = format!("loop/{}/{}-r{}", short_id(ctx.loop_id), role_id, round);

    // 5. Create the session via the standard path
    let use_worktree = isolation == "worktree";
    let base_branch = if round > 1 && use_worktree {
        // Worktree-isolated roles (maker) base on their own previous round
        Some(format!(
            "loop/{}/{}-r{}",
            short_id(ctx.loop_id),
            role_id,
            round - 1
        ))
    } else if !use_worktree {
        // Non-worktree roles (verifier/readonly) base on the maker's current branch
        // so they can review the maker's latest work
        let maker_branch = ctx
            .snapshot
            .runtime
            .created_session_ids
            .get("maker")
            .and_then(|ids| ids.last())
            .and_then(|sid| {
                crate::db::get_session(ctx.conn, sid)
                    .ok()
                    .flatten()
                    .map(|s| s.branch)
            });
        maker_branch
    } else {
        // Round 1 worktree role: use configured base_branch from inputs, or None (defaults to main)
        ctx.snapshot.inputs.get("base_branch").cloned()
    };

    // Render prompt before session creation so it's baked into the launch command
    let rendered_prompt = step
        .prompt
        .as_ref()
        .map(|tpl| render_prompt(tpl, ctx.snapshot, ctx.loop_id));
    let has_prompt = rendered_prompt.is_some();

    let opts = crate::cli::SessionCreateOpts {
        project: project.name.clone(),
        branch: branch_name.clone(),
        name: Some(format!("{} ({})", role_id, short_id(ctx.loop_id))),
        new_branch: true,
        worktree: use_worktree,
        base_branch,
        yolo: ctx.snapshot.policy.auto_approve,
        provider: provider_opt,
        task_key: loop_run.task_key.clone(),
        prompt: rendered_prompt,
        parent_session_id: loop_run.created_by_session_id.clone(),
    };

    let session = match crate::cli::create_session(ctx.conn, opts) {
        Ok(s) => {
            tracing::info!(
                loop_id = %short_id(ctx.loop_id),
                session_id = %s.id,
                role = %role_id,
                branch = %branch_name,
                provider = %provider,
                prompt_provided = has_prompt,
                "exec_session_create: session created successfully"
            );
            s
        }
        Err(e) => {
            tracing::error!(
                loop_id = %short_id(ctx.loop_id),
                role = %role_id,
                branch = %branch_name,
                error = %e,
                "exec_session_create: session creation failed"
            );
            // Rollback: on failure, append error event and return
            LoopService::append_loop_event(
                ctx.conn,
                ctx.loop_id,
                "recipe_step_failed",
                &serde_json::json!({
                    "step_id": step.id,
                    "kind": step.kind,
                    "error": e,
                }),
            )
            .ok();
            return Err(format!("session.create failed: {e}"));
        }
    };

    // 6. Link session to loop_sessions
    LoopService::add_loop_session(
        ctx.conn,
        planeai_core::loop_service::AddLoopSessionParams {
            loop_id: ctx.loop_id.to_string(),
            session_id: session.id.clone(),
            role: role_id.to_string(),
            round: round as i64,
            provider: Some(provider.clone()),
            status: "active".to_string(),
        },
    )
    .map_err(|e| {
        tracing::warn!(
            session_id = %session.id,
            loop_id = %ctx.loop_id,
            role = %role_id,
            "orphaned session: created but failed to link to loop; manual cleanup may be needed"
        );
        format!("failed to link session to loop: {e}")
    })?;

    // 7. Track session in runtime state
    ctx.snapshot
        .runtime
        .created_session_ids
        .entry(role_id.to_string())
        .or_default()
        .push(session.id.clone());

    // 8. (Prompt is now baked into the launch command via opts.prompt — no separate send needed)

    // 9. Keep loop status as Running — the subsequent handoff.wait step will
    //    park it at Observing if needed. This allows auto-tick to continue
    //    through to the wait step without stopping prematurely.

    // 10. Append events and advance
    LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        "recipe_step_completed",
        &serde_json::json!({
            "step_id": step.id,
            "kind": step.kind,
            "session_id": session.id,
            "role": role_id,
            "round": round,
        }),
    )
    .map_err(|e| format!("failed to append loop event: {e}"))?;

    advance_step(ctx.snapshot, step);
    save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

    Ok(TickResult {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "observing".into(),
        extra: vec![field(
            "created_session",
            Value::Object(vec![
                field("id", str_val(short_id(&session.id))),
                field("role", str_val(role_id)),
                field("provider", str_val(&provider)),
                field("round", str_val(&round.to_string())),
            ]),
        )],
        next_actions: vec![format!(
            "wait for {} handoff, then run `planeai-cli axi loop tick {}`",
            role_id,
            short_id(ctx.loop_id)
        )],
    })
}

fn exec_session_prompt(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    let role_id = step.role.as_deref().unwrap_or("default");

    // 1. Resolve session for the role
    let session_ids = ctx
        .snapshot
        .runtime
        .created_session_ids
        .get(role_id)
        .cloned()
        .unwrap_or_default();

    if session_ids.is_empty() {
        return Err(format!(
            "step '{}': no sessions exist for role '{role_id}' — \
             create a session first with session.create",
            step.id
        ));
    }

    // select: latest means last element (sessions are appended in creation order)
    let select = step.select.as_deref().unwrap_or("latest");
    let session_id = match select {
        "latest" => session_ids.last().unwrap().clone(),
        _ => {
            return Err(format!(
                "step '{}': unsupported select value '{}' — only 'latest' is supported",
                step.id, select
            ));
        }
    };

    // 2. Render prompt
    let prompt_template = step.prompt.as_deref().unwrap_or("");
    if prompt_template.is_empty() {
        return Err(format!(
            "step '{}': session.prompt requires a 'prompt' template",
            step.id
        ));
    }
    let rendered = render_prompt(prompt_template, ctx.snapshot, ctx.loop_id);

    // 3. Send prompt via real prompt path
    let ops = crate::session_ops::real_prompt_ops(planeai_paths::notify_socket_path());
    crate::session_ops::send_prompt(ctx.conn, &session_id, &rendered, &ops)
        .map_err(|e| format!("step '{}': prompt delivery failed: {e}", step.id))?;

    // 4. Append event and advance
    LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        "recipe_step_completed",
        &serde_json::json!({
            "step_id": step.id,
            "kind": step.kind,
            "session_id": session_id,
            "role": role_id,
            "round": ctx.snapshot.runtime.round,
        }),
    )
    .map_err(|e| format!("failed to append loop event: {e}"))?;

    advance_step(ctx.snapshot, step);
    save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

    Ok(TickResult {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "observing".into(),
        extra: vec![field(
            "prompted_session",
            Value::Object(vec![
                field("id", str_val(short_id(&session_id))),
                field("role", str_val(role_id)),
            ]),
        )],
        next_actions: vec![format!(
            "wait for {} to complete, then run `planeai-cli axi loop tick {}`",
            role_id,
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

    // Use LoopService to find handoff (strict: requires schema + valid status)
    // Only look for handoffs recorded after the last one we consumed (prevents re-consuming stale ones)
    let after_ts = ctx.snapshot.runtime.last_handoff_consumed_at.as_deref();
    let found_handoff =
        LoopService::find_handoff_for_sessions(ctx.conn, ctx.loop_id, &session_ids, after_ts)
            .map_err(|e| format!("handoff query failed: {e}"))?;

    match found_handoff {
        None => {
            LoopService::transition_loop(ctx.conn, ctx.loop_id, LoopTrigger::HandoffWaiting)
                .map_err(|e| format!("failed to transition loop: {e}"))?;
            LoopService::append_loop_event(
                ctx.conn,
                ctx.loop_id,
                "recipe_step_waiting",
                &serde_json::json!({"step_id": step.id, "waiting_for": "handoff", "role": role_id}),
            )
            .map_err(|e| format!("failed to append loop event: {e}"))?;
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
            // Handoff found — set loop back to Running so auto-tick continues
            LoopService::transition_loop(ctx.conn, ctx.loop_id, LoopTrigger::HandoffConsumed)
                .map_err(|e| format!("failed to transition loop: {e}"))?;

            // Record consumption timestamp so we don't re-consume this handoff in future rounds
            ctx.snapshot.runtime.last_handoff_consumed_at = Some(chrono::Utc::now().to_rfc3339());

            let next_step = step
                .on
                .as_ref()
                .and_then(|m| m.get(&handoff_status))
                .cloned();

            // If the handoff routes to a retry/rejection path, extract the
            // summary and store it in runtime.last_error so the next prompt
            // can inject structured feedback via {{ runtime.last_error }}.
            if handoff_status != "completed" {
                if let Ok(summary) = extract_handoff_summary(ctx.conn, ctx.loop_id, &session_id) {
                    ctx.snapshot.runtime.last_error = Some(summary);
                }
            }

            LoopService::append_loop_event(
                ctx.conn,
                ctx.loop_id,
                "recipe_step_completed",
                &serde_json::json!({
                    "step_id": step.id, "kind": step.kind,
                    "handoff_status": handoff_status,
                    "session_id": session_id, "next_step": next_step,
                }),
            )
            .map_err(|e| format!("failed to append loop event: {e}"))?;

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
        "verifying",
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

    LoopService::transition_loop(ctx.conn, ctx.loop_id, LoopTrigger::RecipeSetStatus(new_status.clone()))
        .map_err(|e| format!("failed to transition loop: {e}"))?;

    LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        "recipe_step_completed",
        &serde_json::json!({"step_id": step.id, "kind": step.kind, "status": status_str}),
    )
    .map_err(|e| format!("failed to append loop event: {e}"))?;

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

    LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        event_kind,
        &serde_json::json!({"step_id": step.id}),
    )
    .map_err(|e| format!("failed to append loop event: {e}"))?;

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
    LoopService::transition_loop(ctx.conn, ctx.loop_id, LoopTrigger::HumanWaitReached)
        .map_err(|e| format!("failed to transition loop: {e}"))?;
    LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        "recipe_step_completed",
        &serde_json::json!({"step_id": step.id, "kind": step.kind}),
    )
    .map_err(|e| format!("failed to append loop event: {e}"))?;

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

fn exec_round_next(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    // Enforce max_rounds: if we're already at the limit, set blocked
    if ctx.snapshot.runtime.round >= ctx.snapshot.policy.max_rounds {
        LoopService::transition_loop(ctx.conn, ctx.loop_id, LoopTrigger::RoundBlocked)
            .map_err(|e| format!("failed to transition loop: {e}"))?;
        LoopService::append_loop_event(
            ctx.conn,
            ctx.loop_id,
            "recipe_runtime_limit_reached",
            &serde_json::json!({
                "step_id": step.id,
                "limit": "max_rounds",
                "value": ctx.snapshot.policy.max_rounds,
                "current_round": ctx.snapshot.runtime.round,
            }),
        )
        .map_err(|e| format!("failed to append loop event: {e}"))?;
        save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

        return Ok(TickResult {
            step_id: step.id.clone(),
            step_kind: step.kind.clone(),
            status: "blocked".into(),
            extra: vec![
                field("limit", str_val("max_rounds")),
                field(
                    "value",
                    str_val(&ctx.snapshot.policy.max_rounds.to_string()),
                ),
            ],
            next_actions: vec![
                "max_rounds reached — inspect the loop and decide whether to continue manually"
                    .to_string(),
            ],
        });
    }

    // Increment round
    ctx.snapshot.runtime.round += 1;
    let new_round = ctx.snapshot.runtime.round;

    // Sync loop_runs.current_round
    ctx.conn
        .execute(
            "UPDATE loop_runs SET current_round = ?1 WHERE id = ?2",
            rusqlite::params![new_round as i64, ctx.loop_id],
        )
        .map_err(|e| format!("failed to update current_round: {e}"))?;

    LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        "recipe_round_started",
        &serde_json::json!({
            "step_id": step.id,
            "round": new_round,
        }),
    )
    .map_err(|e| format!("failed to append loop event: {e}"))?;

    advance_step(ctx.snapshot, step);
    save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

    Ok(TickResult {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "running".into(),
        extra: vec![field("round", str_val(&new_round.to_string()))],
        next_actions: vec![format!(
            "round {} started — run `planeai-cli axi loop tick {}` to continue",
            new_round,
            short_id(ctx.loop_id)
        )],
    })
}

fn exec_gates_run(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    use planeai_core::verifier::{VerifierLimits, VerifyGateRequest};

    if step.gates.is_empty() {
        return Err(format!(
            "step '{}': gates.run requires at least one gate declaration",
            step.id
        ));
    }

    LoopService::transition_loop(ctx.conn, ctx.loop_id, LoopTrigger::GatesStarted).ok();

    // Resolve a session to run gates in.
    // If step.role is specified, use that role's latest session; otherwise fall back to
    // the last session across all roles.
    let session_id = if let Some(ref role) = step.role {
        ctx.snapshot
            .runtime
            .created_session_ids
            .get(role)
            .and_then(|ids| ids.last())
            .cloned()
            .ok_or_else(|| format!("step '{}': no sessions found for role '{}'", step.id, role))?
    } else {
        ctx.snapshot
            .runtime
            .created_session_ids
            .values()
            .flatten()
            .last()
            .cloned()
            .ok_or_else(|| {
                format!(
                    "step '{}': no sessions available for gate execution",
                    step.id
                )
            })?
    };

    // Resolve project path for CWD
    let loop_run = LoopService::get_loop(ctx.conn, ctx.loop_id)
        .map_err(|e| format!("failed to load loop: {e}"))?
        .ok_or_else(|| "loop not found".to_string())?;

    let project = crate::db::get_project(ctx.conn, &loop_run.project_id)
        .map_err(|e| format!("failed to resolve project: {e}"))?
        .ok_or_else(|| format!("project not found: {}", loop_run.project_id))?;

    // Resolve session worktree path
    let session = crate::db::get_session(ctx.conn, &session_id)
        .map_err(|e| format!("failed to get session: {e}"))?
        .ok_or_else(|| format!("session not found: {session_id}"))?;

    // Run all gates — stop on first failure
    let mut overall_status = "pass";
    let mut failed_gate_name = String::new();
    let mut failed_gate_output: Option<String> = None;
    for gate in &step.gates {
        let rendered_command = render_prompt(&gate.command, ctx.snapshot, ctx.loop_id);
        let request = VerifyGateRequest {
            loop_id: ctx.loop_id.to_string(),
            session_id: session_id.clone(),
            name: gate.name.clone(),
            command: rendered_command,
            project_path: project.path.clone(),
            session_worktree_path: session.worktree_path.clone(),
            limits: VerifierLimits::default(),
        };

        match planeai_core::verifier::run_verifier_gate(ctx.conn, request) {
            Ok(result) => {
                let status_str = result.status.as_str();
                if status_str != "pass" {
                    overall_status = if status_str == "error" {
                        "error"
                    } else {
                        "fail"
                    };
                    failed_gate_name = gate.name.clone();
                    // Read the gate output so we can feed it to the retry prompt
                    if let Some(ref path) = result.output_path {
                        failed_gate_output = std::fs::read_to_string(path).ok();
                    }
                    break;
                }
            }
            Err(e) => {
                LoopService::append_loop_event(
                    ctx.conn,
                    ctx.loop_id,
                    "recipe_step_failed",
                    &serde_json::json!({
                        "step_id": step.id,
                        "gate": gate.name,
                        "error": e.to_string(),
                    }),
                )
                .ok();
                overall_status = "error";
                failed_gate_name = gate.name.clone();
                break;
            }
        }
    }

    // Map result through on.pass / on.fail / on.error
    let next_step = step
        .on
        .as_ref()
        .and_then(|m| m.get(overall_status))
        .cloned();

    let event_kind = if overall_status == "pass" {
        "recipe_step_completed"
    } else {
        // Store gate failure in last_error for the retry prompt template
        ctx.snapshot.runtime.last_error = Some(if let Some(ref output) = failed_gate_output {
            const MAX_GATE_OUTPUT: usize = 10_000;
            let truncated = if output.len() > MAX_GATE_OUTPUT {
                let suffix = "\n\n… [output truncated]";
                let safe_end = output[..MAX_GATE_OUTPUT]
                    .char_indices()
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                format!(
                    "Gate '{}' failed (exit status: {}).\n\nOutput:\n{}{}",
                    failed_gate_name,
                    overall_status,
                    &output[..safe_end],
                    suffix
                )
            } else {
                format!(
                    "Gate '{}' failed (exit status: {}).\n\nOutput:\n{}",
                    failed_gate_name, overall_status, output
                )
            };
            truncated
        } else {
            format!("Gate '{}' returned '{}'", failed_gate_name, overall_status)
        });
        "recipe_step_failed"
    };

    LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        event_kind,
        &serde_json::json!({
            "step_id": step.id,
            "kind": step.kind,
            "gates_result": overall_status,
            "next_step": next_step,
        }),
    )
    .map_err(|e| format!("failed to append loop event: {e}"))?;

    if let Some(ref ns) = next_step {
        ctx.snapshot.runtime.current_step = ns.clone();
    } else if overall_status != "pass" {
        tracing::warn!(
            step_id = %step.id,
            gates_result = %overall_status,
            "gates did not pass but step.on has no mapping for '{}'; setting loop to needs_human",
            overall_status,
        );
        LoopService::transition_loop(ctx.conn, ctx.loop_id, LoopTrigger::HumanWaitReached)
            .map_err(|e| format!("failed to transition loop: {e}"))?;
        save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

        return Ok(TickResult {
            step_id: step.id.clone(),
            step_kind: step.kind.clone(),
            status: "needs_human".into(),
            extra: vec![
                field("gates_result", str_val(overall_status)),
                field("reason", str_val("no on-mapping for gate outcome")),
            ],
            next_actions: vec![format!(
                "gates returned '{}' with no configured transition; manual intervention required",
                overall_status
            )],
        });
    } else {
        advance_step(ctx.snapshot, step);
    }
    save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

    let next_display = next_step.as_deref().unwrap_or("(end)");

    Ok(TickResult {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "verifying".into(),
        extra: vec![
            field("gates_result", str_val(overall_status)),
            field("next_step", str_val(next_display)),
        ],
        next_actions: vec![format!(
            "run `planeai-cli axi loop tick {}` to continue to '{}'",
            short_id(ctx.loop_id),
            next_display
        )],
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

/// Extract the summary field from the most recent handoff artifact for a session.
/// Used to populate `runtime.last_error` so retry prompts can include structured feedback.
fn extract_handoff_summary(
    conn: &rusqlite::Connection,
    loop_id: &str,
    session_id: &str,
) -> Result<String, String> {
    let content: Option<String> = conn
        .query_row(
            "SELECT content_json FROM loop_artifacts \
             WHERE loop_id = ?1 AND session_id = ?2 AND kind = 'handoff' \
             ORDER BY created_at DESC, id DESC LIMIT 1",
            rusqlite::params![loop_id, session_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("failed to query handoff: {e}"))?;

    let json_str = content.ok_or_else(|| "no content in handoff".to_string())?;
    let val: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| format!("invalid json: {e}"))?;

    let summary = val
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("(no summary provided)")
        .to_string();

    Ok(summary)
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

/// Safe template rendering for recipe prompts using minijinja.
///
/// Supported template variables:
/// - `{{ inputs.goal }}`, `{{ inputs.task_key }}`, etc. (recipe inputs)
/// - `{{ loop_run.id }}` — the loop run ID
/// - `{{ recipe.id }}` — the recipe identifier
/// - `{{ knowledge.files }}` — formatted knowledge file list
/// - `{{ runtime.round }}` — current round number
/// - `{{ runtime.last_error }}` — last error (if any)
///
/// No file inclusion, no shell execution, no arbitrary code.
#[allow(dead_code)] // Used by tests; production usage lands when session.create is wired.
fn render_prompt(template: &str, snapshot: &RecipeSnapshot, loop_id: &str) -> String {
    use minijinja::{context, Environment};

    let mut env = Environment::new();
    // Disable auto-escaping (templates produce plain text, not HTML)
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);

    if let Err(e) = env.add_template("prompt", template) {
        tracing::warn!(error = %e, "recipe template parse failed; using raw template");
        return template.to_string();
    }

    let tpl = match env.get_template("prompt") {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, "recipe template retrieval failed; using raw template");
            return template.to_string();
        }
    };

    // Build knowledge.files as a formatted string
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

    // Build the context
    let ctx = context! {
        inputs => &snapshot.inputs,
        loop_run => context! {
            id => loop_id,
        },
        recipe => context! {
            id => &snapshot.recipe_id,
        },
        knowledge => context! {
            files => &knowledge_str,
        },
        runtime => context! {
            round => snapshot.runtime.round,
            last_error => snapshot.runtime.last_error.as_deref().unwrap_or(""),
        },
    };

    match tpl.render(ctx) {
        Ok(rendered) => rendered,
        Err(e) => {
            tracing::warn!(error = %e, "recipe template render failed; using raw template");
            template.to_string()
        }
    }
}

#[allow(dead_code)] // Used by tests; production usage will return when session.prompt is wired.
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

// ─── Auto-advance helper ─────────────────────────────────────────────────────

/// Auto-advance a loop's recipe through immediately-executable steps.
///
/// Ticks the recipe up to `MAX_AUTO_TICKS` times, stopping early when:
/// - A tick returns a non-zero code (error).
/// - The loop reaches a terminal, intervention-required, or observing state.
/// - The current step is a `human.wait` (requires explicit user action).
///
/// When `check_human_wait_before_tick` is true, the human.wait check happens
/// before each tick (used by the handoff-complete path where the snapshot may
/// already be on a human.wait step). Otherwise it's checked after each tick.
///
/// Accepts an `Arc<Mutex<Connection>>` and re-acquires the lock for each tick,
/// releasing it between iterations so other commands can access the database.
pub fn auto_advance_with_arc(
    conn_arc: &std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    check_human_wait_before_tick: bool,
) {
    const MAX_AUTO_TICKS: usize = 10;

    for _ in 0..MAX_AUTO_TICKS {
        if check_human_wait_before_tick && is_human_wait_step(snapshot) {
            break;
        }

        if is_gates_step(snapshot) {
            break;
        }

        let conn = match conn_arc.lock() {
            Ok(c) => c,
            Err(_) => break,
        };

        let (_output, code) = tick_recipe(&conn, loop_id, snapshot);

        let updated_json = serde_json::to_value(&*snapshot).unwrap_or_default();
        let _ = LoopService::update_policy_json(&conn, loop_id, &updated_json);

        if code != 0 {
            break;
        }

        let should_stop = if let Ok(Some(r)) = LoopService::get_loop(&conn, loop_id) {
            r.status.is_executor_terminal()
                || r.status.is_intervention_required()
                || r.status == LoopStatus::Observing
        } else {
            false
        };

        drop(conn);

        if should_stop {
            break;
        }

        if !check_human_wait_before_tick && is_human_wait_step(snapshot) {
            break;
        }
    }
}

/// Simpler variant that takes a `&Connection` directly (used by AXI commands
/// that already manage their own connection lifetime).
pub fn auto_advance(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    check_human_wait_before_tick: bool,
) {
    const MAX_AUTO_TICKS: usize = 10;

    for _ in 0..MAX_AUTO_TICKS {
        if check_human_wait_before_tick && is_human_wait_step(snapshot) {
            break;
        }

        if is_gates_step(snapshot) {
            break;
        }

        let (_output, code) = tick_recipe(conn, loop_id, snapshot);

        let updated_json = serde_json::to_value(&*snapshot).unwrap_or_default();
        let _ = LoopService::update_policy_json(conn, loop_id, &updated_json);

        if code != 0 {
            break;
        }

        if let Ok(Some(r)) = LoopService::get_loop(conn, loop_id) {
            if r.status.is_executor_terminal()
                || r.status.is_intervention_required()
                || r.status == LoopStatus::Observing
            {
                break;
            }
        }

        if !check_human_wait_before_tick && is_human_wait_step(snapshot) {
            break;
        }
    }
}

fn is_human_wait_step(snapshot: &RecipeSnapshot) -> bool {
    let current = &snapshot.runtime.current_step;
    snapshot
        .steps
        .iter()
        .find(|s| &s.id == current)
        .map(|s| s.kind == "human.wait")
        .unwrap_or(false)
}

fn is_gates_step(snapshot: &RecipeSnapshot) -> bool {
    let current = &snapshot.runtime.current_step;
    snapshot
        .steps
        .iter()
        .find(|s| &s.id == current)
        .map(|s| s.kind == planeai_core::loop_recipe::STEP_GATES_RUN)
        .unwrap_or(false)
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
                last_error: None,
                last_handoff_consumed_at: None,
            },
            policy: SnapshotPolicy {
                max_rounds: 3,
                max_ticks: 50,
                max_sessions: 5,
                merge_policy: "human".into(),
                auto_approve: true,
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
            gates: vec![],
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
            "loop={{ loop_run.id }} recipe={{ recipe.id }}",
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
