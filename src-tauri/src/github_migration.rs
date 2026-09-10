use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const GITHUB_PLUGIN_ID: &str = "github";
const SNAPSHOT_FILE: &str = "legacy-github-pr-v1.json";
const STATE_VERSION: u64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GithubMigrationState {
    NotNeeded,
    Available,
    Importing,
    Failed,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GithubMigrationStatus {
    pub state: GithubMigrationState,
    pub legacy_detected: bool,
    pub can_migrate: bool,
    pub message: String,
    pub error: Option<String>,
    pub imported_pull_requests: usize,
    pub skipped_state_only: usize,
    pub snapshot_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MigrationRecord {
    state: GithubMigrationState,
    snapshot_path: Option<String>,
    snapshot_sha256: Option<String>,
    error: Option<String>,
    imported_pull_requests: usize,
    skipped_state_only: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LegacySnapshot {
    version: u32,
    created_at: String,
    created_at_ms: u64,
    pull_requests: Vec<LegacyPullRequest>,
    skipped_state_only: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LegacyPullRequest {
    session_id: String,
    branch: Option<String>,
    pr_url: String,
    legacy_state: Option<String>,
    updated_at: u64,
}

pub fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS github_plugin_migration (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            state TEXT NOT NULL,
            snapshot_path TEXT,
            snapshot_sha256 TEXT,
            error TEXT,
            imported_pull_requests INTEGER NOT NULL DEFAULT 0,
            skipped_state_only INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );",
    )
    .map_err(|error| format!("failed to initialize GitHub migration ledger: {error}"))
}

/// Freezes the legacy PR columns before any GitHub sidecar can use them. The
/// completed record deliberately survives until a later legacy-column retirement.
pub fn initialize(conn: &Connection, app_data_dir: &Path) -> Result<(), String> {
    migrate(conn)?;
    let record = read_record(conn)?;
    if matches!(
        record.as_ref().map(|record| &record.state),
        Some(GithubMigrationState::Completed | GithubMigrationState::NotNeeded)
    ) {
        return Ok(());
    }

    if matches!(
        record.as_ref().map(|record| &record.state),
        Some(GithubMigrationState::Importing)
    ) {
        let mut interrupted = record.expect("record was checked");
        interrupted.state = GithubMigrationState::Failed;
        interrupted.error = Some("PlaneAI stopped while GitHub migration was in progress. Retry migration to resume from the frozen backup.".to_string());
        return write_record(conn, &interrupted);
    }
    if record.is_some() {
        return Ok(());
    }

    let snapshot = snapshot_legacy(conn)?;
    if snapshot.pull_requests.is_empty() {
        return Ok(());
    }
    let path = snapshot_path(app_data_dir);
    write_json_atomically(&path, &snapshot, true)?;
    write_record(
        conn,
        &MigrationRecord {
            state: GithubMigrationState::Available,
            snapshot_path: Some(path.display().to_string()),
            snapshot_sha256: Some(snapshot_digest(&snapshot)?),
            error: None,
            imported_pull_requests: snapshot.pull_requests.len(),
            skipped_state_only: snapshot.skipped_state_only,
        },
    )
}

pub fn status(conn: &Connection) -> Result<GithubMigrationStatus, String> {
    migrate(conn)?;
    let record = read_record(conn)?;
    let live = snapshot_legacy(conn)?;
    let Some(record) = record else {
        return Ok(if live.pull_requests.is_empty() {
            GithubMigrationStatus {
                state: GithubMigrationState::NotNeeded,
                legacy_detected: false,
                can_migrate: false,
                message: if live.skipped_state_only > 0 {
                    "Legacy GitHub PR states without URLs were skipped safely; no PR mappings need migration.".to_string()
                } else {
                    "No legacy GitHub pull-request state was found.".to_string()
                },
                error: None,
                imported_pull_requests: 0,
                skipped_state_only: live.skipped_state_only,
                snapshot_path: None,
            }
        } else {
            GithubMigrationStatus {
                state: GithubMigrationState::Available,
                legacy_detected: true,
                can_migrate: true,
                message: "Legacy GitHub pull-request mappings are ready to migrate.".to_string(),
                error: None,
                imported_pull_requests: live.pull_requests.len(),
                skipped_state_only: live.skipped_state_only,
                snapshot_path: None,
            }
        });
    };
    let message = match record.state {
        GithubMigrationState::NotNeeded => "No legacy GitHub pull-request state was found.",
        GithubMigrationState::Available => {
            "Legacy GitHub pull-request mappings are ready to migrate."
        }
        GithubMigrationState::Importing => {
            "GitHub migration is in progress. Do not enable GitHub until it completes."
        }
        GithubMigrationState::Failed => {
            "GitHub migration stopped safely. Retry uses the frozen legacy backup."
        }
        GithubMigrationState::Completed => {
            "Legacy GitHub pull-request mappings were migrated to the GitHub plugin."
        }
    };
    Ok(GithubMigrationStatus {
        state: record.state.clone(),
        legacy_detected: !matches!(record.state, GithubMigrationState::NotNeeded),
        can_migrate: matches!(
            record.state,
            GithubMigrationState::Available | GithubMigrationState::Failed
        ),
        message: message.to_string(),
        error: record.error,
        imported_pull_requests: record.imported_pull_requests,
        skipped_state_only: record.skipped_state_only,
        snapshot_path: record.snapshot_path,
    })
}

pub fn blocks_plugin_start(conn: &Connection) -> bool {
    read_record(conn).ok().flatten().is_some_and(|record| {
        !matches!(
            record.state,
            GithubMigrationState::Completed | GithubMigrationState::NotNeeded
        )
    })
}

/// Performs filesystem work only. The caller must execute it off Tauri's main thread.
pub fn import(conn: &Connection, app_data_dir: &Path) -> Result<GithubMigrationStatus, String> {
    migrate(conn)?;
    let record = read_record(conn)?;
    match record.as_ref().map(|record| &record.state) {
        Some(GithubMigrationState::Completed) => return status(conn),
        Some(GithubMigrationState::Importing) => {
            return Err(
                "GitHub migration is already in progress. Restart PlaneAI if it was interrupted."
                    .to_string(),
            )
        }
        Some(GithubMigrationState::NotNeeded) => {
            return Err("no legacy GitHub migration is available".to_string())
        }
        Some(GithubMigrationState::Available | GithubMigrationState::Failed) | None => {}
    }

    let snapshot = match record
        .as_ref()
        .and_then(|record| record.snapshot_path.as_deref())
    {
        Some(path) => {
            let snapshot = read_snapshot(Path::new(path))?;
            if let Some(expected_digest) = record
                .as_ref()
                .and_then(|record| record.snapshot_sha256.as_deref())
            {
                let actual_digest = snapshot_digest(&snapshot)?;
                if actual_digest != expected_digest {
                    return Err("GitHub migration backup digest does not match the migration ledger; restore the frozen backup before retrying.".to_string());
                }
            }
            snapshot
        }
        None => {
            let snapshot = snapshot_legacy(conn)?;
            if snapshot.pull_requests.is_empty() {
                return status(conn);
            }
            let path = snapshot_path(app_data_dir);
            write_json_atomically(&path, &snapshot, true)?;
            snapshot
        }
    };
    validate_snapshot(&snapshot)?;
    write_record(
        conn,
        &record_for(
            &snapshot,
            app_data_dir,
            GithubMigrationState::Importing,
            None,
        )?,
    )?;

    if let Err(error) = import_snapshot(&snapshot, app_data_dir) {
        write_record(
            conn,
            &record_for(
                &snapshot,
                app_data_dir,
                GithubMigrationState::Failed,
                Some(error.clone()),
            )?,
        )?;
        return Err(error);
    }
    write_record(
        conn,
        &record_for(
            &snapshot,
            app_data_dir,
            GithubMigrationState::Completed,
            None,
        )?,
    )?;
    status(conn)
}

fn record_for(
    snapshot: &LegacySnapshot,
    app_data_dir: &Path,
    state: GithubMigrationState,
    error: Option<String>,
) -> Result<MigrationRecord, String> {
    Ok(MigrationRecord {
        state,
        snapshot_path: Some(snapshot_path(app_data_dir).display().to_string()),
        snapshot_sha256: Some(snapshot_digest(snapshot)?),
        error,
        imported_pull_requests: snapshot.pull_requests.len(),
        skipped_state_only: snapshot.skipped_state_only,
    })
}

fn snapshot_legacy(conn: &Connection) -> Result<LegacySnapshot, String> {
    let created_at = Utc::now();
    let created_at_ms = created_at.timestamp_millis().max(0) as u64;
    let mut statement = conn
        .prepare("SELECT id, branch, pr_url, pr_state, updated_at FROM sessions ORDER BY id")
        .map_err(|error| format!("failed to read legacy GitHub PR state: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|error| format!("failed to query legacy GitHub PR state: {error}"))?;
    let mut pull_requests = Vec::new();
    let mut skipped_state_only = 0;
    for row in rows {
        let (session_id, branch, pr_url, legacy_state, updated_at) =
            row.map_err(|error| format!("failed to decode legacy GitHub PR state: {error}"))?;
        let url = pr_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(url) = url {
            pull_requests.push(LegacyPullRequest {
                session_id,
                branch: branch.filter(|value| !value.trim().is_empty()),
                pr_url: url.to_string(),
                legacy_state: legacy_state.filter(|value| !value.trim().is_empty()),
                updated_at: parse_timestamp_ms(updated_at.as_deref()).unwrap_or(created_at_ms),
            });
        } else if legacy_state
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            skipped_state_only += 1;
        }
    }
    Ok(LegacySnapshot {
        version: 1,
        created_at: created_at.to_rfc3339(),
        created_at_ms,
        pull_requests,
        skipped_state_only,
    })
}

fn parse_timestamp_ms(value: Option<&str>) -> Option<u64> {
    DateTime::parse_from_rfc3339(value?)
        .ok()?
        .timestamp_millis()
        .try_into()
        .ok()
}

fn normalized_state(state: Option<&str>) -> &'static str {
    match state
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "draft" => "draft",
        "merged" | "merge" => "merged",
        "closed" | "close" => "closed",
        _ => "open",
    }
}

fn empty_durable_state() -> Value {
    serde_json::json!({
        "version": STATE_VERSION,
        "pull_requests": {},
        "reconciliation": {
            "status": "idle", "attempt_id": null, "started_at": null, "finished_at": null,
            "checked": 0, "recovered_attempt_id": null, "recovered_at": null, "error": null,
        }
    })
}

fn import_snapshot(snapshot: &LegacySnapshot, app_data_dir: &Path) -> Result<(), String> {
    let settings_path = plugin_data_dir(app_data_dir).join("settings.json");
    let existing = read_settings(&settings_path)?;
    let mut settings = existing
        .as_object()
        .cloned()
        .ok_or("existing plugin settings must be a JSON object")?;
    let mut state = settings
        .get(GITHUB_PLUGIN_ID)
        .cloned()
        .unwrap_or_else(empty_durable_state);
    validate_durable_state(&state)?;
    let mappings = state
        .get_mut("pull_requests")
        .and_then(Value::as_object_mut)
        .expect("validated state has mappings");
    for legacy in &snapshot.pull_requests {
        if let Some(existing) = mappings.get(&legacy.session_id) {
            let existing_url = existing
                .get("url")
                .and_then(Value::as_str)
                .expect("validated mapping has URL");
            if existing_url != legacy.pr_url {
                return Err(format!("existing GitHub pull-request mapping for session {:?} has a different URL; resolve plugin settings before retrying migration", legacy.session_id));
            }
            continue;
        }
        let mut mapping = Map::new();
        mapping.insert(
            "session_id".to_string(),
            Value::String(legacy.session_id.clone()),
        );
        mapping.insert("url".to_string(), Value::String(legacy.pr_url.clone()));
        mapping.insert(
            "state".to_string(),
            Value::String(normalized_state(legacy.legacy_state.as_deref()).to_string()),
        );
        if let Some(branch) = &legacy.branch {
            mapping.insert("branch".to_string(), Value::String(branch.clone()));
        }
        mapping.insert(
            "updated_at".to_string(),
            Value::Number(legacy.updated_at.into()),
        );
        mappings.insert(legacy.session_id.clone(), Value::Object(mapping));
    }
    validate_durable_state(&state)?;
    settings.insert(GITHUB_PLUGIN_ID.to_string(), state);
    let settings = Value::Object(settings);
    write_json_atomically(&settings_path, &settings, false)?;
    let written = read_settings(&settings_path)?;
    validate_durable_state(
        written
            .get(GITHUB_PLUGIN_ID)
            .ok_or("written GitHub settings are missing the github namespace")?,
    )?;
    Ok(())
}

fn validate_snapshot(snapshot: &LegacySnapshot) -> Result<(), String> {
    if snapshot.version != 1 {
        return Err("unsupported legacy GitHub migration snapshot".to_string());
    }
    for mapping in &snapshot.pull_requests {
        if mapping.session_id.trim().is_empty() || mapping.pr_url.trim().is_empty() {
            return Err(
                "legacy GitHub migration snapshot contains an unusable pull-request mapping"
                    .to_string(),
            );
        }
        if mapping
            .branch
            .as_deref()
            .is_some_and(|branch| branch.trim().is_empty())
        {
            return Err("legacy GitHub migration snapshot contains an empty branch".to_string());
        }
    }
    Ok(())
}

fn validate_durable_state(state: &Value) -> Result<(), String> {
    let object = state
        .as_object()
        .ok_or("GitHub durable state must be a JSON object")?;
    require_keys(object, &["version", "pull_requests", "reconciliation"])?;
    if object.get("version").and_then(Value::as_u64) != Some(STATE_VERSION) {
        return Err("unsupported GitHub durable state version".to_string());
    }
    let mappings = object
        .get("pull_requests")
        .and_then(Value::as_object)
        .ok_or("GitHub durable state pull_requests must be an object")?;
    for (session_id, mapping) in mappings {
        if session_id.trim().is_empty() {
            return Err("GitHub durable state has an empty session id".to_string());
        }
        let mapping = mapping
            .as_object()
            .ok_or("GitHub durable state pull request must be an object")?;
        if mapping.keys().any(|key| {
            ![
                "session_id",
                "url",
                "state",
                "remote",
                "branch",
                "updated_at",
            ]
            .contains(&key.as_str())
        }) {
            return Err("GitHub durable state pull request contains unknown fields".to_string());
        }
        if strict_string(mapping.get("session_id")).as_deref() != Some(session_id)
            || strict_string(mapping.get("url")).is_none()
            || strict_string(mapping.get("state")).is_none()
            || mapping.get("updated_at").and_then(Value::as_u64).is_none()
        {
            return Err("GitHub durable state has an invalid pull request mapping".to_string());
        }
        for field in ["remote", "branch"] {
            if let Some(value) = mapping.get(field) {
                if strict_string(Some(value)).is_none() {
                    return Err(format!(
                        "GitHub durable state has invalid pull request {field}"
                    ));
                }
            }
        }
    }
    let reconciliation = object
        .get("reconciliation")
        .and_then(Value::as_object)
        .ok_or("GitHub durable state reconciliation must be an object")?;
    require_keys(
        reconciliation,
        &[
            "status",
            "attempt_id",
            "started_at",
            "finished_at",
            "checked",
            "recovered_attempt_id",
            "recovered_at",
            "error",
        ],
    )?;
    if !matches!(
        reconciliation.get("status").and_then(Value::as_str),
        Some("idle" | "running" | "failed")
    ) || reconciliation
        .get("checked")
        .and_then(Value::as_u64)
        .is_none()
    {
        return Err("GitHub durable state has invalid reconciliation status".to_string());
    }
    for field in ["attempt_id", "recovered_attempt_id", "error"] {
        if !reconciliation.get(field).is_some_and(|value| {
            matches!(value, Value::Null) || strict_string(Some(value)).is_some()
        }) {
            return Err(format!("GitHub durable state has invalid {field}"));
        }
    }
    for field in ["started_at", "finished_at", "recovered_at"] {
        if !reconciliation
            .get(field)
            .is_some_and(|value| matches!(value, Value::Null) || value.as_u64().is_some())
        {
            return Err(format!("GitHub durable state has invalid {field}"));
        }
    }
    Ok(())
}

fn require_keys(object: &Map<String, Value>, required: &[&str]) -> Result<(), String> {
    if object.keys().any(|key| !required.contains(&key.as_str()))
        || required.iter().any(|key| !object.contains_key(*key))
    {
        return Err(
            "GitHub durable state is missing required fields or contains unknown fields"
                .to_string(),
        );
    }
    Ok(())
}

fn strict_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn read_settings(path: &Path) -> Result<Value, String> {
    match std::fs::File::open(path) {
        Ok(file) => {
            let value: Value = serde_json::from_reader(file).map_err(|error| {
                format!("failed to parse existing GitHub plugin settings: {error}")
            })?;
            if value.is_object() {
                Ok(value)
            } else {
                Err("existing plugin settings must be a JSON object".to_string())
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(error) => Err(format!(
            "failed to read existing GitHub plugin settings: {error}"
        )),
    }
}

fn snapshot_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("github-migration").join(SNAPSHOT_FILE)
}
fn plugin_data_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir
        .join("plugins")
        .join("state")
        .join(GITHUB_PLUGIN_ID)
        .join("data")
}
fn read_snapshot(path: &Path) -> Result<LegacySnapshot, String> {
    serde_json::from_reader(
        std::fs::File::open(path)
            .map_err(|error| format!("failed to read GitHub migration backup: {error}"))?,
    )
    .map_err(|error| format!("failed to parse GitHub migration backup: {error}"))
}
fn snapshot_digest(snapshot: &LegacySnapshot) -> Result<String, String> {
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(snapshot).map_err(|error| error.to_string())?);
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn write_json_atomically<T: Serialize>(
    path: &Path,
    value: &T,
    private: bool,
) -> Result<(), String> {
    let parent = path.parent().ok_or("missing migration file parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if private {
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    #[cfg(unix)]
    if private {
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    drop(file);
    planeai_paths::replace_file_atomically(&temporary, path).map_err(|error| error.to_string())
}

fn read_record(conn: &Connection) -> Result<Option<MigrationRecord>, String> {
    conn.query_row("SELECT state, snapshot_path, snapshot_sha256, error, imported_pull_requests, skipped_state_only FROM github_plugin_migration WHERE id = 1", [], |row| {
        let state: String = row.get(0)?;
        let state = match state.as_str() { "not_needed" => GithubMigrationState::NotNeeded, "available" => GithubMigrationState::Available, "importing" => GithubMigrationState::Importing, "failed" => GithubMigrationState::Failed, "completed" => GithubMigrationState::Completed, _ => GithubMigrationState::Failed };
        Ok(MigrationRecord { state, snapshot_path: row.get(1)?, snapshot_sha256: row.get(2)?, error: row.get(3)?, imported_pull_requests: row.get::<_, i64>(4)? as usize, skipped_state_only: row.get::<_, i64>(5)? as usize })
    }).optional().map_err(|error| format!("failed to read GitHub migration ledger: {error}"))
}

fn write_record(conn: &Connection, record: &MigrationRecord) -> Result<(), String> {
    let state = match record.state {
        GithubMigrationState::NotNeeded => "not_needed",
        GithubMigrationState::Available => "available",
        GithubMigrationState::Importing => "importing",
        GithubMigrationState::Failed => "failed",
        GithubMigrationState::Completed => "completed",
    };
    conn.execute("INSERT INTO github_plugin_migration (id, state, snapshot_path, snapshot_sha256, error, imported_pull_requests, skipped_state_only, updated_at) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP) ON CONFLICT(id) DO UPDATE SET state = excluded.state, snapshot_path = excluded.snapshot_path, snapshot_sha256 = excluded.snapshot_sha256, error = excluded.error, imported_pull_requests = excluded.imported_pull_requests, skipped_state_only = excluded.skipped_state_only, updated_at = CURRENT_TIMESTAMP", params![state, record.snapshot_path, record.snapshot_sha256, record.error, record.imported_pull_requests as i64, record.skipped_state_only as i64]).map_err(|error| format!("failed to write GitHub migration ledger: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path) VALUES ('p1', 'Project', '/tmp')",
            [],
        )
        .unwrap();
        conn
    }
    fn session(conn: &Connection, id: &str, url: Option<&str>, state: Option<&str>) {
        conn.execute("INSERT INTO sessions (id, project_id, branch, status, created_at, pr_url, pr_state, updated_at) VALUES (?1, 'p1', 'topic', 'active', '2024-01-01T00:00:00Z', ?2, ?3, '2024-01-02T03:04:05Z')", params![id, url, state]).unwrap();
    }
    fn settings_path(temp: &tempfile::TempDir) -> PathBuf {
        plugin_data_dir(&temp.path().join("data")).join("settings.json")
    }

    #[test]
    fn no_legacy_state_is_not_needed() {
        let conn = legacy_db();
        let temp = tempfile::tempdir().unwrap();
        initialize(&conn, &temp.path().join("data")).unwrap();
        assert_eq!(
            status(&conn).unwrap().state,
            GithubMigrationState::NotNeeded
        );
        assert!(!blocks_plugin_start(&conn));
    }
    #[test]
    fn detection_fences_github_and_skips_state_only_rows() {
        let conn = legacy_db();
        let temp = tempfile::tempdir().unwrap();
        session(
            &conn,
            "with-url",
            Some("https://github.com/o/r/pull/1"),
            None,
        );
        session(&conn, "state-only", None, Some("merged"));
        initialize(&conn, &temp.path().join("data")).unwrap();
        let current = status(&conn).unwrap();
        assert_eq!(current.state, GithubMigrationState::Available);
        assert_eq!(current.skipped_state_only, 1);
        assert!(blocks_plugin_start(&conn));
    }
    #[test]
    fn imports_empty_settings_with_strict_v1_shape() {
        let conn = legacy_db();
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        session(&conn, "s1", Some("https://github.com/o/r/pull/1"), None);
        initialize(&conn, &data).unwrap();
        assert_eq!(
            import(&conn, &data).unwrap().state,
            GithubMigrationState::Completed
        );
        let settings = read_settings(&settings_path(&temp)).unwrap();
        let mapping = &settings["github"]["pull_requests"]["s1"];
        assert_eq!(settings["github"]["version"], 1);
        assert_eq!(mapping["url"], "https://github.com/o/r/pull/1");
        assert_eq!(mapping["state"], "open");
        assert_eq!(mapping["branch"], "topic");
        assert!(mapping["updated_at"].as_u64().is_some());
        assert_eq!(normalized_state(Some("draft")), "draft");
        validate_durable_state(&settings["github"]).unwrap();
    }
    #[test]
    fn preserves_compatible_mappings_reconciliation_and_unrelated_settings() {
        let conn = legacy_db();
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        session(
            &conn,
            "s1",
            Some("https://github.com/o/r/pull/1"),
            Some("merged"),
        );
        initialize(&conn, &data).unwrap();
        let mut github = empty_durable_state();
        github["pull_requests"]["s1"] = serde_json::json!({"session_id":"s1","url":"https://github.com/o/r/pull/1","state":"closed","updated_at":99});
        github["reconciliation"]["status"] = serde_json::json!("failed");
        github["reconciliation"]["error"] = serde_json::json!("keep");
        write_json_atomically(
            &settings_path(&temp),
            &serde_json::json!({"other": {"keep": true}, "github": github}),
            false,
        )
        .unwrap();
        import(&conn, &data).unwrap();
        let settings = read_settings(&settings_path(&temp)).unwrap();
        assert_eq!(settings["other"]["keep"], true);
        assert_eq!(settings["github"]["pull_requests"]["s1"]["state"], "closed");
        assert_eq!(settings["github"]["reconciliation"]["error"], "keep");
    }
    #[test]
    fn different_existing_url_is_a_clear_conflict() {
        let conn = legacy_db();
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        session(&conn, "s1", Some("https://github.com/o/r/pull/1"), None);
        initialize(&conn, &data).unwrap();
        let mut github = empty_durable_state();
        github["pull_requests"]["s1"] = serde_json::json!({"session_id":"s1","url":"https://github.com/o/r/pull/2","state":"open","updated_at":1});
        write_json_atomically(
            &settings_path(&temp),
            &serde_json::json!({"github": github}),
            false,
        )
        .unwrap();
        assert!(import(&conn, &data).unwrap_err().contains("different URL"));
        assert_eq!(status(&conn).unwrap().state, GithubMigrationState::Failed);
    }
    #[test]
    fn retries_idempotently_from_frozen_snapshot() {
        let conn = legacy_db();
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        session(
            &conn,
            "s1",
            Some("https://github.com/o/r/pull/1"),
            Some("open"),
        );
        initialize(&conn, &data).unwrap();
        let backup = std::fs::read(snapshot_path(&data)).unwrap();
        assert_eq!(
            import(&conn, &data).unwrap().state,
            GithubMigrationState::Completed
        );
        conn.execute(
            "UPDATE sessions SET pr_url = 'https://github.com/o/r/pull/2' WHERE id = 's1'",
            [],
        )
        .unwrap();
        assert_eq!(
            import(&conn, &data).unwrap().state,
            GithubMigrationState::Completed
        );
        assert_eq!(std::fs::read(snapshot_path(&data)).unwrap(), backup);
    }
    #[test]
    fn interrupted_import_recovers_to_retryable_failed_fence() {
        let conn = legacy_db();
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        session(&conn, "s1", Some("https://github.com/o/r/pull/1"), None);
        initialize(&conn, &data).unwrap();
        let snapshot = read_snapshot(&snapshot_path(&data)).unwrap();
        write_record(
            &conn,
            &record_for(&snapshot, &data, GithubMigrationState::Importing, None).unwrap(),
        )
        .unwrap();
        initialize(&conn, &data).unwrap();
        assert_eq!(status(&conn).unwrap().state, GithubMigrationState::Failed);
        assert!(blocks_plugin_start(&conn));
        assert_eq!(
            import(&conn, &data).unwrap().state,
            GithubMigrationState::Completed
        );
    }
}
