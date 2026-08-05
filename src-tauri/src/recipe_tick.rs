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
pub(crate) struct TickResult {
    pub(crate) step_id: String,
    pub(crate) step_kind: String,
    pub(crate) status: String,
    pub(crate) extra: Vec<Field>,
    pub(crate) next_actions: Vec<String>,
}

/// Render a TickResult into TOON output.
pub(crate) fn render_tick_result(
    loop_id: &str,
    recipe_id: &str,
    result: TickResult,
) -> (String, i32) {
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

            let next_actions =
                crate::stale_detection::intervention_next_actions(&run.status, short_id(loop_id));

            return render_tick_result(
                loop_id,
                &snapshot.recipe_id,
                TickResult {
                    step_id: snapshot.runtime.current_step.clone(),
                    step_kind: "(guarded)".into(),
                    status: run.status.as_str().to_string(),
                    extra: vec![],
                    next_actions,
                },
            );
        }
    }

    // ─── Seed last_activity_at on first tick ─────────────────────────────
    if snapshot.runtime.last_activity_at.is_none() {
        crate::stale_detection::refresh_activity(snapshot);
    }

    // ─── Session observation & heartbeat ─────────────────────────────────
    // Runs BEFORE stale check so that fresh activity is detected before
    // comparing elapsed time against the threshold.
    let observed = crate::stale_detection::observe_sessions(conn, loop_id, snapshot);
    if observed {
        if let Err(e) = save_snapshot(conn, loop_id, snapshot) {
            tracing::warn!(loop_id = %short_id(loop_id), error = %e, "tick_recipe: failed to persist observation state");
        }
    }

    // ─── Stale detection ─────────────────────────────────────────────────
    if let Some(output) = crate::stale_detection::check_stale(conn, loop_id, snapshot) {
        return output;
    }

    // Check max_ticks
    if snapshot.runtime.tick_count >= snapshot.policy.max_ticks {
        tracing::error!(loop_id = %short_id(loop_id), tick_count = snapshot.runtime.tick_count, max_ticks = snapshot.policy.max_ticks, "tick_recipe: max_ticks exceeded, failing loop");
        snapshot.runtime.status_override = Some(LoopStatus::Failed);
        let _ = save_snapshot(conn, loop_id, snapshot);
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
        STEP_CANDIDATES_CREATE => exec_candidates_create(&mut ctx, &step),
        STEP_CANDIDATES_WAIT => exec_candidates_wait(&mut ctx, &step),
        STEP_ARBITER_RANK => exec_arbiter_rank(&mut ctx, &step),
        _ => return render_error("unsupported recipe step kind"),
    };

    match result {
        Ok(tr) => {
            // Refresh activity if the step made meaningful progress
            // (current_step advanced = real work, not just polling)
            if ctx.snapshot.runtime.current_step != step.id {
                crate::stale_detection::refresh_activity(ctx.snapshot);
                // save_snapshot is already called inside each executor on advance,
                // but we need to persist the refreshed last_activity_at
                if let Err(e) = save_snapshot(conn, loop_id, ctx.snapshot) {
                    tracing::warn!(loop_id = %short_id(loop_id), error = %e, "tick_recipe: failed to persist activity refresh");
                }
            }
            tracing::info!(
                loop_id = %short_id(loop_id),
                step_id = %tr.step_id,
                step_kind = %tr.step_kind,
                status = %tr.status,
                "tick_recipe: step completed"
            );
            render_tick_result(loop_id, &ctx.snapshot.recipe_id, tr)
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

    // 0. Session reuse: if the role already has a session and session_reuse is
    //    enabled, re-prompt the existing session instead of spawning a new one.
    //    NOTE: The prompt-render-send-advance pattern here mirrors exec_session_prompt.
    //    A shared helper could eliminate the duplication — tracked for a follow-up refactor.
    let session_reuse = ctx
        .snapshot
        .roles
        .get(role_id)
        .map(|r| r.session_reuse)
        .unwrap_or(planeai_core::loop_recipe::default_session_reuse());

    // Only attempt reuse when a prompt is provided — without a prompt there's no
    // work to send, and reusing silently would leave the agent idle (F1 fix).
    let has_reuse_prompt = step.prompt.is_some();

    // Vec<String> is append-ordered — last() = most recently created session for this role.
    let existing_session_id = ctx
        .snapshot
        .runtime
        .created_session_ids
        .get(role_id)
        .and_then(|ids| ids.last())
        .cloned();

    if session_reuse && has_reuse_prompt {
        if let Some(session_id) = existing_session_id {
            // Verify the session is still active before reusing
            let session_active = crate::db::get_session(ctx.conn, &session_id)
                .ok()
                .flatten()
                .map(|s| s.status == "active")
                .unwrap_or(false);

            if session_active {
                // Render prompt and send to existing session.
                // On send_prompt failure (e.g., session died between check and send),
                // fall through to create a new session instead of hard-failing the loop (F2 fix).
                // Safety: has_reuse_prompt guarantees step.prompt is Some.
                let rendered_prompt = render_prompt(
                    step.prompt.as_deref().expect("guarded by has_reuse_prompt"),
                    ctx.snapshot,
                    ctx.loop_id,
                );

                let ops = crate::session_ops::real_prompt_ops(planeai_paths::notify_socket_path());
                match crate::session_ops::send_prompt(ctx.conn, &session_id, &rendered_prompt, &ops)
                {
                    Ok(_) => {
                        let round = ctx.snapshot.runtime.round;

                        tracing::info!(
                            loop_id = %short_id(ctx.loop_id),
                            session_id = %short_id(&session_id),
                            role = %role_id,
                            round = round,
                            "exec_session_create: reusing existing session"
                        );

                        LoopService::append_loop_event(
                            ctx.conn,
                            ctx.loop_id,
                            "recipe_step_completed",
                            &serde_json::json!({
                                "step_id": step.id,
                                "kind": step.kind,
                                "session_id": session_id,
                                "role": role_id,
                                "round": round,
                                "reused": true,
                            }),
                        )
                        .map_err(|e| format!("failed to append loop event: {e}"))?;

                        advance_step(ctx.snapshot, step);
                        save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

                        return Ok(TickResult {
                            step_id: step.id.clone(),
                            step_kind: step.kind.clone(),
                            status: "observing".into(),
                            extra: vec![field(
                                "reused_session",
                                Value::Object(vec![
                                    field("id", str_val(short_id(&session_id))),
                                    field("role", str_val(role_id)),
                                    field("round", str_val(&round.to_string())),
                                ]),
                            )],
                            next_actions: vec![format!(
                                "wait for {} handoff, then run `planeai-cli axi loop tick {}`",
                                role_id,
                                short_id(ctx.loop_id)
                            )],
                        });
                    }
                    Err(e) => {
                        // Send failed — session may have died between liveness check and send.
                        // Fall through to create a fresh session rather than failing the loop.
                        tracing::warn!(
                            loop_id = %short_id(ctx.loop_id),
                            session_id = %short_id(&session_id),
                            role = %role_id,
                            error = %e,
                            "exec_session_create: reuse send_prompt failed, falling through to create"
                        );
                    }
                }
            } else {
                // Session is not active — fall through to create a new one
                tracing::info!(
                    loop_id = %short_id(ctx.loop_id),
                    session_id = %short_id(&session_id),
                    role = %role_id,
                    "exec_session_create: existing session not active, creating new one"
                );
            }
        }
    }

    // 1. Check max_sessions
    let existing_sessions =
        LoopService::list_loop_sessions(ctx.conn, ctx.loop_id).unwrap_or_default();
    if existing_sessions.len() as u32 >= ctx.snapshot.policy.max_sessions {
        ctx.snapshot.runtime.status_override = Some(LoopStatus::NeedsHuman);
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
    let provider_raw = role
        .as_ref()
        .map(|r| r.provider.clone())
        .unwrap_or_else(|| "default".to_string());
    // Render provider through template engine so {{ inputs.x }} works
    let provider = if provider_raw.contains("{{") {
        render_prompt(&provider_raw, ctx.snapshot, ctx.loop_id)
    } else {
        provider_raw
    };
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
    let (loop_run, project) = resolve_loop_project(ctx)?;

    // 4. Determine worktree usage based on isolation
    let round = ctx.snapshot.runtime.round;
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
        ctx.snapshot
            .inputs
            .get("base_branch")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };

    // Render prompt before session creation so it's baked into the launch command
    let rendered_prompt = step
        .prompt
        .as_ref()
        .map(|tpl| render_prompt(tpl, ctx.snapshot, ctx.loop_id));
    let has_prompt = rendered_prompt.is_some();

    // Resolve branch: use step.branch (rendered) if present, otherwise generated name
    let (branch_name, new_branch) = resolve_branch_for_step(step, ctx.snapshot, ctx.loop_id);

    let opts = crate::cli::SessionCreateOpts {
        project: project.name.clone(),
        branch: branch_name.clone(),
        name: Some(format!("{} ({})", role_id, short_id(ctx.loop_id))),
        new_branch,
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
            // Step pointer stays at handoff.wait → status derived as Observing
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
            // Handoff found — step advances, status derived from next step kind

            // Record consumption timestamp so we don't re-consume this handoff in future rounds
            ctx.snapshot.runtime.last_handoff_consumed_at = Some(chrono::Utc::now().to_rfc3339());
            // Clear any status_override since the loop is resuming
            ctx.snapshot.runtime.status_override = None;

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

/// Execute a `loop.status` step — declares the loop's status.
///
/// The step pointer stays at this step (never advances). Status is derived from
/// `step.status` by `persist_snapshot`. This makes `loop.status` a **parking
/// spot**: the loop remains here until an external event (handoff, human resume,
/// or recipe routing via `step.on`) moves the pointer elsewhere.
///
/// - Terminal/intervention statuses halt ticking via the tick guard.
/// - Non-terminal statuses (observing, verifying) halt `auto_advance` because
///   the step pointer doesn't change between ticks.
fn exec_loop_status(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    let status_str = step.status.as_deref().unwrap_or("observing");

    const ALLOWED: &[&str] = &[
        "observing",
        "verifying",
        "completed_unreviewed",
        "approved",
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

    LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        "recipe_step_completed",
        &serde_json::json!({"step_id": step.id, "kind": step.kind, "status": status_str}),
    )
    .map_err(|e| format!("failed to append loop event: {e}"))?;

    // Never advance: step pointer stays at loop.status, derivation reads
    // step.status to produce the declared LoopStatus. Terminal/intervention
    // statuses halt ticking via the guard; non-terminal statuses (observing,
    // verifying) halt auto-advance because the step doesn't advance.
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
    ctx.snapshot.runtime.status_override = Some(LoopStatus::NeedsHuman);
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
        ctx.snapshot.runtime.status_override = Some(LoopStatus::Blocked);
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

    // Increment round — clear per-round state so it doesn't carry over.
    ctx.snapshot.runtime.candidate_handoffs.clear();
    ctx.snapshot.runtime.round += 1;
    let new_round = ctx.snapshot.runtime.round;

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

fn exec_gates_run_body(
    ctx: &mut TickContext,
    step: &RecipeStep,
) -> Result<(&'static str, Option<String>), String> {
    use planeai_core::verifier::{VerifierLimits, VerifyGateRequest};

    let session_ids: Vec<String> = if let Some(ref role) = step.role {
        ctx.snapshot
            .runtime
            .created_session_ids
            .get(role)
            .cloned()
            .unwrap_or_default()
    } else {
        ctx.snapshot
            .runtime
            .created_session_ids
            .values()
            .flatten()
            .cloned()
            .collect()
    };

    if session_ids.is_empty() {
        let role_display = step.role.as_deref().unwrap_or("(any)");
        return Err(format!(
            "step '{}': no sessions found for role '{}'",
            step.id, role_display
        ));
    }

    let (_loop_run, project) = resolve_loop_project(ctx)?;

    // Run gates against ALL sessions for the role (supports n-candidates).
    let mut overall_status: &str = "pass";
    let mut failed_gate_name = String::new();
    let mut failed_gate_output: Option<String> = None;
    let mut failed_gate_output_path: Option<String> = None;

    for session_id in &session_ids {
        let session = match crate::db::get_session(ctx.conn, session_id) {
            Ok(Some(s)) => s,
            Ok(None) => {
                tracing::warn!(session_id = %short_id(session_id), "gates: session not found, skipping");
                continue;
            }
            Err(e) => {
                tracing::warn!(session_id = %short_id(session_id), error = %e, "gates: failed to load session, skipping");
                continue;
            }
        };

        for gate in &step.gates {
            let rendered_command = render_prompt(&gate.command, ctx.snapshot, ctx.loop_id);

            // Defense-in-depth: reject gate commands with shell metacharacters that
            // could indicate injection (backticks, $(), eval). Gate commands are
            // authored by humans at loop creation time, not agents.
            const FORBIDDEN_PATTERNS: &[&str] = &["`", "$(", "${", "eval "];
            for pattern in FORBIDDEN_PATTERNS {
                if rendered_command.contains(pattern) {
                    return Err(format!(
                        "step '{}': gate '{}' command contains forbidden pattern '{}' — \
                         gate commands must not use command substitution or eval",
                        step.id, gate.name, pattern
                    ));
                }
            }

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
                        if let Some(ref path) = result.output_path {
                            failed_gate_output = std::fs::read_to_string(path).ok();
                            failed_gate_output_path = result.output_path.clone();
                        }
                        // For multi-candidate: record which session failed but continue
                        // checking others. The first failure determines the overall status.
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
                            "session_id": session_id,
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
        // Stop at first session that fails (fail-fast for routing).
        if overall_status != "pass" {
            break;
        }
    }

    let next_step = step
        .on
        .as_ref()
        .and_then(|m| m.get(overall_status))
        .cloned();

    let event_kind = if overall_status == "pass" {
        "recipe_step_completed"
    } else {
        ctx.snapshot.runtime.last_error = Some(if let Some(ref output) = failed_gate_output {
            let path_note = if let Some(ref path) = failed_gate_output_path {
                format!("\n\nFull output log: {path}\nRead this file for complete details.")
            } else {
                String::new()
            };
            const MAX_GATE_OUTPUT: usize = 100_000;
            let display_output = if output.len() > MAX_GATE_OUTPUT {
                let safe_end = output[..MAX_GATE_OUTPUT]
                    .char_indices()
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                format!("{}\n\n… [output truncated]", &output[..safe_end])
            } else {
                output.clone()
            };
            format!(
                "Gate '{}' failed (exit status: {}).\n\nOutput:\n{}{}",
                failed_gate_name, overall_status, display_output, path_note
            )
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

    Ok((overall_status, next_step))
}

fn exec_gates_run(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    if step.gates.is_empty() {
        return Err(format!(
            "step '{}': gates.run requires at least one gate declaration",
            step.id
        ));
    }

    // Status is derived as Verifying while step pointer is at gates.run
    let gates_body_result = exec_gates_run_body(ctx, step);

    let (overall_status, next_step) = gates_body_result?;

    if let Some(ref ns) = next_step {
        ctx.snapshot.runtime.current_step = ns.clone();
    } else if overall_status != "pass" {
        tracing::warn!(
            step_id = %step.id,
            gates_result = %overall_status,
            "gates did not pass but step.on has no mapping for '{}'; setting loop to needs_human",
            overall_status,
        );
        ctx.snapshot.runtime.status_override = Some(LoopStatus::NeedsHuman);
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

// ─── N-Candidates + Arbiter Step Executors ───────────────────────────────────

fn exec_candidates_create(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    let role_id = step.role.as_deref().unwrap_or("default");

    // 1. Render providers template and split by comma
    let providers_tpl = step
        .providers
        .as_deref()
        .ok_or_else(|| format!("step '{}': candidates.create requires 'providers'", step.id))?;
    let rendered_providers = render_prompt(providers_tpl, ctx.snapshot, ctx.loop_id);
    let providers: Vec<&str> = rendered_providers
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if providers.is_empty() {
        return Err(format!(
            "step '{}': providers list is empty after rendering",
            step.id
        ));
    }

    // Validate provider names: only alphanumeric, dashes, underscores, and dots allowed.
    // This prevents branch-name injection via crafted provider strings (SECURITY 1).
    for p in &providers {
        if !p
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(format!(
                "step '{}': invalid provider name '{}' — only alphanumeric, dashes, underscores, and dots are allowed",
                step.id, p
            ));
        }
    }

    // Deduplicate providers to prevent branch collisions (ROBUSTNESS 1).
    let providers: Vec<&str> = {
        let mut seen = std::collections::HashSet::new();
        providers.into_iter().filter(|p| seen.insert(*p)).collect()
    };

    tracing::info!(
        loop_id = %short_id(ctx.loop_id),
        step_id = %step.id,
        count = providers.len(),
        "exec_candidates_create: creating {} candidate sessions",
        providers.len()
    );
    tracing::debug!(
        loop_id = %short_id(ctx.loop_id),
        providers = ?providers,
        "exec_candidates_create: provider list"
    );

    // 2. Check max_sessions
    let existing_sessions =
        LoopService::list_loop_sessions(ctx.conn, ctx.loop_id).unwrap_or_default();
    let remaining_capacity = ctx
        .snapshot
        .policy
        .max_sessions
        .saturating_sub(existing_sessions.len() as u32);

    if (providers.len() as u32) > remaining_capacity {
        ctx.snapshot.runtime.status_override = Some(LoopStatus::NeedsHuman);
        LoopService::append_loop_event(
            ctx.conn,
            ctx.loop_id,
            "recipe_runtime_limit_reached",
            &serde_json::json!({
                "step_id": step.id,
                "limit": "max_sessions",
                "requested": providers.len(),
                "remaining_capacity": remaining_capacity,
            }),
        )
        .ok();
        save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;
        return Ok(TickResult {
            step_id: step.id.clone(),
            step_kind: step.kind.clone(),
            status: "needs_human".into(),
            extra: vec![field("limit", str_val("max_sessions"))],
            next_actions: vec![format!(
                "need {} sessions but only {} capacity remaining",
                providers.len(),
                remaining_capacity
            )],
        });
    }

    // 3. Resolve loop and project
    let (loop_run, project) = resolve_loop_project(ctx)?;

    let round = ctx.snapshot.runtime.round;

    // Render prompt (shared across all candidates)
    let rendered_prompt = step
        .prompt
        .as_ref()
        .map(|tpl| render_prompt(tpl, ctx.snapshot, ctx.loop_id));

    // Base branch for candidates
    let base_branch = ctx
        .snapshot
        .inputs
        .get("base_branch")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 4. Create a session for each provider.
    // Idempotency guard: on retry (partial failure), skip providers that already have
    // sessions tracked from a previous attempt (CORRECTNESS 2, ROBUSTNESS 1).
    let existing_providers: std::collections::HashSet<String> =
        LoopService::list_loop_sessions(ctx.conn, ctx.loop_id)
            .unwrap_or_default()
            .iter()
            .filter(|ls| ls.role == role_id && ls.round == round as i64)
            .filter_map(|ls| ls.provider.clone())
            .collect();

    let mut created_sessions = Vec::new();
    for provider_name in &providers {
        // Skip providers that already succeeded in a previous attempt
        if existing_providers.contains(*provider_name) {
            continue;
        }

        let branch_name = format!(
            "loop/{}/{}-{}-r{}",
            short_id(ctx.loop_id),
            role_id,
            provider_name,
            round
        );

        // "default" means "use the user's configured default provider"
        let provider_opt = if *provider_name == "default" {
            None
        } else {
            Some(provider_name.to_string())
        };

        let opts = crate::cli::SessionCreateOpts {
            project: project.name.clone(),
            branch: branch_name.clone(),
            name: Some(format!(
                "{}-{} ({})",
                role_id,
                provider_name,
                short_id(ctx.loop_id)
            )),
            new_branch: true,
            worktree: true,
            base_branch: base_branch.clone(),
            yolo: ctx.snapshot.policy.auto_approve,
            provider: provider_opt,
            task_key: loop_run.task_key.clone(),
            prompt: rendered_prompt.clone(),
            parent_session_id: loop_run.created_by_session_id.clone(),
        };

        let session = match crate::cli::create_session(ctx.conn, opts) {
            Ok(s) => {
                tracing::info!(
                    loop_id = %short_id(ctx.loop_id),
                    session_id = %s.id,
                    role = %role_id,
                    provider = %provider_name,
                    branch = %branch_name,
                    "exec_candidates_create: candidate session created"
                );
                s
            }
            Err(e) => {
                tracing::error!(
                    loop_id = %short_id(ctx.loop_id),
                    role = %role_id,
                    provider = %provider_name,
                    error = %e,
                    "exec_candidates_create: session creation failed"
                );
                LoopService::append_loop_event(
                    ctx.conn,
                    ctx.loop_id,
                    "recipe_step_failed",
                    &serde_json::json!({
                        "step_id": step.id,
                        "kind": step.kind,
                        "provider": provider_name,
                        "error": e,
                    }),
                )
                .ok();
                return Err(format!(
                    "candidates.create failed for provider '{}': {e}",
                    provider_name
                ));
            }
        };

        // Link session to loop
        let provider_str = provider_name.to_string();

        LoopService::add_loop_session(
            ctx.conn,
            planeai_core::loop_service::AddLoopSessionParams {
                loop_id: ctx.loop_id.to_string(),
                session_id: session.id.clone(),
                role: role_id.to_string(),
                round: round as i64,
                provider: Some(provider_str),
                status: "active".to_string(),
            },
        )
        .map_err(|e| format!("failed to link session to loop: {e}"))?;

        // Track in runtime
        ctx.snapshot
            .runtime
            .created_session_ids
            .entry(role_id.to_string())
            .or_default()
            .push(session.id.clone());

        // Persist snapshot after each successful session so partial failures
        // don't leave orphaned sessions untracked (CORRECTNESS 2).
        save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

        created_sessions.push((session.id, provider_name.to_string()));
    }

    // 5. Append event and advance
    LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        "recipe_step_completed",
        &serde_json::json!({
            "step_id": step.id,
            "kind": step.kind,
            "candidates_created": created_sessions.len(),
            "providers": providers,
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
            "candidates_created",
            Value::Object(vec![
                field("count", str_val(&created_sessions.len().to_string())),
                field("role", str_val(role_id)),
                field(
                    "providers",
                    Value::List(providers.iter().map(|p| p.to_string()).collect()),
                ),
            ]),
        )],
        next_actions: vec![format!(
            "wait for all {} candidates to hand off, then run `planeai-cli axi loop tick {}`",
            created_sessions.len(),
            short_id(ctx.loop_id)
        )],
    })
}

fn exec_candidates_wait(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    let role_id = step.from.as_deref().unwrap_or("default");
    let round = ctx.snapshot.runtime.round;

    // Only consider sessions from the current round (created_session_ids accumulates
    // across rounds, but candidate_handoffs is cleared per round).
    let all_session_ids = ctx
        .snapshot
        .runtime
        .created_session_ids
        .get(role_id)
        .cloned()
        .unwrap_or_default();

    let current_round_sessions: std::collections::HashSet<String> =
        LoopService::list_loop_sessions(ctx.conn, ctx.loop_id)
            .unwrap_or_default()
            .iter()
            .filter(|ls| ls.role == role_id && ls.round == round as i64)
            .map(|ls| ls.session_id.clone())
            .collect();

    let session_ids: Vec<String> = all_session_ids
        .into_iter()
        .filter(|sid| current_round_sessions.contains(sid))
        .collect();

    if session_ids.is_empty() {
        return Err(format!(
            "step '{}': no sessions exist for role '{role_id}'",
            step.id
        ));
    }

    let total_candidates = session_ids.len();
    let after_ts = ctx.snapshot.runtime.last_handoff_consumed_at.as_deref();

    // Check each candidate session for handoffs
    let mut new_handoffs_found = false;
    for session_id in &session_ids {
        // Skip if we already tracked this candidate's handoff
        if ctx
            .snapshot
            .runtime
            .candidate_handoffs
            .contains_key(session_id)
        {
            continue;
        }

        // Check for handoff from this specific session
        let found = match LoopService::find_handoff_for_sessions(
            ctx.conn,
            ctx.loop_id,
            std::slice::from_ref(session_id),
            after_ts,
        ) {
            Ok(f) => f,
            Err(e) => {
                // Single-session query failure should not abort the entire step (ROBUSTNESS 3).
                tracing::warn!(
                    loop_id = %short_id(ctx.loop_id),
                    session_id = %short_id(session_id),
                    error = %e,
                    "exec_candidates_wait: handoff query failed for session, skipping"
                );
                continue;
            }
        };

        if let Some((_sid, handoff_status)) = found {
            tracing::info!(
                loop_id = %short_id(ctx.loop_id),
                session_id = %short_id(session_id),
                handoff_status = %handoff_status,
                "exec_candidates_wait: candidate handoff received"
            );
            ctx.snapshot
                .runtime
                .candidate_handoffs
                .insert(session_id.clone(), handoff_status);
            new_handoffs_found = true;
        }
    }

    // Track persistent query failures: if no new handoffs were discovered and
    // all unresolved queries failed/returned nothing, increment the failure counter.
    // Escalate to needs_human after 5 consecutive failed ticks.
    if new_handoffs_found {
        ctx.snapshot.runtime.candidates_query_failures = 0;
    } else {
        ctx.snapshot.runtime.candidates_query_failures += 1;
        const MAX_QUERY_FAILURES: u32 = 5;
        if ctx.snapshot.runtime.candidates_query_failures >= MAX_QUERY_FAILURES {
            tracing::error!(
                loop_id = %short_id(ctx.loop_id),
                consecutive_failures = ctx.snapshot.runtime.candidates_query_failures,
                "exec_candidates_wait: persistent handoff query failures, escalating to needs_human"
            );
            ctx.snapshot.runtime.status_override = Some(LoopStatus::NeedsHuman);
            LoopService::append_loop_event(
                ctx.conn,
                ctx.loop_id,
                "recipe_step_failed",
                &serde_json::json!({
                    "step_id": step.id,
                    "kind": step.kind,
                    "reason": "persistent_query_failures",
                    "consecutive_failures": ctx.snapshot.runtime.candidates_query_failures,
                }),
            )
            .ok();
            save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;
            return Ok(TickResult {
                step_id: step.id.clone(),
                step_kind: step.kind.clone(),
                status: "needs_human".into(),
                extra: vec![field(
                    "reason",
                    str_val("persistent handoff query failures"),
                )],
                next_actions: vec![
                    "handoff queries have failed repeatedly — check database health".to_string(),
                ],
            });
        }
    }

    let completed_count = ctx.snapshot.runtime.candidate_handoffs.len();

    // Check if all candidates have handed off
    if completed_count >= total_candidates {
        tracing::info!(
            loop_id = %short_id(ctx.loop_id),
            total = total_candidates,
            completed = completed_count,
            "exec_candidates_wait: all candidates have handed off"
        );

        // Record consumption timestamp
        ctx.snapshot.runtime.last_handoff_consumed_at = Some(chrono::Utc::now().to_rfc3339());
        ctx.snapshot.runtime.status_override = None;

        // Route via step.on (key="all_complete") if available
        let next_step = step
            .on
            .as_ref()
            .and_then(|m| m.get("all_complete"))
            .cloned();

        LoopService::append_loop_event(
            ctx.conn,
            ctx.loop_id,
            "recipe_step_completed",
            &serde_json::json!({
                "step_id": step.id,
                "kind": step.kind,
                "total_candidates": total_candidates,
                "completed_candidates": completed_count,
                "candidate_handoffs": &ctx.snapshot.runtime.candidate_handoffs,
                "next_step": next_step,
            }),
        )
        .map_err(|e| format!("failed to append loop event: {e}"))?;

        if let Some(ref ns) = next_step {
            ctx.snapshot.runtime.current_step = ns.clone();
        } else {
            advance_step(ctx.snapshot, step);
        }
        save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;

        let next_display = next_step.as_deref().unwrap_or("(next)");
        return Ok(TickResult {
            step_id: step.id.clone(),
            step_kind: step.kind.clone(),
            status: "observing".into(),
            extra: vec![
                field(
                    "candidates_complete",
                    Value::Object(vec![
                        field("total", str_val(&total_candidates.to_string())),
                        field("completed", str_val(&completed_count.to_string())),
                    ]),
                ),
                field("next_step", str_val(next_display)),
            ],
            next_actions: vec![format!(
                "all candidates complete — run `planeai-cli axi loop tick {}` to continue to '{}'",
                short_id(ctx.loop_id),
                next_display
            )],
        });
    }

    // Not all candidates have handed off yet — stay at this step
    if new_handoffs_found {
        save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;
    }

    LoopService::append_loop_event(
        ctx.conn,
        ctx.loop_id,
        "recipe_step_waiting",
        &serde_json::json!({
            "step_id": step.id,
            "waiting_for": "candidate_handoffs",
            "role": role_id,
            "total": total_candidates,
            "completed": completed_count,
        }),
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
                field("kind", str_val("candidate_handoffs")),
                field("role", str_val(role_id)),
                field("total", str_val(&total_candidates.to_string())),
                field("completed", str_val(&completed_count.to_string())),
            ]),
        )],
        next_actions: vec![format!(
            "{}/{} candidates have handed off — waiting for remaining",
            completed_count, total_candidates
        )],
    })
}

fn exec_arbiter_rank(ctx: &mut TickContext, step: &RecipeStep) -> Result<TickResult, String> {
    let role_id = step.role.as_deref().unwrap_or("arbiter");

    // 1. Check max_sessions
    let existing_sessions =
        LoopService::list_loop_sessions(ctx.conn, ctx.loop_id).unwrap_or_default();
    if existing_sessions.len() as u32 >= ctx.snapshot.policy.max_sessions {
        ctx.snapshot.runtime.status_override = Some(LoopStatus::NeedsHuman);
        LoopService::append_loop_event(
            ctx.conn,
            ctx.loop_id,
            "recipe_runtime_limit_reached",
            &serde_json::json!({
                "step_id": step.id,
                "limit": "max_sessions",
                "current": existing_sessions.len(),
                "max": ctx.snapshot.policy.max_sessions,
            }),
        )
        .ok();
        save_snapshot(ctx.conn, ctx.loop_id, ctx.snapshot)?;
        return Ok(TickResult {
            step_id: step.id.clone(),
            step_kind: step.kind.clone(),
            status: "needs_human".into(),
            extra: vec![field("limit", str_val("max_sessions"))],
            next_actions: vec!["max_sessions reached — cannot create arbiter session".to_string()],
        });
    }

    // 2. Resolve role from recipe
    let role = ctx.snapshot.roles.get(role_id).cloned();
    let provider_raw = role
        .as_ref()
        .map(|r| r.provider.clone())
        .unwrap_or_else(|| "default".to_string());
    // Render provider through template engine so {{ inputs.x }} works
    let provider = if provider_raw.contains("{{") {
        render_prompt(&provider_raw, ctx.snapshot, ctx.loop_id)
    } else {
        provider_raw
    };
    let isolation = role
        .as_ref()
        .map(|r| r.isolation.clone())
        .unwrap_or_else(|| "readonly".to_string());

    let provider_opt = if provider == "default" {
        None
    } else {
        Some(provider.clone())
    };

    // 3. Resolve project from loop
    let (loop_run, project) = resolve_loop_project(ctx)?;

    let round = ctx.snapshot.runtime.round;

    // 4. Build candidates context for the arbiter prompt
    // The candidate role is derived from step.from (e.g., "maker"), defaulting to "maker".
    let candidate_role = step.from.as_deref().unwrap_or("maker");
    let candidates_context = build_candidates_context(ctx, candidate_role);

    // 5. Render the prompt with candidates context
    let rendered_prompt = step.prompt.as_ref().map(|tpl| {
        render_prompt_with_candidates(tpl, ctx.snapshot, ctx.loop_id, &candidates_context)
    });

    // 6. Create arbiter session
    let use_worktree = isolation == "worktree";
    let branch_name = format!("loop/{}/{}-r{}", short_id(ctx.loop_id), role_id, round);

    // For readonly/review roles, base on first maker's branch so they can see the code.
    // Always create a new branch (new_branch=true) — for non-worktree isolation the session
    // still needs its own branch created from the maker's branch point.
    let base_branch = if !use_worktree {
        ctx.snapshot
            .runtime
            .created_session_ids
            .get("maker")
            .and_then(|ids| ids.first())
            .and_then(|sid| {
                crate::db::get_session(ctx.conn, sid)
                    .ok()
                    .flatten()
                    .map(|s| s.branch)
            })
    } else {
        None
    };

    // Override provider with inputs.arbiter_provider if specified (CORRECTNESS 3).
    // "default" means "use the user's configured default provider" → pass None.
    let provider_opt = match ctx
        .snapshot
        .inputs
        .get("arbiter_provider")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        Some("default") => None,
        Some(ap) => {
            // Validate arbiter provider name (same rules as candidate providers).
            if !ap
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
            {
                return Err(format!(
                    "step '{}': invalid arbiter_provider '{}' — only alphanumeric, dashes, underscores, and dots are allowed",
                    step.id, ap
                ));
            }
            Some(ap.to_string())
        }
        None => provider_opt,
    };

    // Resolve the effective provider name for logging/tracking
    let effective_provider = provider_opt.as_deref().unwrap_or("default").to_string();

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
                provider = %effective_provider,
                "exec_arbiter_rank: arbiter session created"
            );
            s
        }
        Err(e) => {
            tracing::error!(
                loop_id = %short_id(ctx.loop_id),
                role = %role_id,
                error = %e,
                "exec_arbiter_rank: session creation failed"
            );
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
            return Err(format!("arbiter.rank session creation failed: {e}"));
        }
    };

    // 7. Link session to loop
    // 7. Track session in runtime state BEFORE linking — reduces the orphan
    //    window since retries can detect the already-created session.
    ctx.snapshot
        .runtime
        .created_session_ids
        .entry(role_id.to_string())
        .or_default()
        .push(session.id.clone());

    // 8. Link session to loop
    LoopService::add_loop_session(
        ctx.conn,
        planeai_core::loop_service::AddLoopSessionParams {
            loop_id: ctx.loop_id.to_string(),
            session_id: session.id.clone(),
            role: role_id.to_string(),
            round: round as i64,
            provider: Some(effective_provider.clone()),
            status: "active".to_string(),
        },
    )
    .map_err(|e| {
        tracing::warn!(
            session_id = %session.id,
            loop_id = %ctx.loop_id,
            role = %role_id,
            "orphaned arbiter session: created but failed to link to loop; manual cleanup may be needed"
        );
        format!("failed to link arbiter session to loop: {e}")
    })?;

    // 9. Append event and advance
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
            "candidates_reviewed": ctx.snapshot.runtime.candidate_handoffs.len(),
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
            "arbiter_session",
            Value::Object(vec![
                field("id", str_val(short_id(&session.id))),
                field("role", str_val(role_id)),
                field("provider", str_val(&effective_provider)),
            ]),
        )],
        next_actions: vec![format!(
            "wait for arbiter handoff, then run `planeai-cli axi loop tick {}`",
            short_id(ctx.loop_id)
        )],
    })
}

/// Build a formatted string describing all candidate sessions and their handoff status
/// for injection into the arbiter prompt.
fn build_candidates_context(ctx: &TickContext, candidate_role: &str) -> String {
    let mut lines = Vec::new();

    // Collect candidate info from loop_sessions table
    let loop_sessions = LoopService::list_loop_sessions(ctx.conn, ctx.loop_id).unwrap_or_default();

    for ls in &loop_sessions {
        // Only include sessions matching the candidate role
        if ls.role != candidate_role {
            continue;
        }

        let handoff_status = ctx
            .snapshot
            .runtime
            .candidate_handoffs
            .get(&ls.session_id)
            .map(|s| s.as_str())
            .unwrap_or("unknown");

        let provider = ls.provider.as_deref().unwrap_or("unknown");

        // Try to get branch info from the session
        let branch = crate::db::get_session(ctx.conn, &ls.session_id)
            .ok()
            .flatten()
            .map(|s| s.branch)
            .unwrap_or_else(|| "(unknown)".to_string());

        lines.push(format!(
            "- Session: {} | Provider: {} | Branch: {} | Handoff: {}",
            short_id(&ls.session_id),
            provider,
            branch,
            handoff_status
        ));
    }

    if lines.is_empty() {
        "(no candidates found)".to_string()
    } else {
        lines.join("\n")
    }
}

/// Render a prompt template with additional `candidates` variable in the context.
fn render_prompt_with_candidates(
    template: &str,
    snapshot: &RecipeSnapshot,
    loop_id: &str,
    candidates_context: &str,
) -> String {
    render_prompt_inner(template, snapshot, loop_id, Some(candidates_context))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Resolve the loop run and its associated project. Shared by session-creating executors.
fn resolve_loop_project(
    ctx: &TickContext,
) -> Result<(planeai_core::loop_run::LoopRun, crate::db::Project), String> {
    let loop_run = LoopService::get_loop(ctx.conn, ctx.loop_id)
        .map_err(|e| format!("failed to load loop: {e}"))?
        .ok_or_else(|| "loop not found".to_string())?;
    let project = crate::db::get_project(ctx.conn, &loop_run.project_id)
        .map_err(|e| format!("failed to resolve project: {e}"))?
        .ok_or_else(|| format!("project not found: {}", loop_run.project_id))?;
    Ok((loop_run, project))
}

/// Resolve the branch name and whether it's a new branch for a session.create step.
///
/// If the step has a `branch` field, render it through the template engine.
/// If the rendered result is non-empty, use it as-is (existing branch, new_branch=false).
/// Otherwise, fall back to the loop-generated branch name (new_branch=true).
fn resolve_branch_for_step(
    step: &RecipeStep,
    snapshot: &RecipeSnapshot,
    loop_id: &str,
) -> (String, bool) {
    let role_id = step.role.as_deref().unwrap_or("default");
    let round = snapshot.runtime.round;
    let generated = format!("loop/{}/{}-r{}", short_id(loop_id), role_id, round);

    if let Some(ref branch_tpl) = step.branch {
        let rendered = render_prompt(branch_tpl, snapshot, loop_id);
        let trimmed = rendered.trim();
        if !trimmed.is_empty() {
            return (trimmed.to_string(), false);
        }
    }

    (generated, true)
}

fn find_step<'a>(steps: &'a [RecipeStep], id: &str) -> Option<&'a RecipeStep> {
    steps.iter().find(|s| s.id == id)
}

/// Extract the first 8 characters of an ID for display.
pub(crate) fn short_id(id: &str) -> &str {
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
/// Atomically derives and persists LoopStatus from the current step pointer,
/// making the step pointer the single authority for loop state.
fn save_snapshot(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &RecipeSnapshot,
) -> Result<(), String> {
    LoopService::persist_snapshot(conn, loop_id, snapshot)
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
    render_prompt_inner(template, snapshot, loop_id, None)
}

/// Internal prompt renderer. Accepts optional extra context (e.g., candidates summary).
/// Both `render_prompt` and `render_prompt_with_candidates` delegate here.
fn render_prompt_inner(
    template: &str,
    snapshot: &RecipeSnapshot,
    loop_id: &str,
    candidates_context: Option<&str>,
) -> String {
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

    // Build the context — candidates is included as empty string when absent so
    // templates referencing {{ candidates }} don't fail.
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
        candidates => candidates_context.unwrap_or(""),
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

    for i in 0..MAX_AUTO_TICKS {
        if check_human_wait_before_tick && is_human_wait_step(snapshot) {
            break;
        }

        tracing::debug!(loop_id = %short_id(loop_id), tick = i, "[DEBUG-lsr1] auto_advance_with_arc: acquiring lock");
        let conn = match conn_arc.lock() {
            Ok(c) => c,
            Err(_) => break,
        };
        tracing::debug!(loop_id = %short_id(loop_id), tick = i, "[DEBUG-lsr1] auto_advance_with_arc: lock acquired");

        let step_before = snapshot.runtime.current_step.clone();

        let (_output, code) = tick_recipe(&conn, loop_id, snapshot);

        // tick_recipe → save_snapshot already persisted the snapshot with derived status

        if code != 0 {
            tracing::debug!(loop_id = %short_id(loop_id), tick = i, "[DEBUG-lsr1] auto_advance_with_arc: tick failed, releasing lock");
            break;
        }

        // If the step didn't advance, the loop is waiting for external input.
        if snapshot.runtime.current_step == step_before {
            tracing::debug!(loop_id = %short_id(loop_id), tick = i, "[DEBUG-lsr1] auto_advance_with_arc: step didn't advance, releasing lock");
            drop(conn);
            break;
        }

        let should_stop = if let Ok(Some(r)) = LoopService::get_loop(&conn, loop_id) {
            r.status.is_executor_terminal() || r.status.is_intervention_required()
        } else {
            false
        };

        tracing::debug!(loop_id = %short_id(loop_id), tick = i, should_stop, "[DEBUG-lsr1] auto_advance_with_arc: releasing lock");
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

        let step_before = snapshot.runtime.current_step.clone();

        let (_output, code) = tick_recipe(conn, loop_id, snapshot);

        // tick_recipe → save_snapshot already persisted the snapshot with derived status

        if code != 0 {
            break;
        }

        // If the step didn't advance, the loop is waiting for external input
        // (e.g., handoff.wait with no handoff available). Stop to avoid
        // spinning and emitting duplicate events.
        if snapshot.runtime.current_step == step_before {
            break;
        }

        if let Ok(Some(r)) = LoopService::get_loop(conn, loop_id) {
            if r.status.is_executor_terminal() || r.status.is_intervention_required() {
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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use planeai_core::loop_recipe_service::{RecipeRuntime, RecipeSnapshot, SnapshotPolicy};
    use std::collections::BTreeMap;

    fn minimal_snapshot(
        steps: Vec<RecipeStep>,
        inputs: BTreeMap<String, serde_json::Value>,
    ) -> RecipeSnapshot {
        let first_step = steps.first().map(|s| s.id.clone()).unwrap_or_default();
        RecipeSnapshot {
            recipe_schema: "planeai.loop.recipe.v1".into(),
            recipe_id: "test-recipe".into(),
            recipe_name: None,
            recipe_description: None,
            recipe_source: "builtin".into(),
            recipe_path: None,
            inputs,
            input_defs: BTreeMap::new(),
            runtime: RecipeRuntime {
                current_step: first_step,
                tick_count: 0,
                round: 1,
                created_session_ids: BTreeMap::new(),
                last_error: None,
                last_handoff_consumed_at: None,
                status_override: None,
                last_activity_at: None,
                session_observations: BTreeMap::new(),
                candidate_handoffs: BTreeMap::new(),
                candidates_query_failures: 0,
            },
            policy: SnapshotPolicy {
                max_rounds: 3,
                max_ticks: 50,
                max_sessions: 5,
                stale_after_ms: None,
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
            branch: None,
            from: None,
            on: None,
            status: None,
            next: None,
            select: None,
            event_kind: None,
            gates: vec![],
            providers: None,
        }
    }

    // ─── render_prompt tests ─────────────────────────────────────────────────

    #[test]
    fn render_prompt_substitutes_input_variables() {
        let mut inputs = BTreeMap::new();
        inputs.insert(
            "goal".to_string(),
            serde_json::Value::String("fix the bug".to_string()),
        );
        let snapshot = minimal_snapshot(vec![], inputs);

        let result = render_prompt("Do this: {{ inputs.goal }}", &snapshot, "loop-123");
        assert_eq!(result, "Do this: fix the bug");
    }

    #[test]
    fn render_prompt_compact_syntax() {
        let mut inputs = BTreeMap::new();
        inputs.insert(
            "goal".to_string(),
            serde_json::Value::String("ship it".to_string()),
        );
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
        inputs.insert(
            "task_key".to_string(),
            serde_json::Value::String("PROJ-1".to_string()),
        );
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

    #[test]
    fn render_prompt_default_filter_with_missing_input() {
        // This is the pattern used in maker-verifier recipe gate commands:
        // {{ inputs.gate_command | default('make ci') }}
        let snapshot = minimal_snapshot(vec![], BTreeMap::new());
        let template = "{{ inputs.gate_command | default('make ci') }}";
        let result = render_prompt(template, &snapshot, "loop-1");
        assert_eq!(
            result, "make ci",
            "default filter should produce 'make ci' when input is absent"
        );
    }

    #[test]
    fn render_prompt_default_filter_with_present_input() {
        let mut inputs = BTreeMap::new();
        inputs.insert(
            "gate_command".to_string(),
            serde_json::Value::String("cargo test".to_string()),
        );
        let snapshot = minimal_snapshot(vec![], inputs);
        let template = "{{ inputs.gate_command | default('make ci') }}";
        let result = render_prompt(template, &snapshot, "loop-1");
        assert_eq!(result, "cargo test");
    }

    #[test]
    fn render_prompt_provider_template_with_default() {
        // Simulates roles.provider = "{{ inputs.provider | default('default') }}"
        let snapshot = minimal_snapshot(vec![], BTreeMap::new());
        let template = "{{ inputs.provider | default('default') }}";
        let result = render_prompt(template, &snapshot, "loop-1");
        assert_eq!(result, "default");
    }

    #[test]
    fn render_prompt_provider_template_with_input() {
        let mut inputs = BTreeMap::new();
        inputs.insert(
            "provider".to_string(),
            serde_json::Value::String("claude".to_string()),
        );
        let snapshot = minimal_snapshot(vec![], inputs);
        let template = "{{ inputs.provider | default('default') }}";
        let result = render_prompt(template, &snapshot, "loop-1");
        assert_eq!(result, "claude");
    }

    #[test]
    fn render_prompt_with_boolean_input() {
        let mut inputs = BTreeMap::new();
        inputs.insert("draft".to_string(), serde_json::Value::Bool(true));
        let snapshot = minimal_snapshot(vec![], inputs);
        let template = "{{ inputs.draft }}";
        let result = render_prompt(template, &snapshot, "loop-1");
        assert_eq!(result, "true");
    }

    #[test]
    fn render_prompt_with_number_input() {
        let mut inputs = BTreeMap::new();
        inputs.insert("count".to_string(), serde_json::json!(42));
        let snapshot = minimal_snapshot(vec![], inputs);
        let template = "{{ inputs.count }}";
        let result = render_prompt(template, &snapshot, "loop-1");
        assert_eq!(result, "42");
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

    // ─── resolve_branch_for_step tests ───────────────────────────────────────

    #[test]
    fn resolve_branch_uses_step_branch_when_present() {
        let mut inputs = BTreeMap::new();
        inputs.insert(
            "branch".to_string(),
            serde_json::Value::String("feature/my-branch".to_string()),
        );
        let snapshot = minimal_snapshot(vec![], inputs);

        let mut step = make_step("create_maker", "session.create");
        step.branch = Some("{{ inputs.branch }}".to_string());

        let (branch_name, new_branch) = resolve_branch_for_step(&step, &snapshot, "loop-abc12345");
        assert_eq!(branch_name, "feature/my-branch");
        assert!(!new_branch, "existing branch should set new_branch=false");
    }

    #[test]
    fn resolve_branch_falls_back_to_generated_when_absent() {
        let snapshot = minimal_snapshot(vec![], BTreeMap::new());

        let mut step = make_step("create_maker", "session.create");
        step.role = Some("maker".to_string());

        let (branch_name, new_branch) = resolve_branch_for_step(&step, &snapshot, "loop-abc12345");
        assert_eq!(branch_name, "loop/loop-abc/maker-r1");
        assert!(new_branch, "generated branch should set new_branch=true");
    }

    #[test]
    fn resolve_branch_falls_back_when_rendered_empty() {
        let snapshot = minimal_snapshot(vec![], BTreeMap::new());

        let mut step = make_step("create_maker", "session.create");
        step.role = Some("maker".to_string());
        step.branch = Some("{{ inputs.branch }}".to_string()); // inputs.branch is not set

        let (branch_name, new_branch) = resolve_branch_for_step(&step, &snapshot, "loop-abc12345");
        // When rendered template is empty, fall back to generated branch
        assert_eq!(branch_name, "loop/loop-abc/maker-r1");
        assert!(new_branch);
    }

    #[test]
    fn resolve_branch_renders_template_with_runtime_vars() {
        let mut inputs = BTreeMap::new();
        inputs.insert(
            "branch".to_string(),
            serde_json::Value::String("fix/round".to_string()),
        );
        let snapshot = minimal_snapshot(vec![], inputs);

        let mut step = make_step("create_maker", "session.create");
        step.branch = Some("{{ inputs.branch }}-{{ runtime.round }}".to_string());

        let (branch_name, new_branch) = resolve_branch_for_step(&step, &snapshot, "loop-abc12345");
        assert_eq!(branch_name, "fix/round-1");
        assert!(!new_branch);
    }
}
