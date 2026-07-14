//! Recipe tick runtime — thin wiring layer between decision logic and effect execution.
//!
//! All business logic lives in [`crate::loop_decision`]. All I/O lives in
//! [`crate::loop_effects`]. This file is pure dispatch + wiring.

use planeai_core::loop_recipe::*;
use planeai_core::loop_recipe_service::RecipeSnapshot;
use planeai_core::loop_run::LoopTrigger;
use planeai_core::loop_service::LoopService;
use planeai_toon::{field, render, str_val, Value};

use crate::loop_decision::{self, PreTickResult, TickDecision};
use crate::loop_effects::{Effect, EffectExecutor, GateStatus, LoopQueries, RealEffectExecutor};

// ─── TickContext ─────────────────────────────────────────────────────────────

/// Shared context threaded through all wiring functions, eliminating repeated
/// parameter lists (conn, executor, queries, snapshot, loop_id).
struct TickCtx<'a> {
    conn: &'a rusqlite::Connection,
    executor: &'a dyn EffectExecutor,
    queries: &'a dyn LoopQueries,
    snapshot: &'a mut RecipeSnapshot,
    loop_id: &'a str,
}

// ─── Rendering ───────────────────────────────────────────────────────────────

fn render_tick_result(loop_id: &str, recipe_id: &str, d: TickDecision) -> (String, i32) {
    let tick = vec![
        field("loop_id", str_val(loop_id)),
        field("recipe_id", str_val(recipe_id)),
        field("step_id", str_val(&d.step_id)),
        field("step_kind", str_val(&d.step_kind)),
        field("status", str_val(&d.status)),
    ];
    let mut f = vec![field("loop_tick", Value::Object(tick))];
    f.extend(d.extra);
    f.push(field("next_actions", Value::List(d.next_actions)));
    (render(&f), 0)
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
    tick_recipe_with_executor(
        conn,
        loop_id,
        snapshot,
        &RealEffectExecutor,
        &RealEffectExecutor,
    )
}

/// Execute one recipe step using provided executor and queries (testable seam).
pub fn tick_recipe_with_executor(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    executor: &dyn EffectExecutor,
    queries: &dyn LoopQueries,
) -> (String, i32) {
    let loop_status = queries
        .get_loop(conn, loop_id)
        .ok()
        .flatten()
        .map(|r| r.status);
    let step = match loop_decision::pre_tick_guard(snapshot, loop_status.as_ref(), loop_id) {
        PreTickResult::EarlyReturn { output, code } => {
            if snapshot.runtime.tick_count >= snapshot.policy.max_ticks {
                for e in loop_decision::max_ticks_effects(loop_id) {
                    let _ = execute_effect(conn, executor, &e, snapshot);
                }
            }
            return (output, code);
        }
        PreTickResult::Proceed { step } => *step,
    };
    snapshot.runtime.tick_count += 1;

    let mut ctx = TickCtx {
        conn,
        executor,
        queries,
        snapshot,
        loop_id,
    };
    let result = match step.kind.as_str() {
        STEP_SESSION_CREATE => exec_session_create(&mut ctx, &step),
        STEP_SESSION_PROMPT => exec_decide(&mut ctx, &step, loop_decision::decide_session_prompt),
        STEP_HANDOFF_WAIT => exec_handoff_wait(&mut ctx, &step),
        STEP_LOOP_STATUS => exec_decide(&mut ctx, &step, loop_decision::decide_loop_status),
        STEP_LOOP_EVENT => exec_decide_ok(&mut ctx, &step, loop_decision::decide_loop_event),
        STEP_HUMAN_WAIT => exec_decide_ok(&mut ctx, &step, loop_decision::decide_human_wait),
        STEP_ROUND_NEXT => exec_decide_ok(&mut ctx, &step, loop_decision::decide_round_next),
        STEP_GATES_RUN => exec_gates_run(&mut ctx, &step),
        _ => return render_error("unsupported recipe step kind"),
    };
    let recipe_id = ctx.snapshot.recipe_id.clone();
    match result {
        Ok(d) => render_tick_result(loop_id, &recipe_id, d),
        Err(msg) => render_error(&msg),
    }
}

// ─── Generic Dispatchers ─────────────────────────────────────────────────────

fn exec_decide<F>(ctx: &mut TickCtx, step: &RecipeStep, decide: F) -> Result<TickDecision, String>
where
    F: FnOnce(&mut RecipeSnapshot, &RecipeStep, &str) -> Result<TickDecision, String>,
{
    let d = decide(ctx.snapshot, step, ctx.loop_id)?;
    execute_all_effects(ctx.conn, ctx.executor, &d.effects, ctx.snapshot)?;
    Ok(d)
}

fn exec_decide_ok<F>(
    ctx: &mut TickCtx,
    step: &RecipeStep,
    decide: F,
) -> Result<TickDecision, String>
where
    F: FnOnce(&mut RecipeSnapshot, &RecipeStep, &str) -> TickDecision,
{
    let d = decide(ctx.snapshot, step, ctx.loop_id);
    execute_all_effects(ctx.conn, ctx.executor, &d.effects, ctx.snapshot)?;
    Ok(d)
}

// ─── Complex Step Executors ──────────────────────────────────────────────────

fn exec_session_create(ctx: &mut TickCtx, step: &RecipeStep) -> Result<TickDecision, String> {
    let role_id = step.role.as_deref().unwrap_or("default");
    let existing = ctx
        .queries
        .list_loop_sessions(ctx.conn, ctx.loop_id)
        .unwrap_or_default();
    let loop_run = ctx
        .queries
        .get_loop(ctx.conn, ctx.loop_id)?
        .ok_or_else(|| "loop not found".to_string())?;
    let maker_branch = if step.role.as_deref() != Some("maker") {
        ctx.snapshot
            .runtime
            .created_session_ids
            .get("maker")
            .and_then(|ids| ids.last())
            .and_then(|sid| {
                ctx.queries
                    .get_session(ctx.conn, sid)
                    .ok()
                    .flatten()
                    .map(|s| s.branch)
            })
    } else {
        None
    };

    let mut decision = loop_decision::decide_session_create(
        ctx.snapshot,
        step,
        ctx.loop_id,
        existing.len() as u32,
        &loop_run,
        maker_branch,
    )?;
    for effect in &decision.effects {
        match effect {
            Effect::CreateSession { .. } => match ctx.executor.create_session(ctx.conn, effect) {
                Ok(created) => {
                    let provider = ctx
                        .snapshot
                        .roles
                        .get(role_id)
                        .map(|r| r.provider.clone())
                        .unwrap_or_else(|| "default".into());
                    let finalize = loop_decision::finalize_session_create(
                        ctx.snapshot,
                        step,
                        ctx.loop_id,
                        &created.id,
                        role_id,
                        &provider,
                    );
                    decision.extra = vec![field(
                        "created_session",
                        Value::Object(vec![
                            field("id", str_val(loop_decision::short_id(&created.id))),
                            field("role", str_val(role_id)),
                            field("provider", str_val(&provider)),
                            field("round", str_val(&ctx.snapshot.runtime.round.to_string())),
                        ]),
                    )];
                    for fe in &finalize {
                        execute_effect(ctx.conn, ctx.executor, fe, ctx.snapshot)?;
                    }
                }
                Err(e) => {
                    let _ = ctx.executor.append_event(
                        ctx.conn,
                        ctx.loop_id,
                        "recipe_step_failed",
                        &serde_json::json!({"step_id": step.id, "kind": step.kind, "error": e}),
                    );
                    return Err(format!("session.create failed: {e}"));
                }
            },
            _ => {
                execute_effect(ctx.conn, ctx.executor, effect, ctx.snapshot)?;
            }
        }
    }
    Ok(decision)
}

fn exec_handoff_wait(ctx: &mut TickCtx, step: &RecipeStep) -> Result<TickDecision, String> {
    let role_id = step.from.as_deref().unwrap_or("default");
    let sids = ctx
        .snapshot
        .runtime
        .created_session_ids
        .get(role_id)
        .cloned()
        .unwrap_or_default();
    let after_ts = ctx.snapshot.runtime.last_handoff_consumed_at.as_deref();
    let found = ctx
        .queries
        .find_handoff(ctx.conn, ctx.loop_id, &sids, after_ts)?;
    let decision = match found {
        None => {
            loop_decision::decide_handoff_wait_waiting(ctx.snapshot, step, ctx.loop_id, role_id)
        }
        Some((sid, status)) => {
            let summary = if status != "completed" {
                ctx.queries
                    .extract_handoff_summary(ctx.conn, ctx.loop_id, &sid)
                    .ok()
            } else {
                None
            };
            loop_decision::decide_handoff_wait_consumed(
                ctx.snapshot,
                step,
                ctx.loop_id,
                &sid,
                &status,
                summary,
            )
        }
    };
    execute_all_effects(ctx.conn, ctx.executor, &decision.effects, ctx.snapshot)?;
    Ok(decision)
}

fn exec_gates_run(ctx: &mut TickCtx, step: &RecipeStep) -> Result<TickDecision, String> {
    loop_decision::decide_gates_run_preflight(step)?;
    ctx.executor
        .transition_loop(ctx.conn, ctx.loop_id, LoopTrigger::GatesStarted)?;

    let session_id = resolve_gate_session(ctx.snapshot, step)?;
    let loop_run = ctx
        .queries
        .get_loop(ctx.conn, ctx.loop_id)?
        .ok_or("loop not found".to_string())?;
    let project = ctx
        .queries
        .get_project(ctx.conn, &loop_run.project_id)?
        .ok_or_else(|| format!("project not found: {}", loop_run.project_id))?;
    let session = ctx
        .queries
        .get_session(ctx.conn, &session_id)?
        .ok_or_else(|| format!("session not found: {session_id}"))?;

    let (status, name, output, path) = run_gates(
        ctx,
        step,
        &session_id,
        &project.path,
        &session.worktree_path,
    );
    ctx.executor
        .transition_loop(ctx.conn, ctx.loop_id, LoopTrigger::GatesCompleted)?;

    let decision = loop_decision::decide_gates_run_result(
        ctx.snapshot,
        step,
        ctx.loop_id,
        status,
        &name,
        output.as_deref(),
        path.as_deref(),
    );
    execute_all_effects(ctx.conn, ctx.executor, &decision.effects, ctx.snapshot)?;
    Ok(decision)
}

fn resolve_gate_session(snapshot: &RecipeSnapshot, step: &RecipeStep) -> Result<String, String> {
    if let Some(ref role) = step.role {
        snapshot
            .runtime
            .created_session_ids
            .get(role)
            .and_then(|ids| ids.last())
            .cloned()
            .ok_or_else(|| format!("step '{}': no sessions found for role '{}'", step.id, role))
    } else {
        snapshot
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
            })
    }
}

fn run_gates(
    ctx: &TickCtx,
    step: &RecipeStep,
    session_id: &str,
    project_path: &str,
    worktree_path: &Option<String>,
) -> (&'static str, String, Option<String>, Option<String>) {
    let (mut status, mut name, mut output, mut path): (
        &str,
        String,
        Option<String>,
        Option<String>,
    ) = ("pass", String::new(), None, None);
    for gate in &step.gates {
        let cmd = loop_decision::render_prompt(&gate.command, ctx.snapshot, ctx.loop_id);
        let effect = Effect::RunGate {
            loop_id: ctx.loop_id.into(),
            session_id: session_id.into(),
            gate_name: gate.name.clone(),
            command: cmd,
            project_path: project_path.into(),
            worktree_path: worktree_path.clone(),
        };
        match ctx.executor.run_gate(ctx.conn, &effect) {
            Ok(r) if r.status != GateStatus::Pass => {
                status = if r.status == GateStatus::Error {
                    "error"
                } else {
                    "fail"
                };
                name = gate.name.clone();
                output = r.output;
                path = r.output_path;
                break;
            }
            Err(e) => {
                let _ = ctx.executor.append_event(ctx.conn, ctx.loop_id, "recipe_step_failed",
                    &serde_json::json!({"step_id": step.id, "gate": gate.name, "error": e.to_string()}));
                status = "error";
                name = gate.name.clone();
                break;
            }
            _ => {}
        }
    }
    (status, name, output, path)
}

// ─── Effect Execution ────────────────────────────────────────────────────────

fn execute_effect(
    conn: &rusqlite::Connection,
    executor: &dyn EffectExecutor,
    effect: &Effect,
    snapshot: &RecipeSnapshot,
) -> Result<(), String> {
    match effect {
        Effect::TransitionLoop { loop_id, trigger } => {
            executor.transition_loop(conn, loop_id, trigger.clone())
        }
        Effect::AppendEvent {
            loop_id,
            kind,
            payload,
        } => executor.append_event(conn, loop_id, kind, payload),
        Effect::SaveSnapshot { loop_id } => executor.save_snapshot(conn, loop_id, snapshot),
        Effect::UpdateCurrentRound { loop_id, round } => {
            executor.update_current_round(conn, loop_id, *round)
        }
        Effect::LinkSession {
            loop_id,
            session_id,
            role,
            round,
            provider,
        } => executor.link_session(conn, loop_id, session_id, role, *round, provider.as_deref()),
        Effect::SendPrompt {
            session_id,
            prompt_text,
        } => executor.send_prompt(conn, session_id, prompt_text),
        Effect::CreateSession { .. } | Effect::RunGate { .. } => Ok(()),
    }
}

fn execute_all_effects(
    conn: &rusqlite::Connection,
    executor: &dyn EffectExecutor,
    effects: &[Effect],
    snapshot: &RecipeSnapshot,
) -> Result<(), String> {
    for e in effects {
        execute_effect(conn, executor, e, snapshot)?;
    }
    Ok(())
}

// ─── Auto-advance ────────────────────────────────────────────────────────────

/// Auto-advance a loop's recipe through immediately-executable steps.
pub fn auto_advance_with_arc(
    conn_arc: &std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    check_human_wait_before_tick: bool,
) {
    for _ in 0..10 {
        if check_human_wait_before_tick && loop_decision::is_human_wait_step(snapshot) {
            break;
        }
        let conn = match conn_arc.lock() {
            Ok(c) => c,
            Err(_) => break,
        };
        let before = snapshot.runtime.current_step.clone();
        let (_, code) = tick_recipe(&conn, loop_id, snapshot);
        let _ = LoopService::update_policy_json(
            &conn,
            loop_id,
            &serde_json::to_value(&*snapshot).unwrap_or_default(),
        );
        if code != 0 {
            break;
        }
        if snapshot.runtime.current_step == before {
            drop(conn);
            break;
        }
        let stop = LoopService::get_loop(&conn, loop_id)
            .ok()
            .flatten()
            .map(|r| r.status.is_executor_terminal() || r.status.is_intervention_required())
            .unwrap_or(false);
        drop(conn);
        if stop {
            break;
        }
        if !check_human_wait_before_tick && loop_decision::is_human_wait_step(snapshot) {
            break;
        }
    }
}

/// Simpler variant that takes a `&Connection` directly.
pub fn auto_advance(
    conn: &rusqlite::Connection,
    loop_id: &str,
    snapshot: &mut RecipeSnapshot,
    check_human_wait_before_tick: bool,
) {
    for _ in 0..10 {
        if check_human_wait_before_tick && loop_decision::is_human_wait_step(snapshot) {
            break;
        }
        let before = snapshot.runtime.current_step.clone();
        let (_, code) = tick_recipe(conn, loop_id, snapshot);
        let _ = LoopService::update_policy_json(
            conn,
            loop_id,
            &serde_json::to_value(&*snapshot).unwrap_or_default(),
        );
        if code != 0 {
            break;
        }
        if snapshot.runtime.current_step == before {
            break;
        }
        if let Ok(Some(r)) = LoopService::get_loop(conn, loop_id) {
            if r.status.is_executor_terminal() || r.status.is_intervention_required() {
                break;
            }
        }
        if !check_human_wait_before_tick && loop_decision::is_human_wait_step(snapshot) {
            break;
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_effects::{CreatedSession, GateResult};
    use planeai_core::loop_recipe::{RecipeKnowledge, RecipeTools};
    use planeai_core::loop_recipe_service::{RecipeRuntime, SnapshotPolicy};
    use planeai_core::loop_run::{LoopRun, LoopSession, LoopStatus, LoopStrategy};
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    // ─── Mock implementations ────────────────────────────────────────────────

    #[derive(Default)]
    struct MockExecutor {
        effects: RefCell<Vec<String>>,
    }

    impl EffectExecutor for MockExecutor {
        fn create_session(
            &self,
            _conn: &rusqlite::Connection,
            _effect: &Effect,
        ) -> Result<CreatedSession, String> {
            self.effects.borrow_mut().push("create_session".into());
            Ok(CreatedSession {
                id: "mock-session-1".into(),
                branch: "mock-branch".into(),
                worktree_path: None,
            })
        }
        fn send_prompt(
            &self,
            _conn: &rusqlite::Connection,
            _sid: &str,
            _p: &str,
        ) -> Result<(), String> {
            self.effects.borrow_mut().push("send_prompt".into());
            Ok(())
        }
        fn run_gate(
            &self,
            _conn: &rusqlite::Connection,
            _effect: &Effect,
        ) -> Result<GateResult, String> {
            self.effects.borrow_mut().push("run_gate".into());
            Ok(GateResult {
                status: GateStatus::Pass,
                output: None,
                output_path: None,
            })
        }
        fn transition_loop(
            &self,
            _conn: &rusqlite::Connection,
            _lid: &str,
            _t: LoopTrigger,
        ) -> Result<(), String> {
            self.effects.borrow_mut().push("transition_loop".into());
            Ok(())
        }
        fn append_event(
            &self,
            _conn: &rusqlite::Connection,
            _lid: &str,
            _k: &str,
            _p: &serde_json::Value,
        ) -> Result<(), String> {
            self.effects.borrow_mut().push("append_event".into());
            Ok(())
        }
        fn link_session(
            &self,
            _conn: &rusqlite::Connection,
            _lid: &str,
            _sid: &str,
            _r: &str,
            _rnd: i64,
            _p: Option<&str>,
        ) -> Result<(), String> {
            self.effects.borrow_mut().push("link_session".into());
            Ok(())
        }
        fn save_snapshot(
            &self,
            _conn: &rusqlite::Connection,
            _lid: &str,
            _s: &RecipeSnapshot,
        ) -> Result<(), String> {
            self.effects.borrow_mut().push("save_snapshot".into());
            Ok(())
        }
        fn update_current_round(
            &self,
            _conn: &rusqlite::Connection,
            _lid: &str,
            _r: i64,
        ) -> Result<(), String> {
            self.effects
                .borrow_mut()
                .push("update_current_round".into());
            Ok(())
        }
    }

    struct MockQueries {
        loop_run: Option<LoopRun>,
    }

    impl MockQueries {
        fn running() -> Self {
            Self {
                loop_run: Some(LoopRun {
                    id: "loop-1234".into(),
                    project_id: "proj-1".into(),
                    task_key: None,
                    created_by_session_id: None,
                    strategy: LoopStrategy::new("recipe"),
                    goal: "test".into(),
                    status: LoopStatus::Running,
                    current_round: 1,
                    max_rounds: 3,
                    created_at: "2024-01-01T00:00:00Z".into(),
                    updated_at: "2024-01-01T00:00:00Z".into(),
                    executor_finished_at: None,
                    policy_json: None,
                    budget_json: None,
                }),
            }
        }
    }

    impl LoopQueries for MockQueries {
        fn get_loop(
            &self,
            _conn: &rusqlite::Connection,
            _lid: &str,
        ) -> Result<Option<LoopRun>, String> {
            Ok(self.loop_run.clone())
        }
        fn list_loop_sessions(
            &self,
            _conn: &rusqlite::Connection,
            _lid: &str,
        ) -> Result<Vec<LoopSession>, String> {
            Ok(vec![])
        }
        fn find_handoff(
            &self,
            _conn: &rusqlite::Connection,
            _lid: &str,
            _sids: &[String],
            _ts: Option<&str>,
        ) -> Result<Option<(String, String)>, String> {
            Ok(None)
        }
        fn get_session(
            &self,
            _conn: &rusqlite::Connection,
            _sid: &str,
        ) -> Result<Option<crate::db::Session>, String> {
            Ok(None)
        }
        fn get_project(
            &self,
            _conn: &rusqlite::Connection,
            _pid: &str,
        ) -> Result<Option<crate::db::Project>, String> {
            Ok(None)
        }
        fn extract_handoff_summary(
            &self,
            _conn: &rusqlite::Connection,
            _lid: &str,
            _sid: &str,
        ) -> Result<String, String> {
            Ok("mock summary".into())
        }
    }

    fn test_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        planeai_core::loop_service::LoopService::migrate(&conn).unwrap();
        conn
    }

    fn minimal_snapshot(steps: Vec<RecipeStep>) -> RecipeSnapshot {
        let first = steps.first().map(|s| s.id.clone()).unwrap_or_default();
        RecipeSnapshot {
            recipe_schema: "planeai.loop.recipe.v1".into(),
            recipe_id: "test".into(),
            recipe_source: "builtin".into(),
            recipe_path: None,
            inputs: BTreeMap::new(),
            runtime: RecipeRuntime {
                current_step: first,
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

    // ─── Wiring tests ────────────────────────────────────────────────────────

    #[test]
    fn tick_human_wait_executes_transition_effect() {
        let conn = test_db();
        let mut snapshot = minimal_snapshot(vec![
            make_step("wait", "human.wait"),
            make_step("after", "loop.event"),
        ]);
        let executor = MockExecutor::default();
        let queries = MockQueries::running();

        let (output, code) =
            tick_recipe_with_executor(&conn, "loop-1234", &mut snapshot, &executor, &queries);

        assert_eq!(code, 0);
        assert!(output.contains("needs_human"));
        let effects = executor.effects.borrow();
        assert!(
            effects.contains(&"transition_loop".to_string()),
            "should transition to needs_human"
        );
        assert!(
            effects.contains(&"append_event".to_string()),
            "should append event"
        );
        assert!(
            effects.contains(&"save_snapshot".to_string()),
            "should save snapshot"
        );
    }

    #[test]
    fn tick_round_next_executes_update_round_effect() {
        let conn = test_db();
        let mut snapshot = minimal_snapshot(vec![
            make_step("next", "round.next"),
            make_step("after", "loop.event"),
        ]);
        let executor = MockExecutor::default();
        let queries = MockQueries::running();

        let (_, code) =
            tick_recipe_with_executor(&conn, "loop-1234", &mut snapshot, &executor, &queries);

        assert_eq!(code, 0);
        let effects = executor.effects.borrow();
        assert!(effects.contains(&"update_current_round".to_string()));
        assert!(effects.contains(&"save_snapshot".to_string()));
        assert_eq!(snapshot.runtime.round, 2);
    }

    #[test]
    fn tick_terminal_loop_returns_error() {
        let conn = test_db();
        let mut snapshot = minimal_snapshot(vec![make_step("s1", "loop.event")]);
        let executor = MockExecutor::default();
        let queries = MockQueries {
            loop_run: Some(LoopRun {
                id: "loop-1234".into(),
                project_id: "p".into(),
                task_key: None,
                created_by_session_id: None,
                strategy: LoopStrategy::new("recipe"),
                goal: "t".into(),
                status: LoopStatus::Failed,
                current_round: 1,
                max_rounds: 3,
                created_at: "t".into(),
                updated_at: "t".into(),
                executor_finished_at: None,
                policy_json: None,
                budget_json: None,
            }),
        };

        let (output, code) =
            tick_recipe_with_executor(&conn, "loop-1234", &mut snapshot, &executor, &queries);

        assert_eq!(code, 1);
        assert!(output.contains("terminal"));
        assert!(
            executor.effects.borrow().is_empty(),
            "no effects should execute for terminal loop"
        );
    }

    #[test]
    fn tick_max_ticks_fires_transition_effect() {
        let conn = test_db();
        let mut snapshot = minimal_snapshot(vec![make_step("s1", "loop.event")]);
        snapshot.policy.max_ticks = 5;
        snapshot.runtime.tick_count = 5; // at limit
        let executor = MockExecutor::default();
        let queries = MockQueries::running();

        let (output, code) =
            tick_recipe_with_executor(&conn, "loop-1234", &mut snapshot, &executor, &queries);

        assert_eq!(code, 1);
        assert!(output.contains("max_ticks"));
        let effects = executor.effects.borrow();
        assert!(
            effects.contains(&"transition_loop".to_string()),
            "should fire MaxTicksExceeded transition"
        );
    }

    #[test]
    fn tick_loop_event_advances_step_and_saves() {
        let conn = test_db();
        let mut snapshot = minimal_snapshot(vec![
            make_step("ev1", "loop.event"),
            make_step("ev2", "loop.event"),
        ]);
        let executor = MockExecutor::default();
        let queries = MockQueries::running();

        let (_, code) =
            tick_recipe_with_executor(&conn, "loop-1234", &mut snapshot, &executor, &queries);

        assert_eq!(code, 0);
        assert_eq!(snapshot.runtime.current_step, "ev2");
        assert_eq!(snapshot.runtime.tick_count, 1);
        let effects = executor.effects.borrow();
        assert!(effects.contains(&"append_event".to_string()));
        assert!(effects.contains(&"save_snapshot".to_string()));
    }
}
