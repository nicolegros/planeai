use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::config::{self, Config};

const JIRA_PLUGIN_ID: &str = "jira";
const SNAPSHOT_FILE: &str = "legacy-jira-v1.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JiraMigrationState {
    NotNeeded,
    Available,
    Importing,
    Failed,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraMigrationStatus {
    pub state: JiraMigrationState,
    pub legacy_detected: bool,
    pub can_migrate: bool,
    pub message: String,
    pub error: Option<String>,
    pub imported_issues: usize,
    pub imported_links: usize,
    pub snapshot_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MigrationRecord {
    state: JiraMigrationState,
    snapshot_path: Option<String>,
    snapshot_sha256: Option<String>,
    error: Option<String>,
    imported_issues: usize,
    imported_links: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacySnapshot {
    version: u32,
    created_at: String,
    settings: Value,
    credentials: Option<LegacyCredentials>,
    issues: Vec<LegacyIssue>,
    links: Vec<LegacyLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LegacyCredentials {
    refresh_token: String,
    cloud_id: String,
    site: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LegacyIssue {
    issue_key: String,
    summary: String,
    description: String,
    jira_status: String,
    mapped_status: String,
    priority: Option<String>,
    labels: String,
    sync_status: String,
    last_synced_at: String,
    source_name: String,
    task_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LegacyLink {
    task_key: String,
    issue_key: String,
}

pub fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS jira_plugin_migration (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            state TEXT NOT NULL,
            snapshot_path TEXT,
            snapshot_sha256 TEXT,
            error TEXT,
            imported_issues INTEGER NOT NULL DEFAULT 0,
            imported_links INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );",
    )
    .map_err(|error| format!("failed to initialize Jira migration ledger: {error}"))
}

/// Called before enabled plugin processes are revived. A profile with legacy Jira
/// state is deliberately kept offline until the user performs the explicit import.
pub fn initialize(conn: &Connection, config: &Config) -> Result<(), String> {
    migrate(conn)?;
    let record = read_record(conn)?;
    let legacy_detected = legacy_settings(config).is_some();
    let incomplete = record.as_ref().is_some_and(|record| {
        !matches!(
            record.state,
            JiraMigrationState::Completed | JiraMigrationState::NotNeeded
        )
    });
    if legacy_detected || incomplete {
        write_record(
            conn,
            &MigrationRecord {
                state: match record.as_ref().map(|record| &record.state) {
                    Some(JiraMigrationState::Failed) => JiraMigrationState::Failed,
                    Some(JiraMigrationState::Importing) => JiraMigrationState::Failed,
                    _ => JiraMigrationState::Available,
                },
                snapshot_path: record.as_ref().and_then(|record| record.snapshot_path.clone()),
                snapshot_sha256: record.as_ref().and_then(|record| record.snapshot_sha256.clone()),
                error: matches!(record.as_ref().map(|record| &record.state), Some(JiraMigrationState::Importing))
                    .then(|| "PlaneAI stopped while Jira migration was in progress. Retry migration to resume from the frozen backup.".to_string())
                    .or_else(|| record.as_ref().and_then(|record| record.error.clone())),
                imported_issues: record.as_ref().map_or(0, |record| record.imported_issues),
                imported_links: record.as_ref().map_or(0, |record| record.imported_links),
            },
        )?;
        conn.execute(
            "UPDATE plugin_inventory
             SET enabled = 0, runtime_state = 'disabled',
                 last_error = 'Jira is waiting for explicit legacy migration',
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
            [JIRA_PLUGIN_ID],
        )
        .map_err(|error| format!("failed to fence Jira plugin during migration: {error}"))?;
    }
    Ok(())
}

pub fn status(conn: &Connection, config: &Config) -> Result<JiraMigrationStatus, String> {
    migrate(conn)?;
    let record = read_record(conn)?;
    let legacy_detected = legacy_settings(config).is_some();
    let Some(record) = record else {
        return Ok(if legacy_detected {
            JiraMigrationStatus {
                state: JiraMigrationState::Available,
                legacy_detected: true,
                can_migrate: true,
                message: "Legacy Jira data is ready to migrate into the bundled Jira plugin."
                    .to_string(),
                error: None,
                imported_issues: 0,
                imported_links: 0,
                snapshot_path: None,
            }
        } else {
            JiraMigrationStatus {
                state: JiraMigrationState::NotNeeded,
                legacy_detected: false,
                can_migrate: false,
                message: "No legacy Jira state was found.".to_string(),
                error: None,
                imported_issues: 0,
                imported_links: 0,
                snapshot_path: None,
            }
        });
    };
    let message = match record.state {
        JiraMigrationState::Available => {
            "Legacy Jira data is ready to migrate into the bundled Jira plugin."
        }
        JiraMigrationState::Importing => {
            "Jira migration is being prepared. Do not enable Jira until it completes."
        }
        JiraMigrationState::Failed => {
            "Jira migration stopped safely. Retry uses the frozen legacy backup."
        }
        JiraMigrationState::Completed => "Legacy Jira was migrated to the bundled plugin.",
        JiraMigrationState::NotNeeded => "No legacy Jira state was found.",
    };
    Ok(JiraMigrationStatus {
        state: record.state.clone(),
        legacy_detected,
        can_migrate: matches!(
            record.state,
            JiraMigrationState::Available | JiraMigrationState::Failed
        ),
        message: message.to_string(),
        error: record.error,
        imported_issues: record.imported_issues,
        imported_links: record.imported_links,
        snapshot_path: record.snapshot_path,
    })
}

pub fn blocks_plugin_start(conn: &Connection) -> bool {
    read_record(conn).ok().flatten().is_some_and(|record| {
        !matches!(
            record.state,
            JiraMigrationState::Completed | JiraMigrationState::NotNeeded
        )
    })
}

/// Performs only synchronous filesystem/SQLite work. The caller must run this
/// off Tauri's main thread and enable the sidecar only after this returns.
pub fn import(
    conn: &Connection,
    config_dir: &Path,
    app_data_dir: &Path,
    config: &mut Config,
) -> Result<JiraMigrationStatus, String> {
    migrate(conn)?;
    let record = read_record(conn)?;
    match record.as_ref().map(|record| &record.state) {
        Some(JiraMigrationState::Completed) => return status(conn, config),
        Some(JiraMigrationState::Importing) => {
            return Err("Jira migration is already in progress. Wait for it to finish or restart PlaneAI if it was interrupted.".to_string());
        }
        Some(JiraMigrationState::NotNeeded) => {
            return Err("no legacy Jira migration is available".to_string());
        }
        Some(JiraMigrationState::Available | JiraMigrationState::Failed) | None => {}
    }

    let snapshot = match record
        .as_ref()
        .and_then(|record| record.snapshot_path.as_deref())
    {
        Some(path) => read_snapshot(Path::new(path))?,
        None => {
            let snapshot = snapshot_legacy(conn, config, app_data_dir)?;
            let path = snapshot_path(app_data_dir);
            write_snapshot(&path, &snapshot)?;
            let digest = snapshot_digest(&snapshot)?;
            write_record(
                conn,
                &MigrationRecord {
                    state: JiraMigrationState::Available,
                    snapshot_path: Some(path.display().to_string()),
                    snapshot_sha256: Some(digest),
                    error: None,
                    imported_issues: 0,
                    imported_links: 0,
                },
            )?;
            snapshot
        }
    };

    write_record(
        conn,
        &MigrationRecord {
            state: JiraMigrationState::Importing,
            snapshot_path: Some(snapshot_path(app_data_dir).display().to_string()),
            snapshot_sha256: Some(snapshot_digest(&snapshot)?),
            error: None,
            imported_issues: snapshot.issues.len(),
            imported_links: snapshot.links.len(),
        },
    )?;

    if let Err(error) = import_snapshot(&snapshot, app_data_dir) {
        record_failure(conn, app_data_dir, &snapshot, &error)?;
        return Err(error);
    }

    // The backup remains durable. Removing legacy config only happens after the
    // plugin namespace has been written and validated successfully.
    config.integrations = None;
    if let Err(error) = config::save(config_dir, config) {
        record_failure(conn, app_data_dir, &snapshot, &error)?;
        return Err(error);
    }
    status(conn, config)
}

pub fn mark_completed(conn: &Connection, app_data_dir: &Path) -> Result<(), String> {
    let snapshot = read_snapshot(&snapshot_path(app_data_dir))?;
    write_record(
        conn,
        &MigrationRecord {
            state: JiraMigrationState::Completed,
            snapshot_path: Some(snapshot_path(app_data_dir).display().to_string()),
            snapshot_sha256: Some(snapshot_digest(&snapshot)?),
            error: None,
            imported_issues: snapshot.issues.len(),
            imported_links: snapshot.links.len(),
        },
    )
}

pub fn mark_failed(conn: &Connection, error: &str) -> Result<(), String> {
    let mut record = read_record(conn)?.unwrap_or(MigrationRecord {
        state: JiraMigrationState::Failed,
        snapshot_path: None,
        snapshot_sha256: None,
        error: None,
        imported_issues: 0,
        imported_links: 0,
    });
    record.state = JiraMigrationState::Failed;
    record.error = Some(error.to_string());
    write_record(conn, &record)
}

fn legacy_settings(config: &Config) -> Option<Value> {
    config
        .integrations
        .as_ref()?
        .jira
        .as_ref()
        .and_then(|jira| serde_json::to_value(jira).ok())
}

fn normalize_legacy_source_name(
    source_name: &str,
    sources: &HashMap<String, Value>,
) -> Result<String, rusqlite::Error> {
    if !source_name.trim().is_empty() {
        return Ok(source_name.to_string());
    }
    match sources.len() {
        1 => Ok(sources
            .keys()
            .next()
            .expect("one source was checked")
            .to_string()),
        _ => Err(rusqlite::Error::InvalidParameterName(
            "legacy Jira issue has no source_name and its profile has multiple sources".to_string(),
        )),
    }
}

fn snapshot_legacy(
    conn: &Connection,
    config: &Config,
    app_data_dir: &Path,
) -> Result<LegacySnapshot, String> {
    let settings = legacy_settings(config).ok_or("no legacy Jira configuration is available")?;
    let sources: HashMap<String, Value> = settings
        .get("sources")
        .and_then(Value::as_object)
        .ok_or("legacy Jira sources must be an object")?
        .iter()
        .map(|(name, source)| Ok((name.clone(), source.clone())))
        .collect::<Result<_, String>>()?;
    let credentials = read_legacy_credentials(
        app_data_dir,
        settings
            .get("site")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )?;
    let mut statement = conn
        .prepare("SELECT issue_key, summary, description, status, priority, labels, sync_status, last_synced_at, source_name FROM jira_issues")
        .map_err(|error| format!("failed to read legacy Jira issues: {error}"))?;
    let issues = statement
        .query_map([], |row| {
            let issue_key: String = row.get(0)?;
            let jira_status: String = row.get(3)?;
            let stored_source_name: String = row.get::<_, Option<String>>(8)?.unwrap_or_default();
            let source_name = normalize_legacy_source_name(&stored_source_name, &sources)?;
            let task_status = conn
                .query_row(
                    "SELECT status FROM tasks WHERE key = ?1",
                    [&issue_key],
                    |task| task.get(0),
                )
                .optional()
                .unwrap_or(None);
            let mapped_status = task_status
                .clone()
                .or_else(|| {
                    sources
                        .get(&source_name)
                        .and_then(|source| source.get("status_map"))
                        .and_then(Value::as_object)
                        .and_then(|map| map.get(&jira_status))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "todo".to_string());
            Ok(LegacyIssue {
                issue_key,
                summary: row.get(1)?,
                description: row.get(2)?,
                jira_status,
                mapped_status,
                priority: row.get(4)?,
                labels: row.get(5)?,
                sync_status: row.get(6)?,
                last_synced_at: row.get(7)?,
                source_name,
                task_status,
            })
        })
        .map_err(|error| format!("failed to query legacy Jira issues: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to decode legacy Jira issue: {error}"))?;
    let mut statement = conn
        .prepare("SELECT task_key, issue_key FROM jira_task_links")
        .map_err(|error| format!("failed to read legacy Jira task links: {error}"))?;
    let links = statement
        .query_map([], |row| {
            Ok(LegacyLink {
                task_key: row.get(0)?,
                issue_key: row.get(1)?,
            })
        })
        .map_err(|error| format!("failed to query legacy Jira links: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to decode legacy Jira link: {error}"))?;
    Ok(LegacySnapshot {
        version: 1,
        created_at: Utc::now().to_rfc3339(),
        settings,
        credentials,
        issues,
        links,
    })
}

fn read_legacy_credentials(
    app_data_dir: &Path,
    site: &str,
) -> Result<Option<LegacyCredentials>, String> {
    let directory = app_data_dir.join("jira-tokens");
    let refresh = read_optional_trimmed(directory.join("refresh_token"))?;
    let cloud_id = read_optional_trimmed(directory.join("cloud_id"))?;
    match (refresh, cloud_id) {
        (None, None) => Ok(None),
        (Some(refresh_token), Some(cloud_id)) => Ok(Some(LegacyCredentials {
            refresh_token,
            cloud_id,
            site: site.to_string(),
        })),
        _ => Err(
            "legacy Jira credentials are incomplete; both refresh_token and cloud_id are required"
                .to_string(),
        ),
    }
}

fn read_optional_trimmed(path: PathBuf) -> Result<Option<String>, String> {
    match std::fs::read_to_string(&path) {
        Ok(value) => Ok(Some(value.trim().to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("failed to read {}: {error}", path.display())),
    }
}

fn merge_plugin_settings(existing: Option<Value>, legacy: &Value) -> Result<Value, String> {
    let legacy = legacy
        .as_object()
        .ok_or("legacy Jira settings must be an object")?;
    let mut merged = existing
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
        .as_object()
        .cloned()
        .ok_or("existing Jira plugin settings must be an object")?;
    let legacy_sources = legacy
        .get("sources")
        .and_then(Value::as_object)
        .ok_or("legacy Jira sources must be an object")?;
    let mut sources = merged
        .remove("sources")
        .map(|value| {
            value
                .as_object()
                .cloned()
                .ok_or("existing Jira plugin sources must be an object")
        })
        .transpose()?
        .unwrap_or_default();
    for (name, source) in legacy_sources {
        match sources.get(name) {
            Some(existing) if existing != source => return Err(format!("existing Jira plugin source {name:?} conflicts with the legacy source; resolve the plugin state before retrying migration")),
            Some(_) => {}
            None => { sources.insert(name.clone(), source.clone()); }
        }
    }
    for (key, value) in legacy {
        if key != "sources" {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged.insert("sources".to_string(), Value::Object(sources));
    Ok(Value::Object(merged))
}

fn validate_existing_credentials(
    path: &Path,
    legacy: Option<&LegacyCredentials>,
) -> Result<(), String> {
    let existing = match std::fs::File::open(path) {
        Ok(file) => Some(
            serde_json::from_reader::<_, LegacyCredentials>(file).map_err(|error| {
                format!("failed to parse existing Jira plugin credentials: {error}")
            })?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "failed to read existing Jira plugin credentials: {error}"
            ))
        }
    };
    if let (Some(existing), Some(legacy)) = (existing, legacy) {
        if existing != *legacy {
            return Err("existing Jira plugin credentials conflict with the legacy cloud identity; resolve the plugin state before retrying migration".to_string());
        }
    }
    Ok(())
}
fn import_snapshot(snapshot: &LegacySnapshot, app_data_dir: &Path) -> Result<(), String> {
    validate_snapshot(snapshot)?;
    let root = plugin_state_root(app_data_dir);
    let data_dir = root.join("data");
    let secrets_dir = root.join("secrets");
    std::fs::create_dir_all(&data_dir)
        .map_err(|error| format!("failed to create Jira plugin data directory: {error}"))?;
    std::fs::create_dir_all(&secrets_dir)
        .map_err(|error| format!("failed to create Jira plugin secrets directory: {error}"))?;
    #[cfg(unix)]
    std::fs::set_permissions(&secrets_dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let settings_path = data_dir.join("settings.json");
    let existing_settings =
        match std::fs::File::open(&settings_path) {
            Ok(file) => Some(serde_json::from_reader(file).map_err(|error| {
                format!("failed to parse existing Jira plugin settings: {error}")
            })?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(format!(
                    "failed to read existing Jira plugin settings: {error}"
                ))
            }
        };
    let merged_settings = merge_plugin_settings(existing_settings, &snapshot.settings)?;
    let credentials_path = secrets_dir.join("credentials.json");
    validate_existing_credentials(&credentials_path, snapshot.credentials.as_ref())?;
    write_json_atomically(&settings_path, &merged_settings, false)?;
    if let Some(credentials) = &snapshot.credentials {
        write_json_atomically(&credentials_path, credentials, true)?;
    }

    let mut target = Connection::open(data_dir.join("jira.sqlite"))
        .map_err(|error| format!("failed to open Jira plugin database: {error}"))?;
    migrate_plugin_database(&target)?;
    let transaction = target
        .transaction()
        .map_err(|error| format!("failed to begin Jira plugin import: {error}"))?;
    for issue in &snapshot.issues {
        transaction.execute(
            "INSERT OR IGNORE INTO jira_issues (issue_key, summary, description, jira_status, mapped_status, priority, labels, source_name, sync_status, last_synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![issue.issue_key, issue.summary, issue.description, issue.jira_status, issue.mapped_status, issue.priority, issue.labels, issue.source_name, issue.sync_status, issue.last_synced_at],
        ).map_err(|error| format!("failed to import Jira issue {}: {error}", issue.issue_key))?;
        transaction.execute(
            "INSERT OR IGNORE INTO jira_issue_sources (issue_key, source_name, sync_status, departure_prompt_eligible) VALUES (?1, ?2, ?3, ?4)",
            params![issue.issue_key, issue.source_name, issue.sync_status, i64::from(issue.sync_status == "departed")],
        ).map_err(|error| format!("failed to import Jira source state {}: {error}", issue.issue_key))?;
        if issue.sync_status == "departed" && issue.task_status.as_deref() != Some("done") {
            transaction.execute(
                "INSERT OR IGNORE INTO jira_departure_queue (issue_key, summary, queued_at) VALUES (?1, ?2, ?3)",
                params![issue.issue_key, issue.summary, snapshot.created_at],
            ).map_err(|error| format!("failed to import Jira departure {}: {error}", issue.issue_key))?;
        }
    }
    for link in &snapshot.links {
        transaction
            .execute(
                "INSERT OR IGNORE INTO jira_task_links (task_key, issue_key) VALUES (?1, ?2)",
                params![link.task_key, link.issue_key],
            )
            .map_err(|error| {
                format!("failed to import Jira task link {}: {error}", link.task_key)
            })?;
    }
    if let Some(credentials) = &snapshot.credentials {
        transaction
            .execute(
                "INSERT OR IGNORE INTO jira_cache_metadata (key, value) VALUES ('cloud_id', ?1)",
                [credentials.cloud_id.as_str()],
            )
            .map_err(|error| format!("failed to import Jira cloud identity: {error}"))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("failed to commit Jira plugin import: {error}"))?;
    validate_import(snapshot, &data_dir, &secrets_dir, &merged_settings)
}

fn validate_snapshot(snapshot: &LegacySnapshot) -> Result<(), String> {
    if snapshot.version != 1 || !snapshot.settings.is_object() {
        return Err("unsupported legacy Jira migration snapshot".to_string());
    }
    let sources = snapshot
        .settings
        .get("sources")
        .and_then(Value::as_object)
        .ok_or("legacy Jira sources must be an object")?;
    for issue in &snapshot.issues {
        if !sources.contains_key(&issue.source_name) {
            return Err(format!(
                "legacy Jira issue {} refers to unknown source {:?}",
                issue.issue_key, issue.source_name
            ));
        }
        if !matches!(issue.sync_status.as_str(), "synced" | "departed") {
            return Err(format!(
                "legacy Jira issue {} has invalid sync status",
                issue.issue_key
            ));
        }
    }
    Ok(())
}

fn validate_import(
    snapshot: &LegacySnapshot,
    data_dir: &Path,
    secrets_dir: &Path,
    expected_settings: &Value,
) -> Result<(), String> {
    let settings: Value = serde_json::from_reader(
        std::fs::File::open(data_dir.join("settings.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("failed to validate Jira plugin settings: {error}"))?;
    if &settings != expected_settings {
        return Err("Jira plugin settings did not match the validated import target".to_string());
    }
    let conn = Connection::open(data_dir.join("jira.sqlite")).map_err(|error| error.to_string())?;
    for issue in &snapshot.issues {
        let actual = conn.query_row(
            "SELECT summary, description, jira_status, mapped_status, priority, labels, source_name, sync_status, last_synced_at FROM jira_issues WHERE issue_key = ?1",
            [&issue.issue_key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?, row.get::<_, String>(7)?, row.get::<_, String>(8)?)),
        ).optional().map_err(|error| error.to_string())?;
        let expected = (
            issue.summary.clone(),
            issue.description.clone(),
            issue.jira_status.clone(),
            issue.mapped_status.clone(),
            issue.priority.clone(),
            issue.labels.clone(),
            issue.source_name.clone(),
            issue.sync_status.clone(),
            issue.last_synced_at.clone(),
        );
        if actual != Some(expected) {
            return Err(format!(
                "Jira plugin issue {} did not match the legacy migration snapshot",
                issue.issue_key
            ));
        }
        let source = conn.query_row(
            "SELECT sync_status, departure_prompt_eligible FROM jira_issue_sources WHERE issue_key = ?1 AND source_name = ?2",
            params![issue.issue_key, issue.source_name],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        ).optional().map_err(|error| error.to_string())?;
        if source
            != Some((
                issue.sync_status.clone(),
                i64::from(issue.sync_status == "departed"),
            ))
        {
            return Err(format!(
                "Jira plugin source state for {} did not match the legacy migration snapshot",
                issue.issue_key
            ));
        }
    }
    for link in &snapshot.links {
        let actual = conn
            .query_row(
                "SELECT issue_key FROM jira_task_links WHERE task_key = ?1",
                [&link.task_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if actual.as_deref() != Some(&link.issue_key) {
            return Err(format!(
                "Jira plugin task link for {} did not match the legacy migration snapshot",
                link.task_key
            ));
        }
    }
    if let Some(expected) = &snapshot.credentials {
        let actual: LegacyCredentials = serde_json::from_reader(
            std::fs::File::open(secrets_dir.join("credentials.json"))
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("failed to validate Jira plugin credentials: {error}"))?;
        if &actual != expected {
            return Err(
                "Jira plugin credentials did not match the legacy migration snapshot".to_string(),
            );
        }
        let cloud_id: String = conn
            .query_row(
                "SELECT value FROM jira_cache_metadata WHERE key = 'cloud_id'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if cloud_id != expected.cloud_id {
            return Err(
                "Jira plugin cloud identity did not match the legacy migration snapshot"
                    .to_string(),
            );
        }
    }
    Ok(())
}

fn migrate_plugin_database(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS jira_plugin_schema_migrations (version INTEGER PRIMARY KEY);
         CREATE TABLE IF NOT EXISTS jira_issues (
            issue_key TEXT PRIMARY KEY, summary TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
            jira_status TEXT NOT NULL, mapped_status TEXT NOT NULL, priority TEXT,
            labels TEXT NOT NULL DEFAULT '[]', source_name TEXT NOT NULL,
            sync_status TEXT NOT NULL DEFAULT 'synced', last_synced_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS jira_task_links (
            task_key TEXT PRIMARY KEY, issue_key TEXT NOT NULL UNIQUE REFERENCES jira_issues(issue_key)
         );
         CREATE INDEX IF NOT EXISTS jira_issues_source_active ON jira_issues(source_name, sync_status);
         CREATE TABLE IF NOT EXISTS jira_issue_sources (
            issue_key TEXT NOT NULL REFERENCES jira_issues(issue_key), source_name TEXT NOT NULL,
            sync_status TEXT NOT NULL DEFAULT 'synced', departure_prompt_eligible INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (issue_key, source_name)
         );
         CREATE INDEX IF NOT EXISTS jira_issue_sources_source_active ON jira_issue_sources(source_name, sync_status);
         CREATE TABLE IF NOT EXISTS jira_cache_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS jira_departure_queue (
            issue_key TEXT PRIMARY KEY REFERENCES jira_issues(issue_key), summary TEXT NOT NULL, queued_at TEXT NOT NULL
         );
         INSERT OR IGNORE INTO jira_plugin_schema_migrations (version) VALUES (4);",
    ).map_err(|error| format!("failed to initialize Jira plugin database: {error}"))
}

fn snapshot_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("jira-migration").join(SNAPSHOT_FILE)
}

fn plugin_state_root(app_data_dir: &Path) -> PathBuf {
    app_data_dir
        .join("plugins")
        .join("state")
        .join(JIRA_PLUGIN_ID)
}

fn write_snapshot(path: &Path, snapshot: &LegacySnapshot) -> Result<(), String> {
    write_json_atomically(path, snapshot, true)
}

fn read_snapshot(path: &Path) -> Result<LegacySnapshot, String> {
    serde_json::from_reader(
        std::fs::File::open(path)
            .map_err(|error| format!("failed to read Jira migration backup: {error}"))?,
    )
    .map_err(|error| format!("failed to parse Jira migration backup: {error}"))
}

fn snapshot_digest(snapshot: &LegacySnapshot) -> Result<String, String> {
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(snapshot).map_err(|error| error.to_string())?);
    let digest = digest.finalize();
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn write_json_atomically<T: Serialize>(
    path: &Path,
    value: &T,
    private: bool,
) -> Result<(), String> {
    let parent = path.parent().ok_or("missing migration file parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("tmp");
    let encoded = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if private {
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    file.write_all(&encoded)
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    #[cfg(unix)]
    if private {
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    planeai_paths::replace_file_atomically(&temporary, path).map_err(|error| error.to_string())
}

fn record_failure(
    conn: &Connection,
    app_data_dir: &Path,
    snapshot: &LegacySnapshot,
    error: &str,
) -> Result<(), String> {
    write_record(
        conn,
        &MigrationRecord {
            state: JiraMigrationState::Failed,
            snapshot_path: Some(snapshot_path(app_data_dir).display().to_string()),
            snapshot_sha256: Some(snapshot_digest(snapshot)?),
            error: Some(error.to_string()),
            imported_issues: snapshot.issues.len(),
            imported_links: snapshot.links.len(),
        },
    )
}

fn read_record(conn: &Connection) -> Result<Option<MigrationRecord>, String> {
    conn.query_row(
        "SELECT state, snapshot_path, snapshot_sha256, error, imported_issues, imported_links FROM jira_plugin_migration WHERE id = 1",
        [],
        |row| {
            let state: String = row.get(0)?;
            let state = match state.as_str() {
                "not_needed" => JiraMigrationState::NotNeeded,
                "available" => JiraMigrationState::Available,
                "importing" => JiraMigrationState::Importing,
                "failed" => JiraMigrationState::Failed,
                "completed" => JiraMigrationState::Completed,
                _ => JiraMigrationState::Failed,
            };
            Ok(MigrationRecord { state, snapshot_path: row.get(1)?, snapshot_sha256: row.get(2)?, error: row.get(3)?, imported_issues: row.get::<_, i64>(4)? as usize, imported_links: row.get::<_, i64>(5)? as usize })
        },
    ).optional().map_err(|error| format!("failed to read Jira migration ledger: {error}"))
}

fn write_record(conn: &Connection, record: &MigrationRecord) -> Result<(), String> {
    conn.execute(
        "INSERT INTO jira_plugin_migration (id, state, snapshot_path, snapshot_sha256, error, imported_issues, imported_links, updated_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP)
         ON CONFLICT(id) DO UPDATE SET state = excluded.state, snapshot_path = excluded.snapshot_path,
            snapshot_sha256 = excluded.snapshot_sha256, error = excluded.error,
            imported_issues = excluded.imported_issues, imported_links = excluded.imported_links,
            updated_at = CURRENT_TIMESTAMP",
        params![
            match record.state { JiraMigrationState::NotNeeded => "not_needed", JiraMigrationState::Available => "available", JiraMigrationState::Importing => "importing", JiraMigrationState::Failed => "failed", JiraMigrationState::Completed => "completed" },
            record.snapshot_path,
            record.snapshot_sha256,
            record.error,
            record.imported_issues as i64,
            record.imported_links as i64,
        ],
    ).map_err(|error| format!("failed to write Jira migration ledger: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Appearance, IntegrationsConfig, Terminal};
    use planeai_jira::config::{JiraConfig, JiraSyncSource};

    fn config() -> Config {
        Config {
            appearance: Appearance {
                mode: "system".to_string(),
                terminal_theme_dark: String::new(),
                terminal_theme_light: String::new(),
                diff_theme_dark: String::new(),
                diff_theme_light: String::new(),
                theme: "default".to_string(),
            },
            terminal: Terminal {
                font_family: "Menlo".to_string(),
                font_size: 14,
                option_as_meta: true,
            },
            providers: HashMap::new(),
            default_provider: String::new(),
            session_backend: None,
            vim_mode: None,
            task_management: None,
            projects_base_path: None,
            pr_status: None,
            hide_done_tasks: None,
            hide_empty_projects: None,
            daemon_scrollback_bytes: None,
            scrollback_lines: None,
            web_links: None,
            session_log_dir: None,
            extra_path_dirs: vec![],
            auto_open_review: None,
            sound_enabled: None,
            integrations: Some(IntegrationsConfig {
                jira: Some(JiraConfig {
                    site: "https://legacy.atlassian.net".to_string(),
                    sync_interval_ms: 120_000,
                    sources: HashMap::from([(
                        "legacy".to_string(),
                        JiraSyncSource {
                            jql: "project = LEGACY".to_string(),
                            status_map: HashMap::from([(
                                "In Progress".to_string(),
                                "in_progress".to_string(),
                            )]),
                            writeback: None,
                        },
                    )]),
                }),
            }),
        }
    }

    fn legacy_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        planeai_jira::db::migrate(&conn).unwrap();
        conn.execute("INSERT INTO task_projects (prefix) VALUES ('LEG')", [])
            .unwrap();
        conn.execute("INSERT INTO tasks (key, project_prefix, title, description, status, priority, created_at, updated_at) VALUES ('LEG-1', 'LEG', 'Legacy issue', '', 'in_progress', 0, 'now', 'now')", []).unwrap();
        conn.execute("INSERT INTO tasks (key, project_prefix, title, description, status, priority, parent_key, created_at, updated_at) VALUES ('CHILD-1', 'LEG', 'Child', '', 'todo', 0, 'LEG-1', 'now', 'now')", []).unwrap();
        conn.execute("INSERT OR IGNORE INTO jira_issues (issue_key, jira_project, summary, description, status, priority, labels, sync_status, last_synced_at, source_name) VALUES ('LEG-1', 'LEG', 'Legacy issue', 'Description', 'In Progress', 'High', '[\"migration\"]', 'synced', '2024-01-01T00:00:00Z', 'legacy')", []).unwrap();
        conn.execute("INSERT OR IGNORE INTO jira_issues (issue_key, jira_project, summary, description, status, priority, labels, sync_status, last_synced_at, source_name) VALUES ('LEG-2', 'LEG', 'Departed issue', '', 'Done', NULL, '[]', 'departed', '2024-01-02T00:00:00Z', 'legacy')", []).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO jira_task_links (task_key, issue_key) VALUES ('LEG-1', 'LEG-1')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn preserves_compatible_existing_plugin_state() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let app_data_dir = temp.path().join("data");
        let data_dir = app_data_dir.join("plugins/state/jira/data");
        std::fs::create_dir_all(&data_dir).unwrap();
        write_json_atomically(&data_dir.join("settings.json"), &serde_json::json!({"site": "https://legacy.atlassian.net", "sources": {"plugin": {"jql": "project = PLUGIN", "status_map": {}, "writeback": null}}}), false).unwrap();
        let plugin = Connection::open(data_dir.join("jira.sqlite")).unwrap();
        migrate_plugin_database(&plugin).unwrap();
        plugin.execute("INSERT INTO jira_issues (issue_key, summary, description, jira_status, mapped_status, priority, labels, source_name, sync_status, last_synced_at) VALUES ('PLUGIN-1', 'Plugin issue', '', 'Open', 'todo', NULL, '[]', 'plugin', 'synced', 'now')", []).unwrap();
        plugin.execute("INSERT INTO jira_issue_sources (issue_key, source_name, sync_status, departure_prompt_eligible) VALUES ('PLUGIN-1', 'plugin', 'synced', 0)", []).unwrap();

        let conn = legacy_db();
        let mut cfg = config();
        import(&conn, &config_dir, &app_data_dir, &mut cfg).unwrap();

        assert_eq!(
            plugin
                .query_row("SELECT COUNT(*) FROM jira_issues", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            3
        );
        let settings: Value =
            serde_json::from_reader(std::fs::File::open(data_dir.join("settings.json")).unwrap())
                .unwrap();
        let sources = settings.get("sources").and_then(Value::as_object).unwrap();
        assert!(sources.contains_key("plugin"));
        assert!(sources.contains_key("legacy"));
    }

    #[test]
    fn imports_full_legacy_state_once_and_preserves_departures() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let app_data_dir = temp.path().join("data");
        std::fs::create_dir_all(app_data_dir.join("jira-tokens")).unwrap();
        std::fs::write(app_data_dir.join("jira-tokens/refresh_token"), "refresh").unwrap();
        std::fs::write(app_data_dir.join("jira-tokens/cloud_id"), "cloud").unwrap();
        let conn = legacy_db();
        let mut cfg = config();
        migrate(&conn).unwrap();
        crate::plugins::migrate(&conn).unwrap();
        crate::plugins::sync_inventory(&conn, &crate::plugins::bundled_manifests().unwrap())
            .unwrap();
        initialize(&conn, &cfg).unwrap();
        assert!(blocks_plugin_start(&conn));
        assert_eq!(
            conn.query_row(
                "SELECT enabled FROM plugin_inventory WHERE id = 'jira'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT parent_key FROM tasks WHERE key = 'CHILD-1'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "LEG-1"
        );
        import(&conn, &config_dir, &app_data_dir, &mut cfg).unwrap();
        let data = app_data_dir.join("plugins/state/jira/data");
        let plugin = Connection::open(data.join("jira.sqlite")).unwrap();
        assert_eq!(
            plugin
                .query_row("SELECT COUNT(*) FROM jira_issues", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            plugin
                .query_row(
                    "SELECT mapped_status FROM jira_issues WHERE issue_key = 'LEG-1'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "in_progress"
        );
        assert_eq!(
            plugin
                .query_row(
                    "SELECT COUNT(*) FROM jira_departure_queue WHERE issue_key = 'LEG-2'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            plugin
                .query_row("SELECT COUNT(*) FROM jira_task_links", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(cfg.integrations.is_none());
        mark_completed(&conn, &app_data_dir).unwrap();
        assert!(!blocks_plugin_start(&conn));
        assert_eq!(
            status(&conn, &cfg).unwrap().state,
            JiraMigrationState::Completed
        );
    }

    #[test]
    fn interrupted_import_is_fenced_and_retryable_from_the_frozen_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let app_data_dir = temp.path().join("data");
        let conn = legacy_db();
        let mut cfg = config();
        migrate(&conn).unwrap();
        crate::plugins::migrate(&conn).unwrap();
        crate::plugins::sync_inventory(&conn, &crate::plugins::bundled_manifests().unwrap())
            .unwrap();
        let first = import(&conn, &config_dir, &app_data_dir, &mut cfg).unwrap();
        assert_eq!(first.state, JiraMigrationState::Importing);
        assert!(cfg.integrations.is_none());

        // Simulate process termination after data/config validation but before
        // the host enables the sidecar and commits completion.
        initialize(&conn, &cfg).unwrap();
        let recovered = status(&conn, &cfg).unwrap();
        assert_eq!(recovered.state, JiraMigrationState::Failed);
        assert!(recovered.can_migrate);
        assert!(blocks_plugin_start(&conn));

        let retry = import(&conn, &config_dir, &app_data_dir, &mut cfg).unwrap();
        assert_eq!(retry.imported_issues, 2);
        assert_eq!(retry.imported_links, 1);
        assert_eq!(
            status(&conn, &cfg).unwrap().state,
            JiraMigrationState::Importing
        );
    }
}
