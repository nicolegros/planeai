//! Loop effects — the seam between pure decision logic and side-effectful execution.
//!
//! The [`Effect`] enum describes all side effects that recipe tick decisions can
//! produce. The [`EffectExecutor`] trait abstracts execution so tests can inject
//! a no-op/mock executor while production uses the real one.

use planeai_core::loop_run::LoopTrigger;
use planeai_core::loop_service::LoopService;

// ─── Effect ──────────────────────────────────────────────────────────────────

/// A side effect produced by the decision layer. Executed by an [`EffectExecutor`].
#[derive(Debug, Clone)]
pub enum Effect {
    /// Create a new coding session (tmux + worktree).
    CreateSession {
        role: String,
        provider: Option<String>,
        branch: String,
        new_branch: bool,
        base_branch: Option<String>,
        worktree: bool,
        auto_approve: bool,
        project_id: String,
        loop_id: String,
        task_key: Option<String>,
        parent_session_id: Option<String>,
        session_name: String,
        prompt: Option<String>,
    },
    /// Send a prompt to an existing session via IPC.
    SendPrompt {
        session_id: String,
        prompt_text: String,
    },
    /// Run verifier gate commands.
    RunGate {
        loop_id: String,
        session_id: String,
        gate_name: String,
        command: String,
        project_path: String,
        worktree_path: Option<String>,
    },
    /// Transition the loop status via the state machine.
    TransitionLoop {
        loop_id: String,
        trigger: LoopTrigger,
    },
    /// Append a loop event to the audit log.
    AppendEvent {
        loop_id: String,
        kind: String,
        payload: serde_json::Value,
    },
    /// Link a session to the loop_sessions table.
    LinkSession {
        loop_id: String,
        session_id: String,
        role: String,
        round: i64,
        provider: Option<String>,
    },
    /// Persist the updated snapshot to policy_json.
    SaveSnapshot { loop_id: String },
    /// Update loop_runs.current_round in the database.
    UpdateCurrentRound { loop_id: String, round: i64 },
}

// ─── Effect Results ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CreatedSession {
    pub id: String,
    pub branch: String,
    pub worktree_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GateResult {
    pub status: GateStatus,
    pub output: Option<String>,
    pub output_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateStatus {
    Pass,
    Fail,
    Error,
}

impl GateStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Error => "error",
        }
    }
}

// ─── EffectExecutor Trait ────────────────────────────────────────────────────

pub trait EffectExecutor {
    fn create_session(
        &self,
        conn: &rusqlite::Connection,
        effect: &Effect,
    ) -> Result<CreatedSession, String>;
    fn send_prompt(
        &self,
        conn: &rusqlite::Connection,
        session_id: &str,
        prompt: &str,
    ) -> Result<(), String>;
    fn run_gate(&self, conn: &rusqlite::Connection, effect: &Effect) -> Result<GateResult, String>;
    fn transition_loop(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        trigger: LoopTrigger,
    ) -> Result<(), String>;
    fn append_event(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        kind: &str,
        payload: &serde_json::Value,
    ) -> Result<(), String>;
    fn link_session(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        session_id: &str,
        role: &str,
        round: i64,
        provider: Option<&str>,
    ) -> Result<(), String>;
    fn save_snapshot(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        snapshot: &planeai_core::loop_recipe_service::RecipeSnapshot,
    ) -> Result<(), String>;
    fn update_current_round(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        round: i64,
    ) -> Result<(), String>;
    fn get_loop(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
    ) -> Result<Option<planeai_core::loop_run::LoopRun>, String>;
    fn list_loop_sessions(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
    ) -> Result<Vec<planeai_core::loop_run::LoopSession>, String>;
    fn find_handoff(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        session_ids: &[String],
        after_ts: Option<&str>,
    ) -> Result<Option<(String, String)>, String>;
    fn get_session(
        &self,
        conn: &rusqlite::Connection,
        session_id: &str,
    ) -> Result<Option<crate::db::Session>, String>;
    fn get_project(
        &self,
        conn: &rusqlite::Connection,
        project_id: &str,
    ) -> Result<Option<crate::db::Project>, String>;
    fn extract_handoff_summary(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        session_id: &str,
    ) -> Result<String, String>;
}

// ─── Real Executor (production) ──────────────────────────────────────────────

pub struct RealEffectExecutor;

impl EffectExecutor for RealEffectExecutor {
    fn create_session(
        &self,
        conn: &rusqlite::Connection,
        effect: &Effect,
    ) -> Result<CreatedSession, String> {
        let Effect::CreateSession {
            role: _,
            provider,
            branch,
            new_branch,
            base_branch,
            worktree,
            auto_approve,
            project_id,
            loop_id: _,
            task_key,
            parent_session_id,
            session_name,
            prompt,
        } = effect
        else {
            return Err("expected CreateSession effect".into());
        };

        let project = crate::db::get_project(conn, project_id)
            .map_err(|e| format!("failed to resolve project: {e}"))?
            .ok_or_else(|| format!("project not found: {project_id}"))?;

        let opts = crate::cli::SessionCreateOpts {
            project: project.name.clone(),
            branch: branch.clone(),
            name: Some(session_name.clone()),
            new_branch: *new_branch,
            worktree: *worktree,
            base_branch: base_branch.clone(),
            yolo: *auto_approve,
            provider: provider.clone(),
            task_key: task_key.clone(),
            prompt: prompt.clone(),
            parent_session_id: parent_session_id.clone(),
        };

        let session = crate::cli::create_session(conn, opts)
            .map_err(|e| format!("session.create failed: {e}"))?;
        Ok(CreatedSession {
            id: session.id,
            branch: branch.clone(),
            worktree_path: session.worktree_path,
        })
    }

    fn send_prompt(
        &self,
        conn: &rusqlite::Connection,
        session_id: &str,
        prompt: &str,
    ) -> Result<(), String> {
        let ops = crate::session_ops::real_prompt_ops(planeai_paths::notify_socket_path());
        crate::session_ops::send_prompt(conn, session_id, prompt, &ops)
            .map(|_| ())
            .map_err(|e| format!("prompt delivery failed: {e}"))
    }

    fn run_gate(&self, conn: &rusqlite::Connection, effect: &Effect) -> Result<GateResult, String> {
        let Effect::RunGate {
            loop_id,
            session_id,
            gate_name,
            command,
            project_path,
            worktree_path,
        } = effect
        else {
            return Err("expected RunGate effect".into());
        };

        use planeai_core::verifier::{VerifierLimits, VerifyGateRequest};
        let request = VerifyGateRequest {
            loop_id: loop_id.clone(),
            session_id: session_id.clone(),
            name: gate_name.clone(),
            command: command.clone(),
            project_path: project_path.clone(),
            session_worktree_path: worktree_path.clone(),
            limits: VerifierLimits::default(),
        };
        match planeai_core::verifier::run_verifier_gate(conn, request) {
            Ok(result) => {
                let status = match result.status.as_str() {
                    "pass" => GateStatus::Pass,
                    "error" => GateStatus::Error,
                    _ => GateStatus::Fail,
                };
                let output = result
                    .output_path
                    .as_ref()
                    .and_then(|p| std::fs::read_to_string(p).ok());
                Ok(GateResult {
                    status,
                    output,
                    output_path: result.output_path,
                })
            }
            Err(e) => Err(format!("gate execution failed: {e}")),
        }
    }

    fn transition_loop(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        trigger: LoopTrigger,
    ) -> Result<(), String> {
        LoopService::transition_loop(conn, loop_id, trigger)
            .map(|_| ())
            .map_err(|e| format!("failed to transition loop: {e}"))
    }

    fn append_event(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        kind: &str,
        payload: &serde_json::Value,
    ) -> Result<(), String> {
        LoopService::append_loop_event(conn, loop_id, kind, payload)
            .map(|_| ())
            .map_err(|e| format!("failed to append loop event: {e}"))
    }

    fn link_session(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        session_id: &str,
        role: &str,
        round: i64,
        provider: Option<&str>,
    ) -> Result<(), String> {
        LoopService::add_loop_session(
            conn,
            planeai_core::loop_service::AddLoopSessionParams {
                loop_id: loop_id.to_string(),
                session_id: session_id.to_string(),
                role: role.to_string(),
                round,
                provider: provider.map(|s| s.to_string()),
                status: "active".to_string(),
            },
        )
        .map(|_| ())
        .map_err(|e| format!("failed to link session to loop: {e}"))
    }

    fn save_snapshot(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        snapshot: &planeai_core::loop_recipe_service::RecipeSnapshot,
    ) -> Result<(), String> {
        let json_val = serde_json::to_value(snapshot)
            .map_err(|e| format!("failed to serialize snapshot: {e}"))?;
        LoopService::update_policy_json(conn, loop_id, &json_val)
            .map_err(|e| format!("failed to persist snapshot: {e}"))
    }

    fn update_current_round(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        round: i64,
    ) -> Result<(), String> {
        conn.execute(
            "UPDATE loop_runs SET current_round = ?1 WHERE id = ?2",
            rusqlite::params![round, loop_id],
        )
        .map_err(|e| format!("failed to update current_round: {e}"))?;
        Ok(())
    }

    fn get_loop(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
    ) -> Result<Option<planeai_core::loop_run::LoopRun>, String> {
        LoopService::get_loop(conn, loop_id).map_err(|e| format!("failed to load loop: {e}"))
    }

    fn list_loop_sessions(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
    ) -> Result<Vec<planeai_core::loop_run::LoopSession>, String> {
        LoopService::list_loop_sessions(conn, loop_id)
            .map_err(|e| format!("failed to list sessions: {e}"))
    }

    fn find_handoff(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        session_ids: &[String],
        after_ts: Option<&str>,
    ) -> Result<Option<(String, String)>, String> {
        LoopService::find_handoff_for_sessions(conn, loop_id, session_ids, after_ts)
            .map_err(|e| format!("handoff query failed: {e}"))
    }

    fn get_session(
        &self,
        conn: &rusqlite::Connection,
        session_id: &str,
    ) -> Result<Option<crate::db::Session>, String> {
        crate::db::get_session(conn, session_id).map_err(|e| format!("failed to get session: {e}"))
    }

    fn get_project(
        &self,
        conn: &rusqlite::Connection,
        project_id: &str,
    ) -> Result<Option<crate::db::Project>, String> {
        crate::db::get_project(conn, project_id)
            .map_err(|e| format!("failed to resolve project: {e}"))
    }

    fn extract_handoff_summary(
        &self,
        conn: &rusqlite::Connection,
        loop_id: &str,
        session_id: &str,
    ) -> Result<String, String> {
        let content: Option<String> = conn.query_row(
            "SELECT content_json FROM loop_artifacts WHERE loop_id = ?1 AND session_id = ?2 AND kind = 'handoff' ORDER BY created_at DESC, id DESC LIMIT 1",
            rusqlite::params![loop_id, session_id], |row| row.get(0),
        ).map_err(|e| format!("failed to query handoff: {e}"))?;
        let json_str = content.ok_or_else(|| "no content in handoff".to_string())?;
        let val: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| format!("invalid json: {e}"))?;
        Ok(val
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("(no summary provided)")
            .to_string())
    }
}
