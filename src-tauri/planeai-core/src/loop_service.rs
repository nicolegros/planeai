//! Loop service — persistence layer for durable loop runs.
//!
//! Provides CRUD operations for loop runs, loop sessions, events,
//! artifacts, and verifier runs. Migration is idempotent and safe
//! on existing production databases.

use rusqlite::{params, Connection, Result as SqlResult};
use serde_json::Value as JsonValue;

use crate::loop_run::*;

// ─── Params ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CreateLoopParams {
    pub project_id: String,
    pub task_key: Option<String>,
    pub parent_session_id: String,
    pub strategy: LoopStrategy,
    pub goal: String,
    pub max_rounds: i64,
    pub policy_json: Option<JsonValue>,
    pub budget_json: Option<JsonValue>,
}

#[derive(Debug, Clone)]
pub struct AddLoopSessionParams {
    pub loop_id: String,
    pub session_id: String,
    pub role: String,
    pub round: i64,
    pub provider: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct AddArtifactParams {
    pub loop_id: String,
    pub session_id: Option<String>,
    pub kind: String,
    pub path: Option<String>,
    pub content_json: Option<JsonValue>,
}

#[derive(Debug, Clone)]
pub struct AddVerifierRunParams {
    pub loop_id: String,
    pub session_id: Option<String>,
    pub verifier_type: String,
    pub name: String,
    pub command: String,
}

// ─── Service ─────────────────────────────────────────────────────────────────

pub struct LoopService;

impl LoopService {
    /// Idempotent migration — safe to run on fresh and existing production databases.
    pub fn migrate(conn: &Connection) -> SqlResult<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS loop_runs (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                task_key TEXT,
                parent_session_id TEXT NOT NULL,
                strategy TEXT NOT NULL,
                goal TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'draft',
                current_round INTEGER NOT NULL DEFAULT 0,
                max_rounds INTEGER NOT NULL DEFAULT 3,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                finished_at TEXT,
                policy_json TEXT,
                budget_json TEXT
            );

            CREATE TABLE IF NOT EXISTS loop_sessions (
                loop_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                round INTEGER NOT NULL,
                provider TEXT,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (loop_id, session_id)
            );

            CREATE TABLE IF NOT EXISTS loop_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                loop_id TEXT NOT NULL,
                ts TEXT NOT NULL,
                kind TEXT NOT NULL,
                payload_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS loop_artifacts (
                id TEXT PRIMARY KEY,
                loop_id TEXT NOT NULL,
                session_id TEXT,
                kind TEXT NOT NULL,
                path TEXT,
                content_json TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS verifier_runs (
                id TEXT PRIMARY KEY,
                loop_id TEXT NOT NULL,
                session_id TEXT,
                verifier_type TEXT NOT NULL,
                name TEXT NOT NULL,
                command TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                exit_code INTEGER,
                output_path TEXT,
                created_at TEXT NOT NULL,
                finished_at TEXT
            );",
        )?;
        Ok(())
    }

    // ─── Loop Runs ───────────────────────────────────────────────────────────

    pub fn create_loop(conn: &Connection, params: CreateLoopParams) -> SqlResult<LoopRun> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let policy_str = params.policy_json.as_ref().map(|v| v.to_string());
        let budget_str = params.budget_json.as_ref().map(|v| v.to_string());

        conn.execute(
            "INSERT INTO loop_runs (id, project_id, task_key, parent_session_id, strategy, goal, status, current_round, max_rounds, created_at, updated_at, policy_json, budget_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10, ?11, ?12)",
            params![
                id,
                params.project_id,
                params.task_key,
                params.parent_session_id,
                params.strategy.as_str(),
                params.goal,
                LoopStatus::Draft.as_str(),
                params.max_rounds,
                now,
                now,
                policy_str,
                budget_str,
            ],
        )?;

        Ok(LoopRun {
            id,
            project_id: params.project_id,
            task_key: params.task_key,
            parent_session_id: params.parent_session_id,
            strategy: params.strategy,
            goal: params.goal,
            status: LoopStatus::Draft,
            current_round: 0,
            max_rounds: params.max_rounds,
            created_at: now.clone(),
            updated_at: now,
            finished_at: None,
            policy_json: params.policy_json,
            budget_json: params.budget_json,
        })
    }

    pub fn get_loop(conn: &Connection, id: &str) -> SqlResult<Option<LoopRun>> {
        conn.prepare(
            "SELECT id, project_id, task_key, parent_session_id, strategy, goal, status, current_round, max_rounds, created_at, updated_at, finished_at, policy_json, budget_json
             FROM loop_runs WHERE id = ?1",
        )?
        .query_row(params![id], |row| {
            Ok(LoopRun {
                id: row.get(0)?,
                project_id: row.get(1)?,
                task_key: row.get(2)?,
                parent_session_id: row.get(3)?,
                strategy: LoopStrategy::new(row.get::<_, String>(4)?),
                goal: row.get(5)?,
                status: LoopStatus::parse(&row.get::<_, String>(6)?)
                    .unwrap_or(LoopStatus::Draft),
                current_round: row.get(7)?,
                max_rounds: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
                finished_at: row.get(11)?,
                policy_json: row
                    .get::<_, Option<String>>(12)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                budget_json: row
                    .get::<_, Option<String>>(13)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
            })
        })
        .ok()
        .map_or(Ok(None), |r| Ok(Some(r)))
    }

    pub fn list_loops(conn: &Connection, project_id: &str) -> SqlResult<Vec<LoopRun>> {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, task_key, parent_session_id, strategy, goal, status, current_round, max_rounds, created_at, updated_at, finished_at, policy_json, budget_json
             FROM loop_runs WHERE project_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![project_id], |row| {
            Ok(LoopRun {
                id: row.get(0)?,
                project_id: row.get(1)?,
                task_key: row.get(2)?,
                parent_session_id: row.get(3)?,
                strategy: LoopStrategy::new(row.get::<_, String>(4)?),
                goal: row.get(5)?,
                status: LoopStatus::parse(&row.get::<_, String>(6)?)
                    .unwrap_or(LoopStatus::Draft),
                current_round: row.get(7)?,
                max_rounds: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
                finished_at: row.get(11)?,
                policy_json: row
                    .get::<_, Option<String>>(12)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                budget_json: row
                    .get::<_, Option<String>>(13)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
            })
        })?;
        rows.collect()
    }

    pub fn update_loop_status(
        conn: &Connection,
        id: &str,
        status: LoopStatus,
    ) -> SqlResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let finished_at = match status {
            LoopStatus::CompletedUnreviewed
            | LoopStatus::Failed
            | LoopStatus::Cancelled
            | LoopStatus::Approved
            | LoopStatus::Merged
            | LoopStatus::Cleaned => Some(now.clone()),
            _ => None,
        };
        conn.execute(
            "UPDATE loop_runs SET status = ?1, updated_at = ?2, finished_at = COALESCE(?3, finished_at) WHERE id = ?4",
            params![status.as_str(), now, finished_at, id],
        )?;
        Ok(())
    }

    // ─── Loop Sessions ───────────────────────────────────────────────────────

    pub fn add_loop_session(
        conn: &Connection,
        params: AddLoopSessionParams,
    ) -> SqlResult<LoopSession> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO loop_sessions (loop_id, session_id, role, round, provider, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                params.loop_id,
                params.session_id,
                params.role,
                params.round,
                params.provider,
                params.status,
                now,
            ],
        )?;
        Ok(LoopSession {
            loop_id: params.loop_id,
            session_id: params.session_id,
            role: params.role,
            round: params.round,
            provider: params.provider,
            status: params.status,
            created_at: now,
        })
    }

    pub fn list_loop_sessions(
        conn: &Connection,
        loop_id: &str,
    ) -> SqlResult<Vec<LoopSession>> {
        let mut stmt = conn.prepare(
            "SELECT loop_id, session_id, role, round, provider, status, created_at
             FROM loop_sessions WHERE loop_id = ?1 ORDER BY round, created_at",
        )?;
        let rows = stmt.query_map(params![loop_id], |row| {
            Ok(LoopSession {
                loop_id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                round: row.get(3)?,
                provider: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    // ─── Events ──────────────────────────────────────────────────────────────

    pub fn append_loop_event(
        conn: &Connection,
        loop_id: &str,
        kind: &str,
        payload: &JsonValue,
    ) -> SqlResult<LoopEvent> {
        let now = chrono::Utc::now().to_rfc3339();
        let payload_str = payload.to_string();
        conn.execute(
            "INSERT INTO loop_events (loop_id, ts, kind, payload_json) VALUES (?1, ?2, ?3, ?4)",
            params![loop_id, now, kind, payload_str],
        )?;
        let id = conn.last_insert_rowid();
        Ok(LoopEvent {
            id,
            loop_id: loop_id.to_string(),
            ts: now,
            kind: kind.to_string(),
            payload_json: payload.clone(),
        })
    }

    pub fn list_loop_events(conn: &Connection, loop_id: &str) -> SqlResult<Vec<LoopEvent>> {
        let mut stmt = conn.prepare(
            "SELECT id, loop_id, ts, kind, payload_json FROM loop_events WHERE loop_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![loop_id], |row| {
            let payload_str: String = row.get(4)?;
            Ok(LoopEvent {
                id: row.get(0)?,
                loop_id: row.get(1)?,
                ts: row.get(2)?,
                kind: row.get(3)?,
                payload_json: serde_json::from_str(&payload_str).unwrap_or(JsonValue::Null),
            })
        })?;
        rows.collect()
    }

    // ─── Artifacts ───────────────────────────────────────────────────────────

    pub fn add_artifact(conn: &Connection, params: AddArtifactParams) -> SqlResult<LoopArtifact> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let content_str = params.content_json.as_ref().map(|v| v.to_string());
        conn.execute(
            "INSERT INTO loop_artifacts (id, loop_id, session_id, kind, path, content_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                params.loop_id,
                params.session_id,
                params.kind,
                params.path,
                content_str,
                now,
            ],
        )?;
        Ok(LoopArtifact {
            id,
            loop_id: params.loop_id,
            session_id: params.session_id,
            kind: params.kind,
            path: params.path,
            content_json: params.content_json,
            created_at: now,
        })
    }

    // ─── Verifier Runs ───────────────────────────────────────────────────────

    pub fn add_verifier_run(
        conn: &Connection,
        params: AddVerifierRunParams,
    ) -> SqlResult<VerifierRun> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO verifier_runs (id, loop_id, session_id, verifier_type, name, command, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)",
            params![
                id,
                params.loop_id,
                params.session_id,
                params.verifier_type,
                params.name,
                params.command,
                now,
            ],
        )?;
        Ok(VerifierRun {
            id,
            loop_id: params.loop_id,
            session_id: params.session_id,
            verifier_type: params.verifier_type,
            name: params.name,
            command: params.command,
            status: "pending".to_string(),
            exit_code: None,
            output_path: None,
            created_at: now,
            finished_at: None,
        })
    }

    pub fn update_verifier_run(
        conn: &Connection,
        id: &str,
        status: &str,
        exit_code: Option<i32>,
        output_path: Option<&str>,
    ) -> SqlResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE verifier_runs SET status = ?1, exit_code = ?2, output_path = ?3, finished_at = ?4 WHERE id = ?5",
            params![status, exit_code, output_path, now, id],
        )?;
        Ok(())
    }
}
