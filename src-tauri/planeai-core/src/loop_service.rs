//! Loop service — persistence layer for durable loop runs.
//!
//! Provides CRUD operations for loop runs, loop sessions, events,
//! artifacts, and verifier runs. Migration is idempotent and safe
//! on existing production databases.

use rusqlite::{params, Connection, Result as SqlResult};
use serde_json::Value as JsonValue;

use crate::loop_recipe_service::RecipeSnapshot;
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

/// Error returned by [`LoopService::transition_loop`] and [`LoopService::transition_in_tx`].
#[derive(Debug)]
pub enum TransitionError {
    /// The trigger is not valid from the current status.
    Invalid(InvalidTransition),
    /// The loop ID was not found in the database.
    NotFound(String),
    /// The database contains an unrecognized loop status value.
    CorruptStatus(String),
    /// A database error occurred during the transition.
    Db(rusqlite::Error),
}

impl std::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(e) => write!(f, "{e}"),
            Self::NotFound(id) => write!(f, "loop not found: {id}"),
            Self::CorruptStatus(s) => write!(f, "corrupt loop status in database: {s:?}"),
            Self::Db(e) => write!(f, "transition database error: {e}"),
        }
    }
}

impl std::error::Error for TransitionError {}

impl From<rusqlite::Error> for TransitionError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e)
    }
}

impl From<InvalidTransition> for TransitionError {
    fn from(e: InvalidTransition) -> Self {
        Self::Invalid(e)
    }
}

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
    /// If Some, the loop transition will be applied atomically.
    pub trigger: Option<LoopTrigger>,
}

/// Result of a successful atomic handoff recording.
#[derive(Debug, Clone)]
pub struct RecordHandoffResult {
    pub artifact_id: String,
    pub event_id: i64,
}

// ─── Service ─────────────────────────────────────────────────────────────────

pub struct LoopService;

// ─── Status Derivation Helper ────────────────────────────────────────────────

/// Compute the effective `LoopStatus` for a recipe snapshot.
///
/// Resolution order:
/// 1. `snapshot.runtime.status_override` — wins when set (blocking executors).
/// 2. Step-kind derivation via [`derive_status_from_step`] using the current step.
///
/// Returns `None` only if the current step kind is unrecognized and no override is set.
pub fn derive_effective_status(snapshot: &RecipeSnapshot) -> Option<LoopStatus> {
    snapshot.runtime.status_override.clone().or_else(|| {
        let current_step = snapshot
            .steps
            .iter()
            .find(|s| s.id == snapshot.runtime.current_step);
        let step_kind = current_step.map(|s| s.kind.as_str()).unwrap_or("unknown");
        let step_status = current_step.and_then(|s| s.status.as_deref());
        derive_status_from_step(step_kind, step_status)
    })
}

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
                "SELECT id, project_id, task_key, created_by_session_id, strategy, goal, status, max_rounds, created_at, updated_at, executor_finished_at, policy_json, budget_json
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
            "SELECT id, project_id, task_key, created_by_session_id, strategy, goal, status, max_rounds, created_at, updated_at, executor_finished_at, policy_json, budget_json
             FROM loop_runs WHERE project_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![project_id], Self::row_to_loop_run)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row??);
        }
        Ok(results)
    }

    // ─── Transition Table API ────────────────────────────────────────────────

    /// Transition a loop's status by applying a trigger event.
    ///
    /// Validates the transition, persists the new status, and logs an audit
    /// event atomically. Returns the resulting status. On `Unchanged` (no-op),
    /// skips the DB write and returns the current status.
    pub fn transition_loop(
        conn: &Connection,
        id: &str,
        trigger: LoopTrigger,
    ) -> Result<LoopStatus, TransitionError> {
        let tx = conn.unchecked_transaction().map_err(TransitionError::Db)?;
        let result = Self::transition_in_tx(&tx, id, trigger)?;
        tx.commit().map_err(TransitionError::Db)?;
        Ok(result)
    }

    /// Transition a loop's status within a caller-provided transaction.
    ///
    /// Use this when the transition must be atomic with other writes
    /// (e.g., `record_handoff` bundles artifact + status in one tx).
    ///
    /// For recipe-driven loops, validates that the transition result is consistent
    /// with what `persist_snapshot` derivation would produce. Logs a warning on
    /// divergence (expected for lifecycle triggers like Start/Cancel/Approve that
    /// intentionally override the step-derived status).
    pub fn transition_in_tx(
        tx: &rusqlite::Transaction,
        id: &str,
        trigger: LoopTrigger,
    ) -> Result<LoopStatus, TransitionError> {
        // 1. Load current status
        let current_status_str: String = tx
            .query_row(
                "SELECT status FROM loop_runs WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => TransitionError::NotFound(id.to_string()),
                other => TransitionError::Db(other),
            })?;

        let current = LoopStatus::parse(&current_status_str)
            .ok_or_else(|| TransitionError::CorruptStatus(current_status_str.clone()))?;

        // 2. Apply the transition table
        let result = apply(&current, &trigger)?;

        // 3. Handle result
        match result {
            TransitionResult::Unchanged => Ok(current),
            TransitionResult::Changed(ref new_status) => {
                let now = chrono::Utc::now().to_rfc3339();
                let executor_finished_at = if new_status.is_executor_terminal() {
                    Some(now.clone())
                } else {
                    None
                };

                // Persist the new status
                tx.execute(
                    "UPDATE loop_runs SET status = ?1, updated_at = ?2, executor_finished_at = COALESCE(?3, executor_finished_at) WHERE id = ?4",
                    params![new_status.as_str(), now, executor_finished_at, id],
                )?;

                // Log audit event
                let payload = serde_json::json!({
                    "from": current.as_str(),
                    "to": new_status.as_str(),
                    "trigger": &trigger,
                });
                tx.execute(
                    "INSERT INTO loop_events (loop_id, ts, kind, payload_json) VALUES (?1, ?2, 'status_transition', ?3)",
                    params![id, now, payload.to_string()],
                )?;

                // Validate: if this loop has a recipe snapshot, check whether
                // the trigger-driven status agrees with step-pointer derivation.
                // Divergence is expected for lifecycle triggers (Start, Cancel,
                // Approve, etc.) but signals a potential desync for recipe triggers.
                let policy_str: Option<String> = tx
                    .query_row(
                        "SELECT policy_json FROM loop_runs WHERE id = ?1",
                        params![id],
                        |row| row.get(0),
                    )
                    .ok()
                    .flatten();
                if let Some(ref json_str) = policy_str {
                    if let Ok(snapshot) = serde_json::from_str::<RecipeSnapshot>(json_str) {
                        let derived = derive_effective_status(&snapshot);
                        if let Some(ref expected) = derived {
                            if expected != new_status {
                                tracing::warn!(
                                    loop_id = %id,
                                    trigger = %trigger.name(),
                                    transition_status = %new_status.as_str(),
                                    derived_status = %expected.as_str(),
                                    "transition_loop status diverges from step-pointer derivation"
                                );
                            }
                        }
                    }
                }

                Ok(new_status.clone())
            }
        }
    }

    /// Persist the recipe snapshot and atomically derive + set the status column.
    ///
    /// This is the single choke point for snapshot persistence. Status is derived
    /// via [`derive_effective_status`] (override first, then step-kind derivation).
    /// If the derived status differs from the current DB status, a
    /// `status_transition` audit event is emitted.
    pub fn persist_snapshot(
        conn: &Connection,
        id: &str,
        snapshot: &RecipeSnapshot,
    ) -> SqlResult<()> {
        let json_val = serde_json::to_value(snapshot)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let json_str = json_val.to_string();
        let now = chrono::Utc::now().to_rfc3339();

        let derived = derive_effective_status(snapshot);

        let tx = conn.unchecked_transaction()?;

        if let Some(ref new_status) = derived {
            // Read current status for audit event
            let current_status_str: Option<String> = tx
                .query_row(
                    "SELECT status FROM loop_runs WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .ok();

            let executor_finished_at = if new_status.is_executor_terminal() {
                Some(now.clone())
            } else {
                None
            };
            let rows = tx.execute(
                "UPDATE loop_runs SET policy_json = ?1, status = ?2, updated_at = ?3, executor_finished_at = COALESCE(?4, executor_finished_at) WHERE id = ?5",
                params![json_str, new_status.as_str(), now, executor_finished_at, id],
            )?;
            if rows == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }

            // Emit audit event if status actually changed
            if current_status_str.as_deref() != Some(new_status.as_str()) {
                let payload = serde_json::json!({
                    "from": current_status_str.as_deref().unwrap_or("unknown"),
                    "to": new_status.as_str(),
                    "trigger": "step_derivation",
                });
                tx.execute(
                    "INSERT INTO loop_events (loop_id, ts, kind, payload_json) VALUES (?1, ?2, 'status_transition', ?3)",
                    params![id, now, payload.to_string()],
                )?;
            }
        } else {
            // Unrecognized step kind — this means a new step kind was added without
            // updating derive_status_from_step. Set Stale to make the problem visible
            // rather than silently retaining whatever status was there before (desync).
            tracing::warn!(
                loop_id = %id,
                current_step = %snapshot.runtime.current_step,
                "persist_snapshot: cannot derive status from step pointer (unknown step kind); \
                 marking loop as Stale to prevent silent desync"
            );
            let rows = tx.execute(
                "UPDATE loop_runs SET policy_json = ?1, status = 'stale', updated_at = ?2 WHERE id = ?3",
                params![json_str, now, id],
            )?;
            if rows == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Delete a loop run and all its child data (sessions link, events, artifacts, verifier_runs).
    /// Does NOT delete or archive the actual sessions — caller handles that separately if desired.
    /// Returns the list of session_ids that were linked to this loop (for optional cleanup).
    pub fn delete_loop(conn: &Connection, id: &str) -> SqlResult<Vec<String>> {
        // Collect linked session IDs before deleting
        let session_ids = Self::list_loop_sessions(conn, id)?
            .into_iter()
            .map(|s| s.session_id)
            .collect::<Vec<_>>();

        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM verifier_runs WHERE loop_id = ?1", params![id])?;
        tx.execute("DELETE FROM loop_artifacts WHERE loop_id = ?1", params![id])?;
        tx.execute("DELETE FROM loop_events WHERE loop_id = ?1", params![id])?;
        tx.execute("DELETE FROM loop_sessions WHERE loop_id = ?1", params![id])?;
        tx.execute("DELETE FROM loop_runs WHERE id = ?1", params![id])?;
        tx.commit()?;

        Ok(session_ids)
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

    /// Count events referencing a specific session since the given event ID.
    /// Uses the indexed `loop_events(loop_id, id)` for bounded performance.
    /// Excludes internal observation events (loop_heartbeat, loop_stale_detected)
    /// to prevent self-counting that would defeat stale detection.
    pub fn count_session_events_since(
        conn: &Connection,
        loop_id: &str,
        session_id: &str,
        after_event_id: i64,
    ) -> SqlResult<(u64, Option<i64>)> {
        // Count + get max event id in one query.
        // Uses json_extract for structural matching (more robust than LIKE).
        let mut stmt = conn.prepare(
            "SELECT COUNT(*), MAX(id) FROM loop_events \
             WHERE loop_id = ?1 AND id > ?2 \
             AND json_extract(payload_json, '$.session_id') = ?3 \
             AND kind NOT IN ('loop_heartbeat', 'loop_stale_detected')",
        )?;
        let result = stmt.query_row(params![loop_id, after_event_id, session_id], |row| {
            let count: i64 = row.get(0)?;
            let max_id: Option<i64> = row.get(1)?;
            Ok((count as u64, max_id))
        })?;
        Ok(result)
    }

    /// Get the latest event ID for a loop (for cursor seeding).
    pub fn latest_event_id(conn: &Connection, loop_id: &str) -> SqlResult<Option<i64>> {
        conn.query_row(
            "SELECT MAX(id) FROM loop_events WHERE loop_id = ?1",
            params![loop_id],
            |row| row.get(0),
        )
    }

    // ─── Artifacts ───────────────────────────────────────────────────────────

    pub fn add_artifact(conn: &Connection, params: AddArtifactParams) -> SqlResult<LoopArtifact> {
        // Enforce invariant: handoff artifacts must go through record_handoff
        // which performs atomic session status update + event append.
        if params.kind == "handoff" {
            return Err(rusqlite::Error::InvalidQuery);
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
        after_ts: Option<&str>,
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
        let after_clause = if after_ts.is_some() {
            format!("AND created_at > ?{}", session_ids.len() + 2)
        } else {
            String::new()
        };
        let sql = format!(
            "SELECT session_id, content_json FROM loop_artifacts \
             WHERE loop_id = ?1 AND kind = 'handoff' \
             AND session_id IN ({}) {} \
             ORDER BY created_at DESC, id DESC",
            placeholders.join(", "),
            after_clause
        );
        let mut stmt = conn.prepare(&sql)?;
        // Bind params: ?1 = loop_id, ?2..N = session_ids, ?N+1 = after_ts
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        params_vec.push(Box::new(loop_id.to_string()));
        for sid in session_ids {
            params_vec.push(Box::new(sid.clone()));
        }
        if let Some(ts) = after_ts {
            params_vec.push(Box::new(ts.to_string()));
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

        // 5. Apply loop transition if requested (via transition table)
        if let Some(trigger) = params.trigger {
            // Unchanged (no-op) is fine — e.g., HandoffReceived(Completed) from Observing.
            // Invalid transitions are logged but tolerated (race between handoff and cancel).
            // DB errors propagate — they indicate real infrastructure failure.
            match Self::transition_in_tx(&tx, &params.loop_id, trigger) {
                Ok(_) => {}
                Err(TransitionError::Invalid(inv)) => {
                    tracing::warn!(
                        loop_id = %params.loop_id,
                        from = %inv.from.as_str(),
                        trigger = ?inv.trigger,
                        "record_handoff: transition rejected (loop may have been cancelled concurrently)"
                    );
                }
                Err(TransitionError::Db(e)) => return Err(e),
                Err(TransitionError::NotFound(_)) => {
                    // Should not happen — we already validated the loop exists above.
                    // But if it does, propagate as DB error.
                    return Err(rusqlite::Error::QueryReturnedNoRows);
                }
                Err(TransitionError::CorruptStatus(_)) => {
                    return Err(rusqlite::Error::QueryReturnedNoRows);
                }
            }
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
            max_rounds: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
            executor_finished_at: row.get(10)?,
            policy_json: row
                .get::<_, Option<String>>(11)?
                .and_then(|s| serde_json::from_str(&s).ok()),
            budget_json: row
                .get::<_, Option<String>>(12)?
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
