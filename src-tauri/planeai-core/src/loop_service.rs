//! Loop service — persistence layer for durable loop runs.
//!
//! Provides CRUD operations for loop runs, loop sessions, events,
//! artifacts, and verifier runs. Migration is idempotent and safe
//! on existing production databases.

use rusqlite::{params, Connection, Result as SqlResult};
use serde_json::Value as JsonValue;

use crate::loop_run::*;

// ─── Errors ──────────────────────────────────────────────────────────────────

/// Error returned when the database contains an invalid loop status.
/// Service methods fail loudly on corruption — the executor must not silently
/// misinterpret state.
#[derive(Debug, PartialEq)]
pub struct InvalidLoopStatus(pub String);

impl std::fmt::Display for InvalidLoopStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid loop status in database: {:?}", self.0)
    }
}

impl std::error::Error for InvalidLoopStatus {}

/// Parse loop status strictly — returns an error for unrecognized values.
/// The loop executor should never silently misinterpret corrupted state.
fn parse_loop_status_strict(s: &str) -> Result<LoopStatus, InvalidLoopStatus> {
    LoopStatus::parse(s).ok_or_else(|| InvalidLoopStatus(s.to_string()))
}

/// Parse loop status lossily — falls back to Draft for unrecognized values.
/// Use only for UI display where a bad row should not crash the view.
pub fn parse_loop_status_lossy(s: &str) -> LoopStatus {
    LoopStatus::parse(s).unwrap_or_else(|| {
        tracing::warn!(status = %s, "unrecognized loop status in database, falling back to Draft");
        LoopStatus::Draft
    })
}

// ─── Params ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CreateLoopParams {
    pub project_id: String,
    pub task_key: Option<String>,
    pub created_by_session_id: Option<String>,
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

/// Parameters for the atomic handoff recording operation.
#[derive(Debug, Clone)]
pub struct RecordHandoffParams {
    pub loop_id: String,
    pub session_id: String,
    pub artifact_path: Option<String>,
    pub content_json: Option<JsonValue>,
    pub handoff_status: String,
    pub event_payload: JsonValue,
    /// If Some, the loop status will be updated to this value.
    pub new_loop_status: Option<LoopStatus>,
}

/// Result of a successful atomic handoff recording.
#[derive(Debug, Clone)]
pub struct RecordHandoffResult {
    pub artifact_id: String,
    pub event_id: i64,
}

// ─── Service ─────────────────────────────────────────────────────────────────

pub struct LoopService;

impl LoopService {
    /// Idempotent migration — safe to run on fresh and existing production databases.
    ///
    /// Order:
    /// 1. Create tables if missing (fresh DB)
    /// 2. Rebuild loop_runs from #269 schema if needed (rename + nullable fix)
    /// 3. Create indexes (references final column names)
    pub fn migrate(conn: &Connection) -> SqlResult<()> {
        Self::create_tables_if_missing(conn)?;
        Self::migrate_loop_runs_269_schema_if_needed(conn)?;
        Self::migrate_verifier_runs_started_at(conn)?;
        Self::create_indexes(conn)?;
        Ok(())
    }

    fn create_tables_if_missing(conn: &Connection) -> SqlResult<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS loop_runs (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                task_key TEXT,
                created_by_session_id TEXT,
                strategy TEXT NOT NULL,
                goal TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'draft',
                current_round INTEGER NOT NULL DEFAULT 0,
                max_rounds INTEGER NOT NULL DEFAULT 3,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                executor_finished_at TEXT,
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
                started_at TEXT,
                finished_at TEXT
            );",
        )
    }

    /// Rebuild loop_runs from #269 schema if needed.
    ///
    /// #269 had `parent_session_id TEXT NOT NULL` and `finished_at TEXT`.
    /// The new schema uses `created_by_session_id TEXT` (nullable) and
    /// `executor_finished_at TEXT`. SQLite RENAME COLUMN preserves NOT NULL,
    /// so we must use the full table-rebuild procedure inside a transaction.
    fn migrate_loop_runs_269_schema_if_needed(conn: &Connection) -> SqlResult<()> {
        // Use PRAGMA table_info for robust detection (immune to comments/defaults)
        let mut stmt = conn.prepare("PRAGMA table_info(loop_runs)")?;
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();

        // If the table doesn't exist yet (fresh DB), nothing to migrate
        if columns.is_empty() {
            return Ok(());
        }

        let has_parent_col = columns.iter().any(|c| c == "parent_session_id");
        let has_old_finished = columns.iter().any(|c| c == "finished_at")
            && !columns.iter().any(|c| c == "executor_finished_at");

        if !has_parent_col && !has_old_finished {
            return Ok(());
        }

        let source_session_col = if has_parent_col {
            "parent_session_id"
        } else {
            "created_by_session_id"
        };
        let source_finished_col = if has_old_finished {
            "finished_at"
        } else {
            "executor_finished_at"
        };

        let tx = conn.unchecked_transaction()?;

        // DROP IF EXISTS handles recovery from a previously half-failed migration
        tx.execute_batch(
            "DROP TABLE IF EXISTS loop_runs_new;

            CREATE TABLE loop_runs_new (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                task_key TEXT,
                created_by_session_id TEXT,
                strategy TEXT NOT NULL,
                goal TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'draft',
                current_round INTEGER NOT NULL DEFAULT 0,
                max_rounds INTEGER NOT NULL DEFAULT 3,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                executor_finished_at TEXT,
                policy_json TEXT,
                budget_json TEXT
            );",
        )?;

        // SAFETY: source_session_col and source_finished_col are compile-time string
        // literals ("parent_session_id"/"created_by_session_id" and "finished_at"/
        // "executor_finished_at"), never user input. No SQL injection risk.
        tx.execute_batch(&format!(
            "INSERT INTO loop_runs_new (
                id, project_id, task_key, created_by_session_id, strategy, goal,
                status, current_round, max_rounds, created_at, updated_at,
                executor_finished_at, policy_json, budget_json
            )
            SELECT
                id, project_id, task_key, {source_session_col}, strategy, goal,
                status, current_round, max_rounds, created_at, updated_at,
                {source_finished_col}, policy_json, budget_json
            FROM loop_runs;

            DROP TABLE loop_runs;
            ALTER TABLE loop_runs_new RENAME TO loop_runs;",
        ))?;

        tx.commit()?;
        Ok(())
    }

    /// Add started_at column to verifier_runs if missing (idempotent).
    fn migrate_verifier_runs_started_at(conn: &Connection) -> SqlResult<()> {
        let mut stmt = conn.prepare("PRAGMA table_info(verifier_runs)")?;
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();

        if columns.is_empty() || columns.iter().any(|c| c == "started_at") {
            return Ok(());
        }

        conn.execute_batch("ALTER TABLE verifier_runs ADD COLUMN started_at TEXT;")?;
        Ok(())
    }

    fn create_indexes(conn: &Connection) -> SqlResult<()> {
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_loop_runs_project_status_updated
                ON loop_runs(project_id, status, updated_at);

            CREATE INDEX IF NOT EXISTS idx_loop_runs_task_key
                ON loop_runs(task_key);

            CREATE INDEX IF NOT EXISTS idx_loop_runs_created_by_session_id
                ON loop_runs(created_by_session_id);

            CREATE INDEX IF NOT EXISTS idx_loop_sessions_loop_id
                ON loop_sessions(loop_id);

            CREATE INDEX IF NOT EXISTS idx_loop_sessions_session_id
                ON loop_sessions(session_id);

            CREATE INDEX IF NOT EXISTS idx_loop_events_loop_id_id
                ON loop_events(loop_id, id);

            CREATE INDEX IF NOT EXISTS idx_loop_artifacts_loop_id
                ON loop_artifacts(loop_id);

            CREATE INDEX IF NOT EXISTS idx_verifier_runs_loop_id
                ON verifier_runs(loop_id);",
        )
    }

    // ─── Helpers ─────────────────────────────────────────────────────────────

    /// Touch the loop's updated_at timestamp. Called by child-write methods so
    /// that loop staleness detection works even when only child tables change.
    /// Fails if the loop does not exist — prevents orphan child rows.
    fn touch_loop(conn: &Connection, loop_id: &str) -> SqlResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let rows = conn.execute(
            "UPDATE loop_runs SET updated_at = ?1 WHERE id = ?2",
            params![now, loop_id],
        )?;
        if rows == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }

    // ─── Loop Runs ───────────────────────────────────────────────────────────

    pub fn create_loop(conn: &Connection, params: CreateLoopParams) -> SqlResult<LoopRun> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let policy_str = params.policy_json.as_ref().map(|v| v.to_string());
        let budget_str = params.budget_json.as_ref().map(|v| v.to_string());

        conn.execute(
            "INSERT INTO loop_runs (id, project_id, task_key, created_by_session_id, strategy, goal, status, current_round, max_rounds, created_at, updated_at, policy_json, budget_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10, ?11, ?12)",
            params![
                id,
                params.project_id,
                params.task_key,
                params.created_by_session_id,
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
            created_by_session_id: params.created_by_session_id,
            strategy: params.strategy,
            goal: params.goal,
            status: LoopStatus::Draft,
            current_round: 0,
            max_rounds: params.max_rounds,
            created_at: now.clone(),
            updated_at: now,
            executor_finished_at: None,
            policy_json: params.policy_json,
            budget_json: params.budget_json,
        })
    }

    pub fn get_loop(conn: &Connection, id: &str) -> Result<Option<LoopRun>, LoopServiceError> {
        let row = conn
            .prepare(
                "SELECT id, project_id, task_key, created_by_session_id, strategy, goal, status, current_round, max_rounds, created_at, updated_at, executor_finished_at, policy_json, budget_json
                 FROM loop_runs WHERE id = ?1",
            )?
            .query_row(params![id], Self::row_to_loop_run);

        match row {
            Ok(run) => Ok(Some(run?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(LoopServiceError::Db(e)),
        }
    }

    pub fn list_loops(
        conn: &Connection,
        project_id: &str,
    ) -> Result<Vec<LoopRun>, LoopServiceError> {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, task_key, created_by_session_id, strategy, goal, status, current_round, max_rounds, created_at, updated_at, executor_finished_at, policy_json, budget_json
             FROM loop_runs WHERE project_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![project_id], Self::row_to_loop_run)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row??);
        }
        Ok(results)
    }

    pub fn update_loop_status(conn: &Connection, id: &str, status: LoopStatus) -> SqlResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        // executor_finished_at is set when the executor is done producing a
        // reviewable result — i.e., it has finished its work and handed off to
        // human review or cleanup. Statuses after this point are lifecycle, not
        // executor activity.
        let executor_finished_at = if status.is_executor_terminal() {
            Some(now.clone())
        } else {
            None
        };
        let rows_affected = conn.execute(
            "UPDATE loop_runs SET status = ?1, updated_at = ?2, executor_finished_at = COALESCE(?3, executor_finished_at) WHERE id = ?4",
            params![status.as_str(), now, executor_finished_at, id],
        )?;
        if rows_affected == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }

    /// Update the resolved recipe snapshot stored in policy_json.
    /// Wraps the raw column update with touch_loop semantics so staleness
    /// detection and future audit hooks work correctly.
    pub fn update_policy_json(
        conn: &Connection,
        id: &str,
        policy_json: &serde_json::Value,
    ) -> SqlResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let json_str = policy_json.to_string();
        let tx = conn.unchecked_transaction()?;
        let rows = tx.execute(
            "UPDATE loop_runs SET policy_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![json_str, now, id],
        )?;
        if rows == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        tx.commit()?;
        Ok(())
    }

    // ─── Loop Sessions ───────────────────────────────────────────────────────

    pub fn add_loop_session(
        conn: &Connection,
        params: AddLoopSessionParams,
    ) -> SqlResult<LoopSession> {
        let now = chrono::Utc::now().to_rfc3339();
        let tx = conn.unchecked_transaction()?;
        tx.execute(
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
        Self::touch_loop(&tx, &params.loop_id)?;
        tx.commit()?;
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

    /// Update the status of a loop session (loop_sessions.status, NOT the runtime session).
    pub fn update_loop_session_status(
        conn: &Connection,
        loop_id: &str,
        session_id: &str,
        status: &str,
    ) -> SqlResult<()> {
        let tx = conn.unchecked_transaction()?;
        let rows = tx.execute(
            "UPDATE loop_sessions SET status = ?1 WHERE loop_id = ?2 AND session_id = ?3",
            params![status, loop_id, session_id],
        )?;
        if rows == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Self::touch_loop(&tx, loop_id)?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_loop_sessions(conn: &Connection, loop_id: &str) -> SqlResult<Vec<LoopSession>> {
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
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO loop_events (loop_id, ts, kind, payload_json) VALUES (?1, ?2, ?3, ?4)",
            params![loop_id, now, kind, payload_str],
        )?;
        let id = tx.last_insert_rowid();
        Self::touch_loop(&tx, loop_id)?;
        tx.commit()?;
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
        // Enforce invariant: handoff artifacts must go through record_handoff
        // which performs atomic session status update + event append.
        if params.kind == "handoff" {
            return Err(rusqlite::Error::InvalidParameterName(
                "use LoopService::record_handoff for handoff artifacts".to_string(),
            ));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let content_str = params.content_json.as_ref().map(|v| v.to_string());
        let tx = conn.unchecked_transaction()?;
        tx.execute(
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
        Self::touch_loop(&tx, &params.loop_id)?;
        tx.commit()?;
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

    /// Find the most recent accepted handoff artifact for any of the given session IDs.
    /// Returns (session_id, handoff_status) if found.
    ///
    /// Strict validation rules:
    /// - content_json.schema must equal "planeai.handoff.v1"
    /// - content_json.status must be one of: completed, blocked, needs_human, failed
    /// - Returns the globally latest matching handoff across all candidate sessions
    ///
    /// A handoff is "accepted" when it was recorded through `LoopService::record_handoff`.
    /// The durable representation is `loop_artifacts(kind = "handoff")`.
    /// Recipe steps must not treat arbitrary artifact rows as accepted unless they
    /// came through the record_handoff path and pass schema/status validation.
    pub fn find_handoff_for_sessions(
        conn: &Connection,
        loop_id: &str,
        session_ids: &[String],
    ) -> SqlResult<Option<(String, String)>> {
        if session_ids.is_empty() {
            return Ok(None);
        }

        const VALID_HANDOFF_STATUSES: &[&str] = &["completed", "blocked", "needs_human", "failed"];

        // Build IN clause with positional params
        let placeholders: Vec<String> = (0..session_ids.len())
            .map(|i| format!("?{}", i + 2))
            .collect();
        // Fetch recent handoff artifacts (newest first) — we validate in Rust
        // because SQLite JSON functions aren't guaranteed available.
        let sql = format!(
            "SELECT session_id, content_json FROM loop_artifacts \
             WHERE loop_id = ?1 AND kind = 'handoff' \
             AND session_id IN ({}) \
             ORDER BY created_at DESC, id DESC",
            placeholders.join(", ")
        );
        let mut stmt = conn.prepare(&sql)?;
        // Bind params: ?1 = loop_id, ?2..N = session_ids
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        params_vec.push(Box::new(loop_id.to_string()));
        for sid in session_ids {
            params_vec.push(Box::new(sid.clone()));
        }
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut rows = stmt.query(params_refs.as_slice())?;
        while let Some(row) = rows.next()? {
            let session_id: String = row.get(0)?;
            let content_json: Option<String> = row.get(1)?;

            let Some(json_str) = content_json else {
                continue;
            };
            let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) else {
                continue;
            };

            // Require schema == "planeai.handoff.v1"
            let schema = val.get("schema").and_then(|v| v.as_str()).unwrap_or("");
            if schema != "planeai.handoff.v1" {
                continue;
            }

            // Require status is one of the valid handoff statuses
            let Some(status) = val.get("status").and_then(|v| v.as_str()) else {
                continue;
            };
            if !VALID_HANDOFF_STATUSES.contains(&status) {
                continue;
            }

            return Ok(Some((session_id, status.to_string())));
        }

        Ok(None)
    }

    // ─── Handoff Recording (atomic) ──────────────────────────────────────────

    /// Atomically record a handoff: insert artifact, append event, update session
    /// status, and optionally update loop status — all in a single transaction.
    ///
    /// Fails if the loop does not exist or if the session is not part of the loop.
    /// On failure, no partial state is written.
    pub fn record_handoff(
        conn: &Connection,
        params: RecordHandoffParams,
    ) -> SqlResult<RecordHandoffResult> {
        let artifact_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let content_str = params.content_json.as_ref().map(|v| v.to_string());
        let payload_str = params.event_payload.to_string();

        let tx = conn.unchecked_transaction()?;

        // 1. Assert loop exists (via touch_loop)
        Self::touch_loop(&tx, &params.loop_id)?;

        // 2. Assert session belongs to this loop
        let session_rows = tx.execute(
            "UPDATE loop_sessions SET status = ?1 WHERE loop_id = ?2 AND session_id = ?3",
            rusqlite::params![params.handoff_status, params.loop_id, params.session_id],
        )?;
        if session_rows == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }

        // 3. Insert loop_artifact
        tx.execute(
            "INSERT INTO loop_artifacts (id, loop_id, session_id, kind, path, content_json, created_at)
             VALUES (?1, ?2, ?3, 'handoff', ?4, ?5, ?6)",
            rusqlite::params![
                artifact_id,
                params.loop_id,
                params.session_id,
                params.artifact_path,
                content_str,
                now,
            ],
        )?;

        // 4. Append loop_event
        tx.execute(
            "INSERT INTO loop_events (loop_id, ts, kind, payload_json) VALUES (?1, ?2, 'handoff_recorded', ?3)",
            rusqlite::params![params.loop_id, now, payload_str],
        )?;
        let event_id = tx.last_insert_rowid();

        // 5. Update loop status if requested
        if let Some(ref new_status) = params.new_loop_status {
            let executor_finished_at = if new_status.is_executor_terminal() {
                Some(now.clone())
            } else {
                None
            };
            tx.execute(
                "UPDATE loop_runs SET status = ?1, updated_at = ?2, executor_finished_at = COALESCE(?3, executor_finished_at) WHERE id = ?4",
                rusqlite::params![new_status.as_str(), now, executor_finished_at, params.loop_id],
            )?;
        }

        tx.commit()?;

        Ok(RecordHandoffResult {
            artifact_id,
            event_id,
        })
    }

    // ─── Verifier Runs ───────────────────────────────────────────────────────

    pub fn add_verifier_run(
        conn: &Connection,
        params: AddVerifierRunParams,
    ) -> SqlResult<VerifierRun> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let tx = conn.unchecked_transaction()?;
        tx.execute(
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
        Self::touch_loop(&tx, &params.loop_id)?;
        tx.commit()?;
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
            started_at: None,
            finished_at: None,
        })
    }

    /// Transition a verifier run from 'pending' to 'running' and set started_at.
    pub fn start_verifier_run(conn: &Connection, id: &str) -> SqlResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE verifier_runs SET status = 'running', started_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn update_verifier_run(
        conn: &Connection,
        id: &str,
        status: &str,
        exit_code: Option<i32>,
        output_path: Option<&str>,
    ) -> SqlResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let tx = conn.unchecked_transaction()?;
        // Look up the loop_id so we can touch the parent loop
        let loop_id: String = tx.query_row(
            "SELECT loop_id FROM verifier_runs WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        tx.execute(
            "UPDATE verifier_runs SET status = ?1, exit_code = ?2, output_path = ?3, finished_at = ?4 WHERE id = ?5",
            params![status, exit_code, output_path, now, id],
        )?;
        Self::touch_loop(&tx, &loop_id)?;
        tx.commit()?;
        Ok(())
    }

    /// Atomically complete a verifier run: update status/exit_code/output_path
    /// AND append a verifier_completed event in one transaction.
    ///
    /// Fails if the verifier run or loop does not exist. On failure, no partial
    /// state is written.
    pub fn complete_verifier_run(
        conn: &Connection,
        id: &str,
        status: &str,
        exit_code: Option<i32>,
        output_path: Option<&str>,
        event_payload: &serde_json::Value,
    ) -> SqlResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let payload_str = event_payload.to_string();
        let tx = conn.unchecked_transaction()?;

        // Look up the loop_id
        let loop_id: String = tx.query_row(
            "SELECT loop_id FROM verifier_runs WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;

        // Update verifier_run
        tx.execute(
            "UPDATE verifier_runs SET status = ?1, exit_code = ?2, output_path = ?3, finished_at = ?4 WHERE id = ?5",
            params![status, exit_code, output_path, now, id],
        )?;

        // Append verifier_completed event
        tx.execute(
            "INSERT INTO loop_events (loop_id, ts, kind, payload_json) VALUES (?1, ?2, 'verifier_completed', ?3)",
            params![loop_id, now, payload_str],
        )?;

        Self::touch_loop(&tx, &loop_id)?;
        tx.commit()?;
        Ok(())
    }

    // ─── Private helpers ─────────────────────────────────────────────────────

    fn row_to_loop_run(row: &rusqlite::Row) -> rusqlite::Result<Result<LoopRun, LoopServiceError>> {
        let status_str: String = row.get(6)?;
        let status = match parse_loop_status_strict(&status_str) {
            Ok(s) => s,
            Err(e) => return Ok(Err(LoopServiceError::InvalidStatus(e))),
        };
        Ok(Ok(LoopRun {
            id: row.get(0)?,
            project_id: row.get(1)?,
            task_key: row.get(2)?,
            created_by_session_id: row.get(3)?,
            strategy: LoopStrategy::new(row.get::<_, String>(4)?),
            goal: row.get(5)?,
            status,
            current_round: row.get(7)?,
            max_rounds: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
            executor_finished_at: row.get(11)?,
            policy_json: row
                .get::<_, Option<String>>(12)?
                .and_then(|s| serde_json::from_str(&s).ok()),
            budget_json: row
                .get::<_, Option<String>>(13)?
                .and_then(|s| serde_json::from_str(&s).ok()),
        }))
    }
}

// ─── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum LoopServiceError {
    Db(rusqlite::Error),
    InvalidStatus(InvalidLoopStatus),
}

impl std::fmt::Display for LoopServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Db(e) => write!(f, "loop service database error: {e}"),
            Self::InvalidStatus(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LoopServiceError {}

impl From<rusqlite::Error> for LoopServiceError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e)
    }
}
