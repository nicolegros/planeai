//! Tauri commands for the Loop Runs UI.
//!
//! These expose the `LoopService` and `RecipeService` to the frontend,
//! mirroring what the CLI does but without TOON output formatting.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use planeai_core::loop_recipe_service::RecipeService;
#[cfg(test)]
use planeai_core::loop_run::LoopStatus;
use planeai_core::loop_run::{LoopRun, LoopStrategy, LoopTrigger};
use planeai_core::loop_service::{CreateLoopParams, LoopService};

use crate::state::DbState;

use super::blocking;

// ─── Response types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRunSummary {
    pub id: String,
    pub project_id: String,
    pub task_key: Option<String>,
    pub strategy: String,
    pub goal: String,
    pub status: String,
    pub current_round: i64,
    pub max_rounds: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl From<LoopRun> for LoopRunSummary {
    fn from(r: LoopRun) -> Self {
        Self {
            id: r.id,
            project_id: r.project_id,
            task_key: r.task_key,
            strategy: r.strategy.as_str().to_string(),
            goal: r.goal,
            status: r.status.as_str().to_string(),
            current_round: r.current_round,
            max_rounds: r.max_rounds,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopSessionItem {
    pub session_id: String,
    pub role: String,
    pub round: i64,
    pub provider: Option<String>,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopEventItem {
    pub id: i64,
    pub ts: String,
    pub kind: String,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopArtifactItem {
    pub id: String,
    pub session_id: Option<String>,
    pub kind: String,
    pub path: Option<String>,
    pub content_json: Option<serde_json::Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierRunItem {
    pub id: String,
    pub session_id: Option<String>,
    pub verifier_type: String,
    pub name: String,
    pub command: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub output_path: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRunDetail {
    pub run: LoopRunSummary,
    pub sessions: Vec<LoopSessionItem>,
    pub events: Vec<LoopEventItem>,
    pub artifacts: Vec<LoopArtifactItem>,
    pub verifier_runs: Vec<VerifierRunItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source: String,
}

// ─── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_loop_runs(
    db_state: State<'_, DbState>,
    project_id: String,
) -> Result<Vec<LoopRunSummary>, String> {
    tracing::info!(project_id = %project_id, "list_loop_runs");
    let conn = db_state.0.clone();
    blocking(move || {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        LoopService::list_loops(&conn, &project_id)
            .map(|runs| runs.into_iter().map(LoopRunSummary::from).collect())
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn get_loop_run_detail(
    db_state: State<'_, DbState>,
    loop_id: String,
) -> Result<LoopRunDetail, String> {
    tracing::info!(loop_id = %loop_id, "get_loop_run_detail");
    let conn = db_state.0.clone();
    blocking(move || {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        let run = LoopService::get_loop(&conn, &loop_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("loop not found: {loop_id}"))?;

        let sessions = LoopService::list_loop_sessions(&conn, &loop_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|s| LoopSessionItem {
                session_id: s.session_id,
                role: s.role,
                round: s.round,
                provider: s.provider,
                status: s.status,
                created_at: s.created_at,
            })
            .collect();

        let events = LoopService::list_loop_events(&conn, &loop_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|e| LoopEventItem {
                id: e.id,
                ts: e.ts,
                kind: e.kind,
                payload_json: e.payload_json,
            })
            .collect();

        let artifacts = list_loop_artifacts_query(&conn, &loop_id)?;
        let verifier_runs = list_verifier_runs_query(&conn, &loop_id)?;

        Ok(LoopRunDetail {
            run: LoopRunSummary::from(run),
            sessions,
            events,
            artifacts,
            verifier_runs,
        })
    })
    .await
}

#[tauri::command]
pub async fn list_loop_recipes(
    db_state: State<'_, DbState>,
    project_id: String,
) -> Result<Vec<RecipeSummary>, String> {
    let conn_arc = db_state.0.clone();
    blocking(move || {
        let conn = conn_arc.lock().map_err(|e| e.to_string())?;
        // Resolve project path for recipe discovery
        let project = crate::db::get_project(&conn, &project_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("project not found: {project_id}"))?;
        drop(conn);

        let project_root = std::path::Path::new(&project.path);
        let recipes = RecipeService::discover_all(Some(project_root));
        Ok(recipes
            .into_iter()
            .filter(|dr| dr.recipe.trigger.is_v1_executable())
            .map(|dr| RecipeSummary {
                id: dr.recipe.id,
                name: dr.recipe.name,
                description: dr.recipe.description,
                source: dr.source.as_str().to_string(),
            })
            .collect())
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn create_loop_run(
    db_state: State<'_, DbState>,
    app_handle: AppHandle,
    project_id: String,
    goal: String,
    recipe_id: String,
    task_key: Option<String>,
    max_rounds: Option<i64>,
    base_branch: Option<String>,
    start: bool,
) -> Result<LoopRunSummary, String> {
    tracing::info!(project_id = %project_id, recipe_id = %recipe_id, start, "create_loop_run");
    let conn_arc = db_state.0.clone();
    let result = blocking(move || {
        let conn = conn_arc.lock().map_err(|e| e.to_string())?;

        // Resolve project path for recipe lookup
        let project = crate::db::get_project(&conn, &project_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("project not found: {project_id}"))?;

        let project_root = std::path::Path::new(&project.path);

        // Resolve recipe
        let discovered = RecipeService::resolve(&recipe_id, Some(project_root))
            .map_err(|e| format!("recipe error: {e}"))?;

        // Validate recipe
        let validation = RecipeService::validate(&discovered.recipe, Some(project_root));
        if !validation.valid {
            return Err(format!(
                "recipe '{}' failed validation: {}",
                recipe_id,
                validation.errors.join("; ")
            ));
        }

        // Build snapshot for policy_json
        let mut inputs = std::collections::BTreeMap::new();
        inputs.insert("goal".to_string(), goal.clone());
        if let Some(ref key) = task_key {
            inputs.insert("task_key".to_string(), key.clone());
        }
        if let Some(ref branch) = base_branch {
            if !branch.is_empty() {
                inputs.insert("base_branch".to_string(), branch.clone());
            }
        }
        let snapshot = RecipeService::create_snapshot(&discovered, inputs);
        let resolved_max_rounds = max_rounds.unwrap_or(snapshot.policy.max_rounds as i64);
        let policy_json = serde_json::to_value(&snapshot).ok();
        let policy_json_for_tick = policy_json.clone();

        // Create the loop run
        let params = CreateLoopParams {
            project_id,
            task_key,
            created_by_session_id: None, // UI-initiated, no parent session
            strategy: LoopStrategy::new(&discovered.recipe.id),
            goal,
            max_rounds: resolved_max_rounds,
            policy_json,
            budget_json: None,
        };
        let run = LoopService::create_loop(&conn, params).map_err(|e| e.to_string())?;

        // Optionally start it
        if start {
            LoopService::transition_loop(&conn, &run.id, LoopTrigger::Start)
                .map_err(|e| e.to_string())?;
            LoopService::append_loop_event(
                &conn,
                &run.id,
                "loop_started",
                &serde_json::json!({"source": "ui"}),
            )
            .map_err(|e| e.to_string())?;

            // Auto-tick immediately so the recipe begins executing
            if let Some(ref pj) = policy_json_for_tick {
                if let Ok(mut snapshot) = serde_json::from_value::<
                    planeai_core::loop_recipe_service::RecipeSnapshot,
                >(pj.clone())
                {
                    drop(conn);
                    planeai::recipe_tick::auto_advance_with_arc(
                        &conn_arc,
                        &run.id,
                        &mut snapshot,
                        false,
                    );
                    let conn = conn_arc.lock().map_err(|e| e.to_string())?;
                    let updated = LoopService::get_loop(&conn, &run.id)
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| "loop disappeared after creation".to_string())?;
                    return Ok(LoopRunSummary::from(updated));
                }
            }

            // Return with updated status
            let updated = LoopService::get_loop(&conn, &run.id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "loop disappeared after creation".to_string())?;
            return Ok(LoopRunSummary::from(updated));
        }

        Ok(LoopRunSummary::from(run))
    })
    .await?;

    let _ = app_handle.emit("loop-state-changed", ());
    Ok(result)
}

#[tauri::command]
pub async fn tick_loop(
    db_state: State<'_, DbState>,
    app_handle: AppHandle,
    loop_id: String,
) -> Result<(), String> {
    tracing::info!(loop_id = %loop_id, "tick_loop");
    let conn_arc = db_state.0.clone();
    blocking(move || {
        let conn = conn_arc.lock().map_err(|e| e.to_string())?;

        // Verify loop exists and is in a tickable state
        let run = LoopService::get_loop(&conn, &loop_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("loop not found: {loop_id}"))?;

        if run.status.is_executor_terminal() {
            return Err(format!(
                "loop {} is in terminal status '{}' — cannot tick",
                &loop_id[..8.min(loop_id.len())],
                run.status.as_str()
            ));
        }

        if run.status.is_intervention_required() {
            return Err(format!(
                "loop {} requires intervention (status: '{}') — cannot tick",
                &loop_id[..8.min(loop_id.len())],
                run.status.as_str()
            ));
        }

        // If there's a recipe snapshot, execute ticks until a waiting/terminal state
        if let Some(policy_json) = run.policy_json {
            if let Ok(mut snapshot) = serde_json::from_value::<
                planeai_core::loop_recipe_service::RecipeSnapshot,
            >(policy_json.clone())
            {
                drop(conn);
                planeai::recipe_tick::auto_advance_with_arc(
                    &conn_arc,
                    &loop_id,
                    &mut snapshot,
                    false,
                );
            }
        } else {
            // Non-recipe loop: just increment round
            let new_round = run.current_round + 1;
            conn.execute(
                "UPDATE loop_runs SET current_round = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![new_round, chrono::Utc::now().to_rfc3339(), loop_id],
            )
            .map_err(|e| e.to_string())?;
        }

        Ok(())
    })
    .await?;

    let _ = app_handle.emit("loop-state-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn start_loop(
    db_state: State<'_, DbState>,
    app_handle: AppHandle,
    loop_id: String,
) -> Result<(), String> {
    tracing::info!(loop_id = %loop_id, "start_loop");
    let conn_arc = db_state.0.clone();
    blocking(move || {
        let conn = conn_arc.lock().map_err(|e| e.to_string())?;

        let run = LoopService::get_loop(&conn, &loop_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("loop not found: {loop_id}"))?;

        LoopService::transition_loop(&conn, &loop_id, LoopTrigger::Start)
            .map_err(|e| e.to_string())?;

        LoopService::append_loop_event(
            &conn,
            &loop_id,
            "loop_started",
            &serde_json::json!({"source": "ui"}),
        )
        .map_err(|e| e.to_string())?;

        // Auto-tick immediately after starting so the recipe begins executing
        if let Some(ref policy_json) = run.policy_json {
            if let Ok(mut snapshot) = serde_json::from_value::<
                planeai_core::loop_recipe_service::RecipeSnapshot,
            >(policy_json.clone())
            {
                drop(conn);
                planeai::recipe_tick::auto_advance_with_arc(
                    &conn_arc,
                    &loop_id,
                    &mut snapshot,
                    false,
                );
            }
        }

        Ok(())
    })
    .await?;

    let _ = app_handle.emit("loop-state-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn stop_loop(
    db_state: State<'_, DbState>,
    app_handle: AppHandle,
    loop_id: String,
) -> Result<(), String> {
    tracing::info!(loop_id = %loop_id, "stop_loop");
    let conn_arc = db_state.0.clone();
    blocking(move || {
        let conn = conn_arc.lock().map_err(|e| e.to_string())?;

        LoopService::transition_loop(&conn, &loop_id, LoopTrigger::Cancel)
            .map_err(|e| e.to_string())?;

        LoopService::append_loop_event(
            &conn,
            &loop_id,
            "loop_stopped",
            &serde_json::json!({"source": "ui"}),
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    })
    .await?;

    let _ = app_handle.emit("loop-state-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn delete_loop(
    db_state: State<'_, DbState>,
    app_handle: AppHandle,
    loop_id: String,
) -> Result<Vec<String>, String> {
    tracing::info!(loop_id = %loop_id, "delete_loop");
    let conn_arc = db_state.0.clone();
    let session_ids = blocking(move || {
        let conn = conn_arc.lock().map_err(|e| e.to_string())?;
        LoopService::delete_loop(&conn, &loop_id).map_err(|e| e.to_string())
    })
    .await?;

    let _ = app_handle.emit("loop-state-changed", ());
    Ok(session_ids)
}

// ─── Query helpers (not exposed as commands) ─────────────────────────────────

fn list_loop_artifacts_query(
    conn: &rusqlite::Connection,
    loop_id: &str,
) -> Result<Vec<LoopArtifactItem>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, kind, path, content_json, created_at
             FROM loop_artifacts WHERE loop_id = ?1 ORDER BY created_at",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![loop_id], |row| {
            let content_str: Option<String> = row.get(4)?;
            Ok(LoopArtifactItem {
                id: row.get(0)?,
                session_id: row.get(1)?,
                kind: row.get(2)?,
                path: row.get(3)?,
                content_json: content_str.and_then(|s| serde_json::from_str(&s).ok()),
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn list_verifier_runs_query(
    conn: &rusqlite::Connection,
    loop_id: &str,
) -> Result<Vec<VerifierRunItem>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, verifier_type, name, command, status, exit_code, output_path, created_at, started_at, finished_at
             FROM verifier_runs WHERE loop_id = ?1 ORDER BY created_at",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![loop_id], |row| {
            Ok(VerifierRunItem {
                id: row.get(0)?,
                session_id: row.get(1)?,
                verifier_type: row.get(2)?,
                name: row.get(3)?,
                command: row.get(4)?,
                status: row.get(5)?,
                exit_code: row.get(6)?,
                output_path: row.get(7)?,
                created_at: row.get(8)?,
                started_at: row.get(9)?,
                finished_at: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use planeai_core::loop_service::{AddLoopSessionParams, AddVerifierRunParams};
    use planeai_core::services::open_db_at;

    fn test_db() -> rusqlite::Connection {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let conn = open_db_at(&path).unwrap();
        std::mem::forget(dir);
        conn
    }

    fn seed_project(conn: &rusqlite::Connection) -> String {
        let id = "proj-test-1".to_string();
        conn.execute(
            "INSERT INTO projects (id, name, path) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, "Test Project", "/tmp/test-project"],
        )
        .unwrap();
        id
    }

    #[test]
    fn list_loop_runs_returns_loops_for_project() {
        let conn = test_db();
        let project_id = seed_project(&conn);

        // Create two loops in the project
        LoopService::create_loop(
            &conn,
            CreateLoopParams {
                project_id: project_id.clone(),
                task_key: Some("PLA-1".into()),
                created_by_session_id: None,
                strategy: LoopStrategy::new("maker-verifier"),
                goal: "First loop".into(),
                max_rounds: 3,
                policy_json: None,
                budget_json: None,
            },
        )
        .unwrap();
        LoopService::create_loop(
            &conn,
            CreateLoopParams {
                project_id: project_id.clone(),
                task_key: None,
                created_by_session_id: None,
                strategy: LoopStrategy::new("plan-implement-review"),
                goal: "Second loop".into(),
                max_rounds: 5,
                policy_json: None,
                budget_json: None,
            },
        )
        .unwrap();

        // Create a loop in a different project (should not appear)
        LoopService::create_loop(
            &conn,
            CreateLoopParams {
                project_id: "other-project".into(),
                task_key: None,
                created_by_session_id: None,
                strategy: LoopStrategy::new("maker-verifier"),
                goal: "Other project loop".into(),
                max_rounds: 2,
                policy_json: None,
                budget_json: None,
            },
        )
        .unwrap();

        let runs = LoopService::list_loops(&conn, &project_id).unwrap();
        let summaries: Vec<LoopRunSummary> = runs.into_iter().map(LoopRunSummary::from).collect();

        assert_eq!(summaries.len(), 2);
        // Ordered by created_at DESC — second loop first
        assert_eq!(summaries[0].goal, "Second loop");
        assert_eq!(summaries[0].strategy, "plan-implement-review");
        assert_eq!(summaries[0].max_rounds, 5);
        assert_eq!(summaries[1].goal, "First loop");
        assert_eq!(summaries[1].task_key, Some("PLA-1".to_string()));
    }

    #[test]
    fn get_loop_run_detail_returns_full_loop_with_children() {
        let conn = test_db();
        let project_id = seed_project(&conn);

        let run = LoopService::create_loop(
            &conn,
            CreateLoopParams {
                project_id: project_id.clone(),
                task_key: Some("PLA-42".into()),
                created_by_session_id: None,
                strategy: LoopStrategy::new("maker-verifier"),
                goal: "Fix auth bug".into(),
                max_rounds: 3,
                policy_json: None,
                budget_json: None,
            },
        )
        .unwrap();

        // Add a session
        LoopService::add_loop_session(
            &conn,
            AddLoopSessionParams {
                loop_id: run.id.clone(),
                session_id: "sess-maker-1".into(),
                role: "maker".into(),
                round: 1,
                provider: Some("claude".into()),
                status: "running".into(),
            },
        )
        .unwrap();

        // Add an event
        LoopService::append_loop_event(
            &conn,
            &run.id,
            "session_created",
            &serde_json::json!({"session_id": "sess-maker-1", "role": "maker"}),
        )
        .unwrap();

        // Add a verifier run
        LoopService::add_verifier_run(
            &conn,
            AddVerifierRunParams {
                loop_id: run.id.clone(),
                session_id: Some("sess-maker-1".into()),
                verifier_type: "command".into(),
                name: "tests".into(),
                command: "cargo test".into(),
            },
        )
        .unwrap();

        // Query detail
        let detail_run = LoopService::get_loop(&conn, &run.id).unwrap().unwrap();
        let sessions = LoopService::list_loop_sessions(&conn, &run.id).unwrap();
        let events = LoopService::list_loop_events(&conn, &run.id).unwrap();
        let artifacts = list_loop_artifacts_query(&conn, &run.id).unwrap();
        let verifier_runs = list_verifier_runs_query(&conn, &run.id).unwrap();

        let detail = LoopRunDetail {
            run: LoopRunSummary::from(detail_run),
            sessions: sessions
                .into_iter()
                .map(|s| LoopSessionItem {
                    session_id: s.session_id,
                    role: s.role,
                    round: s.round,
                    provider: s.provider,
                    status: s.status,
                    created_at: s.created_at,
                })
                .collect(),
            events: events
                .into_iter()
                .map(|e| LoopEventItem {
                    id: e.id,
                    ts: e.ts,
                    kind: e.kind,
                    payload_json: e.payload_json,
                })
                .collect(),
            artifacts,
            verifier_runs,
        };

        assert_eq!(detail.run.goal, "Fix auth bug");
        assert_eq!(detail.run.task_key, Some("PLA-42".to_string()));
        assert_eq!(detail.sessions.len(), 1);
        assert_eq!(detail.sessions[0].role, "maker");
        assert_eq!(detail.sessions[0].provider, Some("claude".to_string()));
        assert_eq!(detail.events.len(), 1);
        assert_eq!(detail.events[0].kind, "session_created");
        assert_eq!(detail.verifier_runs.len(), 1);
        assert_eq!(detail.verifier_runs[0].name, "tests");
        assert_eq!(detail.verifier_runs[0].command, "cargo test");
    }

    #[test]
    fn stop_loop_transitions_to_cancelled() {
        let conn = test_db();
        let project_id = seed_project(&conn);

        let run = LoopService::create_loop(
            &conn,
            CreateLoopParams {
                project_id,
                task_key: None,
                created_by_session_id: None,
                strategy: LoopStrategy::new("maker-verifier"),
                goal: "Loop to stop".into(),
                max_rounds: 3,
                policy_json: None,
                budget_json: None,
            },
        )
        .unwrap();

        // Start it
        LoopService::transition_loop(&conn, &run.id, LoopTrigger::Start).unwrap();

        // Stop it
        LoopService::transition_loop(&conn, &run.id, LoopTrigger::Cancel).unwrap();
        LoopService::append_loop_event(
            &conn,
            &run.id,
            "loop_stopped",
            &serde_json::json!({"source": "ui"}),
        )
        .unwrap();

        let updated = LoopService::get_loop(&conn, &run.id).unwrap().unwrap();
        assert_eq!(updated.status, LoopStatus::Cancelled);

        let events = LoopService::list_loop_events(&conn, &run.id).unwrap();
        assert!(events.iter().any(|e| e.kind == "loop_stopped"));
    }
}
