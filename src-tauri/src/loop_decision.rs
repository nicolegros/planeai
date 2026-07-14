//! Pure decision logic for recipe tick execution.
//!
//! Every function in this module is deterministic. Side effects are described as
//! [`Effect`] values; actual execution is delegated to an [`EffectExecutor`].
//!
//! # Design: `&mut RecipeSnapshot` instead of owned return
//!
//! The spec calls for `(snapshot, step) → (NewSnapshot, Vec<Effect>)`. We use
//! `(&mut RecipeSnapshot, &RecipeStep, &str) → Result<TickDecision, String>`
//! where `TickDecision` carries the `Vec<Effect>` and the snapshot is mutated
//! in-place. This is the standard Rust idiom for "return new state" without
//! cloning a ~2KB struct on every step. The functions remain **pure** in the
//! meaningful sense: deterministic, no I/O, fully testable with just a snapshot
//! and step inputs.

use planeai_core::loop_recipe::*;
use planeai_core::loop_recipe_service::RecipeSnapshot;
use planeai_core::loop_run::{LoopStatus, LoopTrigger};
use planeai_toon::{field, str_val, Field, Value};

use crate::loop_effects::Effect;

// ─── Decision Output ─────────────────────────────────────────────────────────

pub struct TickDecision {
    pub step_id: String,
    pub step_kind: String,
    pub status: String,
    pub extra: Vec<Field>,
    pub next_actions: Vec<String>,
    pub effects: Vec<Effect>,
}

pub enum PreTickResult {
    EarlyReturn { output: String, code: i32 },
    Proceed { step: Box<RecipeStep> },
}

// ─── Pre-tick Guards ─────────────────────────────────────────────────────────

pub fn pre_tick_guard(
    snapshot: &RecipeSnapshot,
    loop_status: Option<&LoopStatus>,
    loop_id: &str,
) -> PreTickResult {
    use planeai_toon::render;
    if let Some(status) = loop_status {
        if status.is_executor_terminal() {
            return PreTickResult::EarlyReturn {
                output: render(&[field(
                    "error",
                    str_val(&format!(
                        "loop {} is in terminal status '{}' — cannot execute steps",
                        short_id(loop_id),
                        status.as_str()
                    )),
                )]),
                code: 1,
            };
        }
        if status.is_intervention_required() {
            let tick_fields = vec![
                field("loop_id", str_val(loop_id)),
                field("recipe_id", str_val(&snapshot.recipe_id)),
                field("step_id", str_val(&snapshot.runtime.current_step)),
                field("step_kind", str_val("(guarded)")),
                field("status", str_val(status.as_str())),
            ];
            return PreTickResult::EarlyReturn {
                output: render(&[
                    field("loop_tick", Value::Object(tick_fields)),
                    field(
                        "next_actions",
                        Value::List(vec![
                            "loop requires human intervention before it can proceed".into(),
                        ]),
                    ),
                ]),
                code: 0,
            };
        }
    }
    if snapshot.runtime.tick_count >= snapshot.policy.max_ticks {
        return PreTickResult::EarlyReturn {
            output: planeai_toon::render(&[
                field("error", str_val("max_ticks exceeded")),
                field(
                    "loop_tick",
                    Value::Object(vec![
                        field("loop_id", str_val(loop_id)),
                        field("status", str_val("failed")),
                    ]),
                ),
            ]),
            code: 1,
        };
    }
    let step = match find_step(&snapshot.steps, &snapshot.runtime.current_step) {
        Some(s) => s.clone(),
        None => {
            return PreTickResult::EarlyReturn {
                output: planeai_toon::render(&[field(
                    "error",
                    str_val(&format!(
                        "recipe step not found: {}",
                        snapshot.runtime.current_step
                    )),
                )]),
                code: 1,
            }
        }
    };
    if !step.is_v1_executable() {
        let help = if step.is_recognized() {
            format!(
                "step kind '{}' is recognized but not executable until a future release",
                step.kind
            )
        } else {
            format!("step kind '{}' is unknown", step.kind)
        };
        return PreTickResult::EarlyReturn {
            output: planeai_toon::render(&[
                field("error", str_val("unsupported recipe step kind")),
                field("step_id", str_val(&step.id)),
                field("kind", str_val(&step.kind)),
                field("help", Value::List(vec![help])),
            ]),
            code: 1,
        };
    }
    PreTickResult::Proceed {
        step: Box::new(step),
    }
}

pub fn max_ticks_effects(loop_id: &str) -> Vec<Effect> {
    vec![
        Effect::TransitionLoop {
            loop_id: loop_id.to_string(),
            trigger: LoopTrigger::MaxTicksExceeded,
        },
        Effect::AppendEvent {
            loop_id: loop_id.to_string(),
            kind: "recipe_step_failed".into(),
            payload: serde_json::json!({"reason": "max_ticks exceeded"}),
        },
    ]
}

// ─── session.create ──────────────────────────────────────────────────────────

pub fn decide_session_create(
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
    loop_id: &str,
    existing_session_count: u32,
    loop_run: &planeai_core::loop_run::LoopRun,
    maker_branch: Option<String>,
) -> Result<TickDecision, String> {
    let role_id = step.role.as_deref().unwrap_or("default");
    if existing_session_count >= snapshot.policy.max_sessions {
        return Ok(TickDecision {
            step_id: step.id.clone(),
            step_kind: step.kind.clone(),
            status: "needs_human".into(),
            extra: vec![field("limit", str_val("max_sessions"))],
            next_actions: vec!["max_sessions reached — cannot create more sessions".to_string()],
            effects: vec![
                Effect::TransitionLoop {
                    loop_id: loop_id.to_string(),
                    trigger: LoopTrigger::SessionLimitReached,
                },
                Effect::AppendEvent {
                    loop_id: loop_id.to_string(),
                    kind: "recipe_runtime_limit_reached".into(),
                    payload: serde_json::json!({"step_id": step.id, "limit": "max_sessions"}),
                },
                Effect::SaveSnapshot {
                    loop_id: loop_id.to_string(),
                },
            ],
        });
    }

    let role = snapshot.roles.get(role_id).cloned();
    let provider = role
        .as_ref()
        .map(|r| r.provider.clone())
        .unwrap_or_else(|| "default".to_string());
    let isolation = role
        .as_ref()
        .map(|r| r.isolation.clone())
        .unwrap_or_else(|| "worktree".to_string());
    let provider_opt = if provider == "default" {
        None
    } else {
        Some(provider.clone())
    };

    let round = snapshot.runtime.round;
    let use_worktree = isolation == "worktree";
    let base_branch = if round > 1 && use_worktree {
        Some(format!(
            "loop/{}/{}-r{}",
            short_id(loop_id),
            role_id,
            round - 1
        ))
    } else if !use_worktree {
        maker_branch
    } else {
        snapshot
            .inputs
            .get("base_branch")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };

    let rendered_prompt = step
        .prompt
        .as_ref()
        .map(|tpl| render_prompt(tpl, snapshot, loop_id));
    let (branch_name, new_branch) = resolve_branch_for_step(step, snapshot, loop_id);
    let session_name = format!("{} ({})", role_id, short_id(loop_id));

    Ok(TickDecision {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "observing".into(),
        extra: vec![],
        next_actions: vec![format!(
            "wait for {} handoff, then run `planeai-cli axi loop tick {}`",
            role_id,
            short_id(loop_id)
        )],
        effects: vec![Effect::CreateSession {
            role: role_id.to_string(),
            provider: provider_opt,
            branch: branch_name,
            new_branch,
            base_branch,
            worktree: use_worktree,
            auto_approve: snapshot.policy.auto_approve,
            project_id: loop_run.project_id.clone(),
            loop_id: loop_id.to_string(),
            task_key: loop_run.task_key.clone(),
            parent_session_id: loop_run.created_by_session_id.clone(),
            session_name,
            prompt: rendered_prompt,
        }],
    })
}

pub fn finalize_session_create(
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
    loop_id: &str,
    session_id: &str,
    role_id: &str,
    provider: &str,
) -> Vec<Effect> {
    let round = snapshot.runtime.round;
    snapshot
        .runtime
        .created_session_ids
        .entry(role_id.to_string())
        .or_default()
        .push(session_id.to_string());
    advance_step(snapshot, step);
    vec![
        Effect::LinkSession {
            loop_id: loop_id.to_string(),
            session_id: session_id.to_string(),
            role: role_id.to_string(),
            round: round as i64,
            provider: Some(provider.to_string()),
        },
        Effect::AppendEvent {
            loop_id: loop_id.to_string(),
            kind: "recipe_step_completed".into(),
            payload: serde_json::json!({"step_id": step.id, "kind": step.kind, "session_id": session_id, "role": role_id, "round": round}),
        },
        Effect::SaveSnapshot {
            loop_id: loop_id.to_string(),
        },
    ]
}

// ─── session.prompt ──────────────────────────────────────────────────────────

pub fn decide_session_prompt(
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
    loop_id: &str,
) -> Result<TickDecision, String> {
    let role_id = step.role.as_deref().unwrap_or("default");
    let session_ids = snapshot
        .runtime
        .created_session_ids
        .get(role_id)
        .cloned()
        .unwrap_or_default();
    if session_ids.is_empty() {
        return Err(format!("step '{}': no sessions exist for role '{role_id}' — create a session first with session.create", step.id));
    }
    let select = step.select.as_deref().unwrap_or("latest");
    let session_id = match select {
        "latest" => session_ids.last().unwrap().clone(),
        _ => {
            return Err(format!(
                "step '{}': unsupported select value '{}' — only 'latest' is supported",
                step.id, select
            ))
        }
    };
    let prompt_template = step.prompt.as_deref().unwrap_or("");
    if prompt_template.is_empty() {
        return Err(format!(
            "step '{}': session.prompt requires a 'prompt' template",
            step.id
        ));
    }
    let rendered = render_prompt(prompt_template, snapshot, loop_id);
    advance_step(snapshot, step);
    Ok(TickDecision {
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
            short_id(loop_id)
        )],
        effects: vec![
            Effect::SendPrompt {
                session_id: session_id.clone(),
                prompt_text: rendered,
            },
            Effect::AppendEvent {
                loop_id: loop_id.to_string(),
                kind: "recipe_step_completed".into(),
                payload: serde_json::json!({"step_id": step.id, "kind": step.kind, "session_id": session_id, "role": role_id, "round": snapshot.runtime.round}),
            },
            Effect::SaveSnapshot {
                loop_id: loop_id.to_string(),
            },
        ],
    })
}

// ─── handoff.wait ────────────────────────────────────────────────────────────

pub fn decide_handoff_wait_waiting(
    _snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
    loop_id: &str,
    role_id: &str,
) -> TickDecision {
    TickDecision {
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
        effects: vec![
            Effect::TransitionLoop {
                loop_id: loop_id.to_string(),
                trigger: LoopTrigger::HandoffWaiting,
            },
            Effect::AppendEvent {
                loop_id: loop_id.to_string(),
                kind: "recipe_step_waiting".into(),
                payload: serde_json::json!({"step_id": step.id, "waiting_for": "handoff", "role": role_id}),
            },
            Effect::SaveSnapshot {
                loop_id: loop_id.to_string(),
            },
        ],
    }
}

pub fn decide_handoff_wait_consumed(
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
    loop_id: &str,
    session_id: &str,
    handoff_status: &str,
    handoff_summary: Option<String>,
) -> TickDecision {
    snapshot.runtime.last_handoff_consumed_at = Some(chrono::Utc::now().to_rfc3339());
    let next_step = step
        .on
        .as_ref()
        .and_then(|m| m.get(handoff_status))
        .cloned();
    if handoff_status != "completed" {
        if let Some(summary) = handoff_summary {
            snapshot.runtime.last_error = Some(summary);
        }
    }
    if let Some(ref ns) = next_step {
        snapshot.runtime.current_step = ns.clone();
    } else {
        advance_step(snapshot, step);
    }
    let next_display = next_step.as_deref().unwrap_or("(end)");
    TickDecision {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "observing".into(),
        extra: vec![
            field(
                "matched_handoff",
                Value::Object(vec![
                    field("session_id", str_val(short_id(session_id))),
                    field("status", str_val(handoff_status)),
                ]),
            ),
            field("next_step", str_val(next_display)),
        ],
        next_actions: vec![format!(
            "run `planeai-cli axi loop tick {}` to apply next step",
            short_id(loop_id)
        )],
        effects: vec![
            Effect::TransitionLoop {
                loop_id: loop_id.to_string(),
                trigger: LoopTrigger::HandoffConsumed,
            },
            Effect::AppendEvent {
                loop_id: loop_id.to_string(),
                kind: "recipe_step_completed".into(),
                payload: serde_json::json!({"step_id": step.id, "kind": step.kind, "handoff_status": handoff_status, "session_id": session_id, "next_step": next_step}),
            },
            Effect::SaveSnapshot {
                loop_id: loop_id.to_string(),
            },
        ],
    }
}

// ─── loop.status ─────────────────────────────────────────────────────────────

pub fn decide_loop_status(
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
    loop_id: &str,
) -> Result<TickDecision, String> {
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
    if !new_status.is_executor_terminal() && !new_status.is_intervention_required() {
        advance_step(snapshot, step);
    }
    let next_action = if new_status.is_executor_terminal() {
        "review the loop output before merging".to_string()
    } else if new_status.is_intervention_required() {
        "human intervention required".to_string()
    } else {
        format!(
            "run `planeai-cli axi loop tick {}` to continue",
            short_id(loop_id)
        )
    };
    Ok(TickDecision {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: status_str.to_string(),
        extra: vec![field("state_changed", Value::Bool(true))],
        next_actions: vec![next_action],
        effects: vec![
            Effect::TransitionLoop {
                loop_id: loop_id.to_string(),
                trigger: LoopTrigger::RecipeSetStatus(new_status),
            },
            Effect::AppendEvent {
                loop_id: loop_id.to_string(),
                kind: "recipe_step_completed".into(),
                payload: serde_json::json!({"step_id": step.id, "kind": step.kind, "status": status_str}),
            },
            Effect::SaveSnapshot {
                loop_id: loop_id.to_string(),
            },
        ],
    })
}

// ─── loop.event ──────────────────────────────────────────────────────────────

pub fn decide_loop_event(
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
    loop_id: &str,
) -> TickDecision {
    let event_kind = step.event_kind.as_deref().unwrap_or("recipe_event");
    advance_step(snapshot, step);
    TickDecision {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "observing".into(),
        extra: vec![field("event_kind", str_val(event_kind))],
        next_actions: vec![format!(
            "run `planeai-cli axi loop tick {}` to continue",
            short_id(loop_id)
        )],
        effects: vec![
            Effect::AppendEvent {
                loop_id: loop_id.to_string(),
                kind: event_kind.to_string(),
                payload: serde_json::json!({"step_id": step.id}),
            },
            Effect::SaveSnapshot {
                loop_id: loop_id.to_string(),
            },
        ],
    }
}

// ─── human.wait ──────────────────────────────────────────────────────────────

pub fn decide_human_wait(
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
    loop_id: &str,
) -> TickDecision {
    advance_step(snapshot, step);
    TickDecision {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "needs_human".into(),
        extra: vec![],
        next_actions: vec!["human review required before the loop can proceed".to_string()],
        effects: vec![
            Effect::TransitionLoop {
                loop_id: loop_id.to_string(),
                trigger: LoopTrigger::HumanWaitReached,
            },
            Effect::AppendEvent {
                loop_id: loop_id.to_string(),
                kind: "recipe_step_completed".into(),
                payload: serde_json::json!({"step_id": step.id, "kind": step.kind}),
            },
            Effect::SaveSnapshot {
                loop_id: loop_id.to_string(),
            },
        ],
    }
}

// ─── round.next ──────────────────────────────────────────────────────────────

pub fn decide_round_next(
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
    loop_id: &str,
) -> TickDecision {
    if snapshot.runtime.round >= snapshot.policy.max_rounds {
        return TickDecision {
            step_id: step.id.clone(),
            step_kind: step.kind.clone(),
            status: "blocked".into(),
            extra: vec![
                field("limit", str_val("max_rounds")),
                field("value", str_val(&snapshot.policy.max_rounds.to_string())),
            ],
            next_actions: vec![
                "max_rounds reached — inspect the loop and decide whether to continue manually"
                    .to_string(),
            ],
            effects: vec![
                Effect::TransitionLoop {
                    loop_id: loop_id.to_string(),
                    trigger: LoopTrigger::RoundBlocked,
                },
                Effect::AppendEvent {
                    loop_id: loop_id.to_string(),
                    kind: "recipe_runtime_limit_reached".into(),
                    payload: serde_json::json!({"step_id": step.id, "limit": "max_rounds", "value": snapshot.policy.max_rounds, "current_round": snapshot.runtime.round}),
                },
                Effect::SaveSnapshot {
                    loop_id: loop_id.to_string(),
                },
            ],
        };
    }
    snapshot.runtime.round += 1;
    let new_round = snapshot.runtime.round;
    advance_step(snapshot, step);
    TickDecision {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "running".into(),
        extra: vec![field("round", str_val(&new_round.to_string()))],
        next_actions: vec![format!(
            "round {} started — run `planeai-cli axi loop tick {}` to continue",
            new_round,
            short_id(loop_id)
        )],
        effects: vec![
            Effect::UpdateCurrentRound {
                loop_id: loop_id.to_string(),
                round: new_round as i64,
            },
            Effect::AppendEvent {
                loop_id: loop_id.to_string(),
                kind: "recipe_round_started".into(),
                payload: serde_json::json!({"step_id": step.id, "round": new_round}),
            },
            Effect::SaveSnapshot {
                loop_id: loop_id.to_string(),
            },
        ],
    }
}

// ─── gates.run ───────────────────────────────────────────────────────────────

pub fn decide_gates_run_preflight(step: &RecipeStep) -> Result<(), String> {
    if step.gates.is_empty() {
        return Err(format!(
            "step '{}': gates.run requires at least one gate declaration",
            step.id
        ));
    }
    Ok(())
}

pub fn decide_gates_run_result(
    snapshot: &mut RecipeSnapshot,
    step: &RecipeStep,
    loop_id: &str,
    overall_status: &str,
    failed_gate_name: &str,
    failed_gate_output: Option<&str>,
    failed_gate_output_path: Option<&str>,
) -> TickDecision {
    let next_step = step
        .on
        .as_ref()
        .and_then(|m| m.get(overall_status))
        .cloned();
    if overall_status != "pass" {
        snapshot.runtime.last_error = Some(if let Some(output) = failed_gate_output {
            let path_note = failed_gate_output_path
                .map(|p| format!("\n\nFull output log: {p}\nRead this file for complete details."))
                .unwrap_or_default();
            const MAX: usize = 100_000;
            let display = if output.len() > MAX {
                let end = output[..MAX]
                    .char_indices()
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                format!("{}\n\n… [output truncated]", &output[..end])
            } else {
                output.to_string()
            };
            format!(
                "Gate '{}' failed (exit status: {}).\n\nOutput:\n{}{}",
                failed_gate_name, overall_status, display, path_note
            )
        } else {
            format!("Gate '{}' returned '{}'", failed_gate_name, overall_status)
        });
    }
    let event_kind = if overall_status == "pass" {
        "recipe_step_completed"
    } else {
        "recipe_step_failed"
    };
    let mut effects = vec![Effect::AppendEvent {
        loop_id: loop_id.to_string(),
        kind: event_kind.to_string(),
        payload: serde_json::json!({"step_id": step.id, "kind": step.kind, "gates_result": overall_status, "next_step": next_step}),
    }];

    if let Some(ref ns) = next_step {
        snapshot.runtime.current_step = ns.clone();
    } else if overall_status != "pass" {
        effects.push(Effect::TransitionLoop {
            loop_id: loop_id.to_string(),
            trigger: LoopTrigger::HumanWaitReached,
        });
        effects.push(Effect::SaveSnapshot {
            loop_id: loop_id.to_string(),
        });
        return TickDecision {
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
            effects,
        };
    } else {
        advance_step(snapshot, step);
    }
    effects.push(Effect::SaveSnapshot {
        loop_id: loop_id.to_string(),
    });
    let next_display = next_step.as_deref().unwrap_or("(end)");
    TickDecision {
        step_id: step.id.clone(),
        step_kind: step.kind.clone(),
        status: "verifying".into(),
        extra: vec![
            field("gates_result", str_val(overall_status)),
            field("next_step", str_val(next_display)),
        ],
        next_actions: vec![format!(
            "run `planeai-cli axi loop tick {}` to continue to '{}'",
            short_id(loop_id),
            next_display
        )],
        effects,
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

pub fn find_step<'a>(steps: &'a [RecipeStep], id: &str) -> Option<&'a RecipeStep> {
    steps.iter().find(|s| s.id == id)
}

pub fn short_id(id: &str) -> &str {
    // Slice by char boundary to avoid panics on multi-byte UTF-8
    id.get(..8).unwrap_or(id)
}

pub fn advance_step(snapshot: &mut RecipeSnapshot, current: &RecipeStep) {
    if let Some(ref next) = current.next {
        snapshot.runtime.current_step = next.clone();
        return;
    }
    let idx = snapshot.steps.iter().position(|s| s.id == current.id);
    if let Some(i) = idx {
        if i + 1 < snapshot.steps.len() {
            snapshot.runtime.current_step = snapshot.steps[i + 1].id.clone();
        } else {
            // Last step completed — set a sentinel so pre_tick_guard returns
            // "recipe step not found" on the next tick, preventing re-execution.
            snapshot.runtime.current_step = "__completed__".to_string();
        }
    }
}

/// Resolve branch name and whether it's new. If step.branch is set and renders
/// non-empty, use it (existing branch). Otherwise generate loop-managed name.
pub fn resolve_branch_for_step(
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

pub fn render_prompt(template: &str, snapshot: &RecipeSnapshot, loop_id: &str) -> String {
    use minijinja::{context, Environment};
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    if let Err(e) = env.add_template("prompt", template) {
        tracing::warn!(error = %e, "recipe template parse failed; using raw template");
        return template.to_string();
    }
    let tpl = match env.get_template("prompt") {
        Ok(t) => t,
        Err(_) => return template.to_string(),
    };
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
    let ctx = context! {
        inputs => &snapshot.inputs,
        loop_run => context! { id => loop_id },
        recipe => context! { id => &snapshot.recipe_id },
        knowledge => context! { files => &knowledge_str },
        runtime => context! { round => snapshot.runtime.round, last_error => snapshot.runtime.last_error.as_deref().unwrap_or("") },
    };
    tpl.render(ctx).unwrap_or_else(|_| template.to_string())
}

#[allow(dead_code)]
pub fn truncate(s: &str, max: usize) -> String {
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

pub fn is_human_wait_step(snapshot: &RecipeSnapshot) -> bool {
    snapshot
        .steps
        .iter()
        .find(|s| s.id == snapshot.runtime.current_step)
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
            id: id.into(),
            kind: kind.into(),
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
        }
    }

    fn make_loop_run(loop_id: &str, project_id: &str) -> planeai_core::loop_run::LoopRun {
        planeai_core::loop_run::LoopRun {
            id: loop_id.into(),
            project_id: project_id.into(),
            task_key: None,
            created_by_session_id: None,
            strategy: planeai_core::loop_run::LoopStrategy::new("recipe"),
            goal: "test".into(),
            status: LoopStatus::Running,
            current_round: 1,
            max_rounds: 3,
            created_at: "2024-01-01T00:00:00Z".into(),
            updated_at: "2024-01-01T00:00:00Z".into(),
            executor_finished_at: None,
            policy_json: None,
            budget_json: None,
        }
    }

    #[test]
    fn decide_session_create_emits_create_effect() {
        let mut step = make_step("create_maker", STEP_SESSION_CREATE);
        step.role = Some("maker".into());
        let mut snapshot = minimal_snapshot(
            vec![step.clone(), make_step("next", "loop.event")],
            BTreeMap::new(),
        );
        snapshot.roles.insert(
            "maker".into(),
            RecipeRole {
                provider: "claude".into(),
                mode: "write".into(),
                isolation: "worktree".into(),
                instructions: None,
            },
        );
        let decision = decide_session_create(
            &mut snapshot,
            &step,
            "loop-abc123",
            0,
            &make_loop_run("loop-abc123", "p1"),
            None,
        )
        .unwrap();
        assert_eq!(decision.status, "observing");
        assert!(decision
            .effects
            .iter()
            .any(|e| matches!(e, Effect::CreateSession { .. })));
    }

    #[test]
    fn decide_session_create_max_sessions_triggers_needs_human() {
        let mut step = make_step("create_maker", STEP_SESSION_CREATE);
        step.role = Some("maker".into());
        let mut snapshot = minimal_snapshot(vec![step.clone()], BTreeMap::new());
        snapshot.policy.max_sessions = 2;
        let decision = decide_session_create(
            &mut snapshot,
            &step,
            "loop-abc",
            2,
            &make_loop_run("loop-abc", "p1"),
            None,
        )
        .unwrap();
        assert_eq!(decision.status, "needs_human");
        assert!(decision.effects.iter().any(|e| matches!(
            e,
            Effect::TransitionLoop {
                trigger: LoopTrigger::SessionLimitReached,
                ..
            }
        )));
    }

    #[test]
    fn decide_round_next_increments_round() {
        let step = make_step("next_round", STEP_ROUND_NEXT);
        let mut snapshot = minimal_snapshot(
            vec![step.clone(), make_step("after", "loop.event")],
            BTreeMap::new(),
        );
        let decision = decide_round_next(&mut snapshot, &step, "loop-abc");
        assert_eq!(decision.status, "running");
        assert_eq!(snapshot.runtime.round, 2);
        assert_eq!(snapshot.runtime.current_step, "after");
        assert!(decision
            .effects
            .iter()
            .any(|e| matches!(e, Effect::UpdateCurrentRound { round: 2, .. })));
    }

    #[test]
    fn decide_round_next_enforces_max_rounds() {
        let step = make_step("next_round", STEP_ROUND_NEXT);
        let mut snapshot = minimal_snapshot(vec![step.clone()], BTreeMap::new());
        snapshot.policy.max_rounds = 3;
        snapshot.runtime.round = 3;
        let decision = decide_round_next(&mut snapshot, &step, "loop-abc");
        assert_eq!(decision.status, "blocked");
        assert!(decision.effects.iter().any(|e| matches!(
            e,
            Effect::TransitionLoop {
                trigger: LoopTrigger::RoundBlocked,
                ..
            }
        )));
    }

    #[test]
    fn decide_gates_run_result_pass_advances_step() {
        let step = make_step("verify", STEP_GATES_RUN);
        let mut snapshot = minimal_snapshot(
            vec![step.clone(), make_step("done", "loop.event")],
            BTreeMap::new(),
        );
        let decision =
            decide_gates_run_result(&mut snapshot, &step, "loop-abc", "pass", "", None, None);
        assert_eq!(decision.status, "verifying");
        assert_eq!(snapshot.runtime.current_step, "done");
    }

    #[test]
    fn decide_gates_run_result_fail_with_mapping() {
        let mut step = make_step("verify", STEP_GATES_RUN);
        let mut on_map = BTreeMap::new();
        on_map.insert("fail".into(), "retry".into());
        step.on = Some(on_map);
        let mut snapshot = minimal_snapshot(
            vec![
                step.clone(),
                make_step("retry", "session.prompt"),
                make_step("done", "loop.event"),
            ],
            BTreeMap::new(),
        );
        let decision = decide_gates_run_result(
            &mut snapshot,
            &step,
            "loop-abc",
            "fail",
            "test",
            Some("FAILED"),
            None,
        );
        assert_eq!(decision.status, "verifying");
        assert_eq!(snapshot.runtime.current_step, "retry");
        assert!(snapshot.runtime.last_error.is_some());
    }

    #[test]
    fn decide_gates_run_result_fail_no_mapping_needs_human() {
        let step = make_step("verify", STEP_GATES_RUN);
        let mut snapshot = minimal_snapshot(vec![step.clone()], BTreeMap::new());
        let decision =
            decide_gates_run_result(&mut snapshot, &step, "loop-abc", "fail", "lint", None, None);
        assert_eq!(decision.status, "needs_human");
        assert!(decision.effects.iter().any(|e| matches!(
            e,
            Effect::TransitionLoop {
                trigger: LoopTrigger::HumanWaitReached,
                ..
            }
        )));
    }

    #[test]
    fn pre_tick_guard_max_ticks_exceeded() {
        let mut snapshot = minimal_snapshot(vec![make_step("s1", "loop.event")], BTreeMap::new());
        snapshot.policy.max_ticks = 10;
        snapshot.runtime.tick_count = 10;
        match pre_tick_guard(&snapshot, Some(&LoopStatus::Running), "loop-123") {
            PreTickResult::EarlyReturn { code, output } => {
                assert_eq!(code, 1);
                assert!(output.contains("max_ticks"));
            }
            _ => panic!("expected EarlyReturn"),
        }
    }

    #[test]
    fn decide_human_wait_transitions_to_needs_human() {
        let step = make_step("wait", STEP_HUMAN_WAIT);
        let mut snapshot = minimal_snapshot(
            vec![step.clone(), make_step("after", "loop.event")],
            BTreeMap::new(),
        );
        let decision = decide_human_wait(&mut snapshot, &step, "loop-abc");
        assert_eq!(decision.status, "needs_human");
        assert_eq!(snapshot.runtime.current_step, "after");
        assert!(decision.effects.iter().any(|e| matches!(
            e,
            Effect::TransitionLoop {
                trigger: LoopTrigger::HumanWaitReached,
                ..
            }
        )));
    }

    #[test]
    fn render_prompt_substitutes_input_variables() {
        let mut inputs = BTreeMap::new();
        inputs.insert(
            "goal".into(),
            serde_json::Value::String("fix the bug".into()),
        );
        let snapshot = minimal_snapshot(vec![], inputs);
        assert_eq!(
            render_prompt("Do this: {{ inputs.goal }}", &snapshot, "loop-123"),
            "Do this: fix the bug"
        );
    }

    #[test]
    fn render_prompt_with_boolean_input() {
        let mut inputs = BTreeMap::new();
        inputs.insert("draft".into(), serde_json::Value::Bool(true));
        let snapshot = minimal_snapshot(vec![], inputs);
        assert_eq!(
            render_prompt("{{ inputs.draft }}", &snapshot, "loop-1"),
            "true"
        );
    }

    #[test]
    fn render_prompt_default_filter() {
        let snapshot = minimal_snapshot(vec![], BTreeMap::new());
        assert_eq!(
            render_prompt("{{ inputs.x | default('fallback') }}", &snapshot, "l"),
            "fallback"
        );
    }

    #[test]
    fn advance_step_sequential() {
        let steps = vec![
            make_step("s1", "loop.event"),
            make_step("s2", "loop.event"),
            make_step("s3", "loop.event"),
        ];
        let mut snapshot = minimal_snapshot(steps.clone(), BTreeMap::new());
        advance_step(&mut snapshot, &steps[0]);
        assert_eq!(snapshot.runtime.current_step, "s2");
        advance_step(&mut snapshot, &steps[1]);
        assert_eq!(snapshot.runtime.current_step, "s3");
    }

    #[test]
    fn advance_step_explicit_next() {
        let mut s1 = make_step("s1", "loop.event");
        s1.next = Some("s3".into());
        let steps = vec![
            s1.clone(),
            make_step("s2", "loop.event"),
            make_step("s3", "loop.event"),
        ];
        let mut snapshot = minimal_snapshot(steps, BTreeMap::new());
        advance_step(&mut snapshot, &s1);
        assert_eq!(snapshot.runtime.current_step, "s3");
    }

    #[test]
    fn resolve_branch_uses_step_branch_when_set() {
        let mut step = make_step("s1", STEP_SESSION_CREATE);
        step.role = Some("maker".into());
        step.branch = Some("my-feature".into());
        let snapshot = minimal_snapshot(vec![step.clone()], BTreeMap::new());
        let (branch, new) = resolve_branch_for_step(&step, &snapshot, "loop-abc");
        assert_eq!(branch, "my-feature");
        assert!(!new);
    }

    #[test]
    fn resolve_branch_generates_when_no_step_branch() {
        let mut step = make_step("s1", STEP_SESSION_CREATE);
        step.role = Some("maker".into());
        let snapshot = minimal_snapshot(vec![step.clone()], BTreeMap::new());
        let (branch, new) = resolve_branch_for_step(&step, &snapshot, "loop-abcdefgh");
        assert_eq!(branch, "loop/loop-abc/maker-r1");
        assert!(new);
    }

    #[test]
    fn short_id_normal() {
        assert_eq!(short_id("abcdefghijklmnop"), "abcdefgh");
    }

    #[test]
    fn short_id_short_input() {
        assert_eq!(short_id("abc"), "abc");
    }

    #[test]
    fn truncate_within_limit() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_over_limit() {
        let r = truncate("hello world", 5);
        assert!(r.ends_with("..."));
    }
}
