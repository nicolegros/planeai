use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use planeai_jira::config::WritebackConfig;
use rand::RngExt;
use reqwest::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use url::Url;

include!(concat!(env!("OUT_DIR"), "/oauth_credentials.rs"));

const PLUGIN_ID: &str = "jira";
const PLUGIN_NAME: &str = "Jira";
const PLUGIN_VERSION: &str = "0.1.0";
const HOST_API_VERSION: &str = "planeai.plugin-host.v1";
const MAX_RPC_FRAME_BYTES: u64 = 64 * 1024;
const AUTH_URL: &str = "https://auth.atlassian.com/authorize";
const TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
const RESOURCES_URL: &str = "https://api.atlassian.com/oauth/token/accessible-resources";
const SCOPES: &str = "read:jira-work write:jira-work offline_access";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);
const CALLBACK_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CALLBACK_REQUEST_LINE_BYTES: u64 = 8 * 1024;
const OAUTH_NETWORK_TIMEOUT: Duration = Duration::from_secs(20);
const CALLBACK_ADDRESS: &str = "127.0.0.1:19287";
const REDIRECT_URI: &str = "http://localhost:19287/callback";

#[derive(Deserialize)]
struct Request {
    jsonrpc: String,
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Debug, thiserror::Error)]
enum AuthError {
    #[error("Jira site URL must be an https URL with a host")]
    InvalidSite,
    #[error("OAuth state mismatch")]
    StateMismatch,
    #[error("Jira authorization failed: {0}")]
    ProviderError(String),
    #[error("Cloud ID not found for site: {0}")]
    CloudIdNotFound(String),
    #[error("Timed out waiting for browser callback")]
    Timeout,
    #[error("OAuth callback could not start: {0}")]
    CallbackStart(String),
    #[error("OAuth request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("OAuth callback failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("OAuth URL could not be built: {0}")]
    Url(#[from] url::ParseError),
    #[error("Jira did not return a refresh token")]
    MissingRefreshToken,
    #[error("Jira plugin secrets are unavailable: {0}")]
    Secrets(String),
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenErrorResponse {
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AccessibleResource {
    id: String,
    url: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct Credentials {
    refresh_token: String,
    cloud_id: String,
    site: String,
}

struct PendingAuth {
    attempt_id: String,
    site: String,
    verifier: String,
    state: String,
    listener: TcpListener,
}

struct AuthCompletion {
    attempt_id: String,
    task: tokio::task::JoinHandle<Result<(), AuthError>>,
}

struct JiraPlugin {
    data_dir: PathBuf,
    secrets_dir: PathBuf,
    pending_auth: Option<PendingAuth>,
    completion: Option<AuthCompletion>,
    authorization_error: Option<String>,
    completed_attempt: Option<String>,
    last_writeback: Option<Value>,
    client: Client,
    token_url: String,
    resources_url: String,
}

impl JiraPlugin {
    fn from_environment() -> Result<Self, String> {
        let data_dir = std::env::var_os("PLANEAI_PLUGIN_DATA_DIR")
            .map(PathBuf::from)
            .ok_or("PLANEAI_PLUGIN_DATA_DIR was not provided by the host")?;
        let secrets_dir = std::env::var_os("PLANEAI_PLUGIN_SECRETS_DIR")
            .map(PathBuf::from)
            .ok_or("PLANEAI_PLUGIN_SECRETS_DIR was not provided by the host")?;
        std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&secrets_dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        std::fs::set_permissions(&secrets_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            data_dir,
            secrets_dir,
            pending_auth: None,
            completion: None,
            authorization_error: None,
            completed_attempt: None,
            last_writeback: None,
            client: Client::builder()
                .timeout(OAUTH_NETWORK_TIMEOUT)
                .build()
                .map_err(|error| error.to_string())?,
            token_url: TOKEN_URL.to_string(),
            resources_url: RESOURCES_URL.to_string(),
        })
    }

    fn settings(&self) -> Result<Value, String> {
        let path = self.data_dir.join("settings.json");
        if !path.exists() {
            return Ok(json!({ "site": "", "sync_interval_ms": 60000 }));
        }
        let value: Value = serde_json::from_reader(
            std::fs::File::open(path).map_err(|e| format!("failed to read settings: {e}"))?,
        )
        .map_err(|e| format!("failed to parse settings: {e}"))?;
        if !value.is_object() {
            return Err("plugin settings must be a JSON object".to_string());
        }
        Ok(value)
    }

    fn site(&self) -> Result<String, AuthError> {
        configured_site(&self.data_dir)
    }

    fn credentials_path(&self) -> PathBuf {
        self.secrets_dir.join("credentials.json")
    }

    fn read_credentials(&self) -> Result<Credentials, AuthError> {
        serde_json::from_reader(
            std::fs::File::open(self.credentials_path())
                .map_err(|error| AuthError::Secrets(error.to_string()))?,
        )
        .map_err(|error| AuthError::Secrets(error.to_string()))
    }

    fn delete_credentials(&self) -> Result<(), AuthError> {
        match std::fs::remove_file(self.credentials_path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AuthError::Secrets(error.to_string())),
        }
    }

    async fn connect_start(&mut self, params: &Value) -> Result<Value, AuthError> {
        if self.pending_auth.is_some() || self.completion.is_some() {
            return Err(AuthError::CallbackStart(
                "an authorization flow is already waiting for a callback".to_string(),
            ));
        }
        if self.read_credentials().is_ok() {
            return Err(AuthError::CallbackStart(
                "disconnect the current Jira site before authorizing another one".to_string(),
            ));
        }
        let attempt_id = params
            .get("attempt_id")
            .and_then(Value::as_str)
            .filter(|attempt_id| !attempt_id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(generate_state);
        let site = self.site()?;
        let listener = TcpListener::bind(CALLBACK_ADDRESS).await.map_err(|e| {
            AuthError::CallbackStart(format!(
                "failed to bind OAuth callback port 19287: {e}. Is another PlaneAI instance running?"
            ))
        })?;
        let (verifier, challenge) = generate_pkce();
        let state = generate_state();
        let authorization_url = build_auth_url(REDIRECT_URI, &challenge, &state)?;
        self.authorization_error = None;
        self.completed_attempt = None;
        self.pending_auth = Some(PendingAuth {
            attempt_id,
            site,
            verifier,
            state,
            listener,
        });
        Ok(json!({ "authorization_url": authorization_url.to_string() }))
    }

    async fn connect_complete(&mut self, params: &Value) -> Result<Value, AuthError> {
        let requested_attempt = params.get("attempt_id").and_then(Value::as_str);
        let pending = self.pending_auth.take().ok_or_else(|| {
            AuthError::CallbackStart("start authorization before completing it".to_string())
        })?;
        if requested_attempt.is_some_and(|attempt_id| attempt_id != pending.attempt_id) {
            self.pending_auth = Some(pending);
            return Err(AuthError::CallbackStart(
                "authorization attempt does not match the active flow".to_string(),
            ));
        }
        let attempt_id = pending.attempt_id.clone();
        let client = self.client.clone();
        let token_url = self.token_url.clone();
        let resources_url = self.resources_url.clone();
        let secrets_dir = self.secrets_dir.clone();
        let data_dir = self.data_dir.clone();
        self.completion = Some(AuthCompletion {
            attempt_id,
            task: tokio::spawn(async move {
                finish_authorization(
                    pending,
                    &client,
                    &token_url,
                    &resources_url,
                    &data_dir,
                    &secrets_dir,
                )
                .await
            }),
        });
        Ok(json!({ "authorizing": true }))
    }

    async fn status(&mut self) -> Value {
        self.reap_completion().await;
        let credentials = self.read_credentials().ok();
        json!({
            "plugin_id": PLUGIN_ID,
            "plugin_name": PLUGIN_NAME,
            "plugin_version": PLUGIN_VERSION,
            "host_api_version": HOST_API_VERSION,
            "runtime_state": "running",
            "last_error": self.authorization_error,
            "last_writeback": self.last_writeback,
            "connected": credentials.is_some(),
            "authorizing": self.completion.is_some(),
            "site": credentials.map(|credentials| credentials.site),
        })
    }

    async fn reap_completion(&mut self) {
        if self
            .completion
            .as_ref()
            .is_some_and(|completion| completion.task.is_finished())
        {
            let completion = self
                .completion
                .take()
                .expect("completion was checked above");
            let attempt_id = completion.attempt_id;
            let result = completion.task.await;
            self.authorization_error = result
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()))
                .err();
            self.completed_attempt = Some(attempt_id);
        }
    }

    async fn cancel_authorization(&mut self, attempt_id: Option<&str>) -> bool {
        let pending_matches = self.pending_auth.as_ref().is_some_and(|pending| {
            attempt_id.is_none_or(|attempt_id| pending.attempt_id == attempt_id)
        });
        if pending_matches {
            self.pending_auth = None;
        }
        let completion_matches = self.completion.as_ref().is_some_and(|completion| {
            attempt_id.is_none_or(|attempt_id| completion.attempt_id == attempt_id)
        });
        if let Some(completion) = completion_matches.then(|| self.completion.take()).flatten() {
            completion.task.abort();
            let _ = completion.task.await;
        }
        let completed_matches =
            self.completed_attempt
                .as_deref()
                .is_some_and(|completed_attempt| {
                    attempt_id.is_none_or(|attempt_id| completed_attempt == attempt_id)
                });
        if completed_matches {
            self.completed_attempt = None;
        }
        pending_matches || completion_matches || completed_matches
    }

    async fn connect_cancel(&mut self, params: &Value) -> Result<Value, AuthError> {
        let cancelled = self
            .cancel_authorization(params.get("attempt_id").and_then(Value::as_str))
            .await;
        if cancelled {
            self.delete_credentials()?;
        }
        Ok(json!({ "cancelled": cancelled }))
    }

    async fn disconnect(&mut self) -> Result<Value, AuthError> {
        self.cancel_authorization(None).await;
        self.delete_credentials()?;
        Ok(json!({ "connected": false }))
    }

    fn save_settings(&self, settings: &Value) -> Result<Value, String> {
        if !settings.is_object() {
            return Err("plugin settings must be a JSON object".to_string());
        }
        let path = self.data_dir.join("settings.json");
        let temporary = self.data_dir.join(".settings.tmp");
        std::fs::write(
            &temporary,
            serde_json::to_vec_pretty(settings)
                .map_err(|error| format!("failed to serialize settings: {error}"))?,
        )
        .map_err(|error| format!("failed to write settings: {error}"))?;
        if let Err(error) = planeai_paths::replace_file_atomically(&temporary, &path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!("failed to save settings: {error}"));
        }
        Ok(settings.clone())
    }

    fn database(&self) -> Result<Connection, String> {
        let conn = Connection::open(self.data_dir.join("jira.sqlite"))
            .map_err(|error| format!("failed to open Jira plugin database: {error}"))?;
        migrate_database(&conn)?;
        Ok(conn)
    }

    fn update_source_settings(&self, settings: &Value) -> Result<Value, String> {
        let old_value = self.settings()?;
        let old = settings_from_value(&old_value)?;
        let new = settings_from_value(settings)?;
        validate_settings(&new)?;
        if let Ok(credentials) = self.read_credentials() {
            let configured_site = canonicalize_site(&new.site)
                .map_err(|error| format!("invalid connected Jira site: {error}"))?;
            if configured_site != credentials.site {
                return Err(
                    "disconnect the current Jira site before changing the configured site"
                        .to_string(),
                );
            }
        }

        let renamed = renamed_sources(&old, &new);
        let saved = self.save_settings(settings)?;
        let sync_memberships = (|| {
            let mut conn = self.database()?;
            for (from, to) in &renamed {
                rename_source_memberships(&mut conn, from, to)?;
            }
            for source in old.sources.keys() {
                if !new.sources.contains_key(source) && !renamed.contains_key(source) {
                    depart_source(&conn, source)?;
                }
            }
            Ok(())
        })();
        if let Err(error) = sync_memberships {
            let _ = self.save_settings(&old_value);
            return Err(error);
        }
        Ok(saved)
    }

    fn rename_source(&self, _params: &Value) -> Result<Value, String> {
        Err("source renames must be persisted through jira.settings.update".to_string())
    }

    fn sidebar_items(&self) -> Result<Value, String> {
        let credentials = match self.read_credentials() {
            Ok(credentials) => credentials,
            Err(_) => return Ok(json!({ "items": [] })),
        };
        let conn = self.database()?;
        if !cache_matches_site(&conn, &credentials.cloud_id)? {
            return Ok(json!({ "items": [] }));
        }
        let mut statement = conn.prepare("SELECT issue_key, summary, mapped_status FROM jira_issues WHERE sync_status = 'synced' ORDER BY last_synced_at DESC, issue_key").map_err(|error| format!("failed to list Jira sidebar items: {error}"))?;
        let rows = statement.query_map([], |row| Ok(json!({ "key": row.get::<_, String>(0)?, "title": row.get::<_, String>(1)?, "status": row.get::<_, String>(2)?, "child_count": 0 }))).map_err(|error| format!("failed to query Jira sidebar items: {error}"))?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|error| format!("failed to read Jira sidebar item: {error}"))?);
        }
        Ok(json!({ "items": items }))
    }

    fn issue(&self, params: &Value) -> Result<Value, String> {
        let key = params
            .get("key")
            .and_then(Value::as_str)
            .filter(|key| !key.is_empty())
            .ok_or("jira issue get requires key")?;
        let conn = self.database()?;
        conn.query_row(
            "SELECT issue_key, summary, description FROM jira_issues WHERE issue_key = ?1 AND sync_status = 'synced'",
            [key],
            |row| {
                Ok(json!({
                    "key": row.get::<_, String>(0)?,
                    "title": row.get::<_, String>(1)?,
                    "description": row.get::<_, String>(2)?,
                }))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => "Jira issue is no longer available".to_string(),
            _ => format!("failed to get Jira issue: {error}"),
        })
    }

    async fn sync_now<R, W>(&self, input: &mut R, output: &mut W) -> Result<Value, String>
    where
        R: tokio::io::AsyncBufRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        let settings = settings_from_value(&self.settings()?)?;
        validate_settings(&settings)?;
        let mut credentials = self
            .read_credentials()
            .map_err(|error| format!("Jira is not connected: {error}"))?;
        let token = refresh_access_token(
            &self.secrets_dir,
            &self.client,
            &self.token_url,
            &credentials.refresh_token,
        )
        .await?;
        if let Some(refresh_token) = token.refresh_token {
            persist_rotated_refresh_token(&self.secrets_dir, &mut credentials, refresh_token)
                .map_err(|error| {
                    format!("failed to persist refreshed Jira credentials: {error}")
                })?;
        }
        let access_token = token.access_token;
        let conn = self.database()?;
        ensure_cache_site(&conn, &credentials.cloud_id)?;
        let mut result = SyncTotals::default();
        let mut request_id = 0_u64;
        let mut sources: Vec<_> = settings.sources.iter().collect();
        sources.sort_by_key(|(name, _)| *name);
        for (source_name, source) in sources {
            let issues = match fetch_issues(
                &self.client,
                &credentials.cloud_id,
                &access_token,
                &source.jql,
            )
            .await
            {
                Ok(issues) => issues,
                Err(_) => {
                    result.errors += 1;
                    continue;
                }
            };
            let mut seen = HashSet::new();
            let mut source_failed = false;
            for issue in issues {
                seen.insert(issue.key.clone());
                let mapped_status = map_status(
                    &issue.status,
                    &source.status_map,
                    issue.status_category.as_deref(),
                );
                let existing = match host_call(
                    input,
                    output,
                    &mut request_id,
                    "host.task.get",
                    json!({ "key": issue.key }),
                )
                .await
                {
                    Ok(value) => value.get("task").cloned().unwrap_or(Value::Null),
                    Err(_) => {
                        source_failed = true;
                        break;
                    }
                };
                let task_result = if existing.is_null() {
                    host_call(input, output, &mut request_id, "host.task.create", json!({ "key": issue.key, "title": issue.summary, "description": issue.description, "status": mapped_status, "priority": map_priority(issue.priority.as_deref()), "tags": issue.labels })).await.map(|_| { result.created += 1; })
                } else if linked_task_key(&conn, &issue.key)?.is_none() {
                    Err(format!(
                        "refusing to overwrite local task {} without a Jira task link",
                        issue.key
                    ))
                } else {
                    let mapped_priority = map_priority(issue.priority.as_deref());
                    let changed = existing.get("title").and_then(Value::as_str)
                        != Some(issue.summary.as_str())
                        || existing.get("description").and_then(Value::as_str)
                            != Some(issue.description.as_str())
                        || existing.get("status").and_then(Value::as_str)
                            != Some(mapped_status.as_str())
                        || existing.get("priority").and_then(Value::as_i64)
                            != Some(i64::from(mapped_priority))
                        || existing.get("tags")
                            != Some(&Value::Array(
                                issue.labels.iter().cloned().map(Value::String).collect(),
                            ));
                    if changed {
                        host_call(
                            input,
                            output,
                            &mut request_id,
                            "host.task.update",
                            json!({
                                "key": issue.key,
                                "title": issue.summary,
                                "description": issue.description,
                                "status": mapped_status,
                                "priority": mapped_priority,
                                "tags": issue.labels,
                            }),
                        )
                        .await
                        .map(|_| {
                            result.updated += 1;
                        })
                    } else {
                        Ok(())
                    }
                };
                if task_result.is_err() {
                    source_failed = true;
                    break;
                }
                upsert_issue(&conn, &issue, source_name, &mapped_status)?;
                sync_issue_source(&conn, &issue.key, source_name)?;
                conn.execute("INSERT INTO jira_task_links (task_key, issue_key) VALUES (?1, ?2) ON CONFLICT(issue_key) DO UPDATE SET task_key = excluded.task_key", params![issue.key, issue.key]).map_err(|error| format!("failed to persist Jira task link: {error}"))?;
            }
            if source_failed {
                result.errors += 1;
                continue;
            }
            for departed in active_source_issues(&conn, source_name)? {
                if seen.contains(&departed.key) {
                    continue;
                }
                let task = host_call(
                    input,
                    output,
                    &mut request_id,
                    "host.task.get",
                    json!({ "key": departed.key }),
                )
                .await
                .ok()
                .and_then(|value| value.get("task").cloned())
                .unwrap_or(Value::Null);
                mark_departed(&conn, &departed.key, source_name)?;
                if task.get("status").and_then(Value::as_str) != Some("done") {
                    result.departed += 1;
                }
            }
        }
        serde_json::to_value(result).map_err(|error| error.to_string())
    }
    fn selected_writeback_source(
        &self,
        issue_key: &str,
    ) -> Result<Option<(String, WritebackConfig)>, String> {
        let conn = self.database()?;
        let source_name = conn
            .query_row(
                "SELECT source_name FROM jira_issue_sources WHERE issue_key = ?1 AND sync_status = 'synced' ORDER BY source_name LIMIT 1",
                params![issue_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("failed to resolve Jira writeback source: {error}"))?;
        let Some(source_name) = source_name else {
            return Ok(None);
        };
        let settings = settings_from_value(&self.settings()?);
        let writeback = settings
            .sources
            .get(&source_name)
            .and_then(|source| source.writeback.clone())
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| {
                format!("invalid Jira writeback settings for {source_name}: {error}")
            })?;
        Ok(writeback.map(|writeback| (source_name, writeback)))
    }

    async fn writeback_lifecycle(
        &mut self,
        issue_key: &str,
        source_name: &str,
        action: &str,
        config: &WritebackConfig,
    ) -> Value {
        let target = if action == "start" {
            config.on_start.as_deref()
        } else {
            config.on_complete.as_deref()
        };
        let message = format!(
            "planeai: Task moved to {} at {}",
            if action == "start" {
                "Start"
            } else {
                "Complete"
            },
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ")
        );
        let mut errors = Vec::new();
        let mut transition_ok = target.is_none();
        let mut comment_ok = !config.comment;
        match self
            .read_credentials()
            .map_err(|e| format!("Jira is not connected: {e}"))
        {
            Err(error) => errors.push(error),
            Ok(mut credentials) => match refresh_access_token(
                &self.secrets_dir,
                &self.client,
                &self.token_url,
                &credentials.refresh_token,
            )
            .await
            {
                Err(error) => errors.push(error),
                Ok(token) => {
                    if let Some(refresh_token) = token.refresh_token {
                        if let Err(error) = persist_rotated_refresh_token(
                            &self.secrets_dir,
                            &mut credentials,
                            refresh_token,
                        ) {
                            errors.push(error.to_string());
                        }
                    }
                    let base = format!(
                        "https://api.atlassian.com/ex/jira/{}/rest/api/3/issue/{issue_key}",
                        credentials.cloud_id
                    );
                    if let Some(target) = target {
                        let transition = async {
                            let transitions: Value = self
                                .client
                                .get(format!("{base}/transitions"))
                                .bearer_auth(&token.access_token)
                                .send()
                                .await
                                .map_err(|e| format!("Jira transition lookup failed: {e}"))?
                                .error_for_status()
                                .map_err(|e| format!("Jira transition lookup failed: {e}"))?
                                .json()
                                .await
                                .map_err(|e| format!("invalid Jira transitions response: {e}"))?;
                            let id = transitions
                                .get("transitions")
                                .and_then(Value::as_array)
                                .and_then(|items| {
                                    items.iter().find(|item| {
                                        item.get("to")
                                            .and_then(|to| to.get("name"))
                                            .and_then(Value::as_str)
                                            .is_some_and(|name| name.eq_ignore_ascii_case(target))
                                    })
                                })
                                .and_then(|item| item.get("id"))
                                .and_then(Value::as_str)
                                .ok_or_else(|| {
                                    format!("Jira transition target {target:?} was not available")
                                })?;
                            self.client
                                .post(format!("{base}/transitions"))
                                .bearer_auth(&token.access_token)
                                .json(&json!({"transition":{"id":id}}))
                                .send()
                                .await
                                .map_err(|e| format!("Jira transition failed: {e}"))?
                                .error_for_status()
                                .map_err(|e| format!("Jira transition failed: {e}"))?;
                            Ok::<(), String>(())
                        }
                        .await;
                        match transition {
                            Ok(()) => transition_ok = true,
                            Err(error) => errors.push(error),
                        }
                    }
                    if config.comment {
                        let body = json!({"body":{"type":"doc","version":1,"content":[{"type":"paragraph","content":[{"type":"text","text":message}]}]}});
                        match self
                            .client
                            .post(format!("{base}/comment"))
                            .bearer_auth(&token.access_token)
                            .json(&body)
                            .send()
                            .await
                            .map_err(|e| format!("Jira comment failed: {e}"))
                            .and_then(|r| {
                                r.error_for_status()
                                    .map_err(|e| format!("Jira comment failed: {e}"))
                            }) {
                            Ok(_) => comment_ok = true,
                            Err(error) => errors.push(error),
                        }
                    }
                }
            },
        }
        let ok = errors.is_empty();
        if !ok {
            eprintln!("jira lifecycle writeback failed issue={issue_key} source={source_name} action={action}: {}", errors.join("; "));
        }
        let outcome = json!({"at":Utc::now(),"issue_key":issue_key,"source":source_name,"action":action,"ok":ok,"transition_ok":transition_ok,"comment_ok":comment_ok,"error":(!errors.is_empty()).then(|| errors.join("; "))});
        self.last_writeback = Some(outcome.clone());
        outcome
    }

    async fn task_lifecycle(&mut self, params: &Value) -> Result<Value, String> {
        let events = params
            .get("batch")
            .and_then(|batch| batch.get("events"))
            .and_then(Value::as_array)
            .ok_or("task lifecycle batch requires events")?;
        let mut outcomes = Vec::new();
        for event in events {
            let event_type = event
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let candidate = match event_type {
                "child_assigned"
                    if event
                        .get("is_first_child_assignment")
                        .and_then(Value::as_bool)
                        == Some(true) =>
                {
                    event
                        .get("parent_key")
                        .and_then(Value::as_str)
                        .map(|key| (key, "start"))
                }
                "status_changed"
                    if event.get("previous_status").and_then(Value::as_str) != Some("done")
                        && event.get("new_status").and_then(Value::as_str) == Some("done") =>
                {
                    event
                        .get("task_key")
                        .and_then(Value::as_str)
                        .map(|key| (key, "complete"))
                }
                _ => None,
            };
            let Some((issue_key, action)) = candidate else {
                continue;
            };
            if let Some((source_name, config)) = self.selected_writeback_source(issue_key)? {
                let configured = match action {
                    "start" => config.on_start.is_some(),
                    "complete" => config.on_complete.is_some(),
                    _ => false,
                } || config.comment;
                if configured {
                    outcomes.push(
                        self.writeback_lifecycle(issue_key, &source_name, action, &config)
                            .await,
                    );
                }
            }
        }
        Ok(json!({"outcomes": outcomes}))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct JiraSettings {
    #[serde(default)]
    site: String,
    #[serde(default = "default_sync_interval_ms")]
    sync_interval_ms: u64,
    #[serde(default)]
    sources: HashMap<String, JiraSource>,
}
fn default_sync_interval_ms() -> u64 {
    60_000
}
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
struct JiraSource {
    #[serde(default)]
    jql: String,
    #[serde(default)]
    status_map: HashMap<String, String>,
    #[serde(default)]
    writeback: Option<Value>,
}
#[derive(Debug, Default, Serialize)]
struct SyncTotals {
    created: usize,
    updated: usize,
    departed: usize,
    errors: usize,
}
#[derive(Debug)]
struct FetchedIssue {
    key: String,
    summary: String,
    description: String,
    status: String,
    status_category: Option<String>,
    priority: Option<String>,
    labels: Vec<String>,
}
#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    issues: Vec<RawIssue>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}
#[derive(Deserialize)]
struct RawIssue {
    key: String,
    fields: RawFields,
}
#[derive(Deserialize)]
struct RawFields {
    summary: String,
    #[serde(default)]
    description: Value,
    status: RawStatus,
    priority: Option<RawName>,
    #[serde(default)]
    labels: Vec<String>,
}
#[derive(Deserialize)]
struct RawStatus {
    name: String,
    #[serde(rename = "statusCategory")]
    status_category: Option<RawName>,
}
#[derive(Deserialize)]
struct RawName {
    name: String,
}
#[derive(Debug)]
struct StoredIssue {
    key: String,
}
fn settings_from_value(value: &Value) -> Result<JiraSettings, String> {
    serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid Jira plugin settings: {error}"))
}

fn validate_settings(settings: &JiraSettings) -> Result<(), String> {
    for (name, source) in &settings.sources {
        if source.jql.trim().is_empty() {
            return Err(format!(
                "Jira source {name:?} requires a non-empty JQL query"
            ));
        }
    }
    Ok(())
}

fn renamed_sources(old: &JiraSettings, new: &JiraSettings) -> HashMap<String, String> {
    let mut removed: Vec<_> = old
        .sources
        .iter()
        .filter(|(name, _)| !new.sources.contains_key(*name))
        .collect();
    removed.sort_by_key(|(name, _)| *name);
    let mut added: Vec<_> = new
        .sources
        .iter()
        .filter(|(name, _)| !old.sources.contains_key(*name))
        .collect();
    added.sort_by_key(|(name, _)| *name);

    let mut renamed = HashMap::new();
    let mut used = HashSet::new();
    for (old_name, old_source) in removed {
        if let Some((new_name, _)) = added
            .iter()
            .find(|(new_name, new_source)| !used.contains(*new_name) && *new_source == old_source)
        {
            renamed.insert((*old_name).clone(), (*new_name).clone());
            used.insert((*new_name).clone());
        }
    }
    renamed
}

fn migrate_database(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS jira_plugin_schema_migrations (
            version INTEGER PRIMARY KEY
        );",
    )
    .map_err(|error| format!("failed to initialize Jira plugin migration table: {error}"))?;

    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM jira_plugin_schema_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to read Jira plugin schema version: {error}"))?;

    if current_version < 1 {
        conn.execute_batch(
            "CREATE TABLE jira_issues (
                issue_key TEXT PRIMARY KEY,
                summary TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                jira_status TEXT NOT NULL,
                mapped_status TEXT NOT NULL,
                priority TEXT,
                labels TEXT NOT NULL DEFAULT '[]',
                source_name TEXT NOT NULL,
                sync_status TEXT NOT NULL DEFAULT 'synced',
                last_synced_at TEXT NOT NULL
            );
            CREATE TABLE jira_task_links (
                task_key TEXT PRIMARY KEY,
                issue_key TEXT NOT NULL UNIQUE REFERENCES jira_issues(issue_key)
            );
            CREATE INDEX jira_issues_source_active ON jira_issues(source_name, sync_status);
            INSERT INTO jira_plugin_schema_migrations (version) VALUES (1);",
        )
        .map_err(|error| format!("failed to apply Jira plugin migration 1: {error}"))?;
    }

    if current_version < 2 {
        conn.execute_batch(
            "CREATE TABLE jira_issue_sources (
                issue_key TEXT NOT NULL REFERENCES jira_issues(issue_key),
                source_name TEXT NOT NULL,
                sync_status TEXT NOT NULL DEFAULT 'synced',
                PRIMARY KEY (issue_key, source_name)
            );
            CREATE INDEX jira_issue_sources_source_active ON jira_issue_sources(source_name, sync_status);
            INSERT OR IGNORE INTO jira_issue_sources (issue_key, source_name, sync_status)
                SELECT issue_key, source_name, sync_status FROM jira_issues;
            INSERT INTO jira_plugin_schema_migrations (version) VALUES (2);",
        )
        .map_err(|error| format!("failed to apply Jira plugin migration 2: {error}"))?;
    }

    if current_version < 3 {
        conn.execute_batch(
            "CREATE TABLE jira_cache_metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            INSERT INTO jira_plugin_schema_migrations (version) VALUES (3);",
        )
        .map_err(|error| format!("failed to apply Jira plugin migration 3: {error}"))?;
    }

    Ok(())
}

fn clear_sync_cache(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "DELETE FROM jira_task_links;
         DELETE FROM jira_issue_sources;
         DELETE FROM jira_issues;",
    )
    .map_err(|error| format!("failed to clear Jira synchronization cache: {error}"))
}

fn cache_matches_site(conn: &Connection, cloud_id: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT value FROM jira_cache_metadata WHERE key = 'cloud_id'",
        [],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map(|stored| stored.as_deref() == Some(cloud_id))
    .map_err(|error| format!("failed to read Jira cache site: {error}"))
}

fn ensure_cache_site(conn: &Connection, cloud_id: &str) -> Result<(), String> {
    if cache_matches_site(conn, cloud_id)? {
        return Ok(());
    }
    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| format!("failed to start Jira cache site update: {error}"))?;
    clear_sync_cache(&transaction)?;
    transaction
        .execute(
            "INSERT INTO jira_cache_metadata (key, value) VALUES ('cloud_id', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![cloud_id],
        )
        .map_err(|error| format!("failed to persist Jira cache site: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("failed to commit Jira cache site update: {error}"))
}

fn upsert_issue(
    conn: &Connection,
    issue: &FetchedIssue,
    source_name: &str,
    mapped_status: &str,
) -> Result<(), String> {
    let labels = serde_json::to_string(&issue.labels).map_err(|error| error.to_string())?;
    conn.execute("INSERT INTO jira_issues (issue_key, summary, description, jira_status, mapped_status, priority, labels, source_name, sync_status, last_synced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'synced', ?9) ON CONFLICT(issue_key) DO UPDATE SET summary = excluded.summary, description = excluded.description, jira_status = excluded.jira_status, mapped_status = excluded.mapped_status, priority = excluded.priority, labels = excluded.labels, source_name = excluded.source_name, sync_status = 'synced', last_synced_at = excluded.last_synced_at", params![issue.key, issue.summary, issue.description, issue.status, mapped_status, issue.priority, labels, source_name, Utc::now().to_rfc3339()]).map_err(|error| format!("failed to persist Jira issue: {error}"))?;
    Ok(())
}
fn rename_source_memberships(conn: &mut Connection, from: &str, to: &str) -> Result<(), String> {
    let transaction = conn
        .transaction()
        .map_err(|error| format!("failed to start Jira source rename: {error}"))?;
    transaction
        .execute(
            "UPDATE jira_issues SET source_name = ?1 WHERE source_name = ?2",
            params![to, from],
        )
        .map_err(|error| format!("failed to rename legacy Jira source ownership: {error}"))?;
    transaction
        .execute(
            "INSERT INTO jira_issue_sources (issue_key, source_name, sync_status)
             SELECT issue_key, ?1, sync_status FROM jira_issue_sources WHERE source_name = ?2
             ON CONFLICT(issue_key, source_name) DO UPDATE SET sync_status =
                 CASE WHEN jira_issue_sources.sync_status = 'synced' OR excluded.sync_status = 'synced'
                 THEN 'synced' ELSE 'departed' END",
            params![to, from],
        )
        .map_err(|error| format!("failed to rename Jira source memberships: {error}"))?;
    transaction
        .execute(
            "DELETE FROM jira_issue_sources WHERE source_name = ?1",
            params![from],
        )
        .map_err(|error| format!("failed to remove renamed Jira source memberships: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("failed to commit Jira source rename: {error}"))
}

fn linked_task_key(conn: &Connection, issue_key: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT task_key FROM jira_task_links WHERE issue_key = ?1",
        params![issue_key],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| format!("failed to read Jira task link: {error}"))
}

fn sync_issue_source(conn: &Connection, key: &str, source_name: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO jira_issue_sources (issue_key, source_name, sync_status) VALUES (?1, ?2, 'synced') ON CONFLICT(issue_key, source_name) DO UPDATE SET sync_status = 'synced'",
        params![key, source_name],
    )
    .map_err(|error| format!("failed to persist Jira source membership: {error}"))?;
    refresh_issue_sync_status(conn, key)
}

fn active_source_issues(conn: &Connection, source_name: &str) -> Result<Vec<StoredIssue>, String> {
    let mut statement = conn
        .prepare(
            "SELECT issue_key FROM jira_issue_sources WHERE source_name = ?1 AND sync_status = 'synced'",
        )
        .map_err(|error| format!("failed to list active Jira source issues: {error}"))?;
    let rows = statement
        .query_map(params![source_name], |row| {
            Ok(StoredIssue { key: row.get(0)? })
        })
        .map_err(|error| format!("failed to query Jira source memberships: {error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn mark_departed(conn: &Connection, key: &str, source_name: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE jira_issue_sources SET sync_status = 'departed' WHERE issue_key = ?1 AND source_name = ?2",
        params![key, source_name],
    )
    .map_err(|error| format!("failed to mark Jira source membership departed: {error}"))?;
    refresh_issue_sync_status(conn, key)
}

fn depart_source(conn: &Connection, source_name: &str) -> Result<(), String> {
    let issues = active_source_issues(conn, source_name)?;
    conn.execute(
        "UPDATE jira_issue_sources SET sync_status = 'departed' WHERE source_name = ?1 AND sync_status = 'synced'",
        params![source_name],
    )
    .map_err(|error| format!("failed to depart removed Jira source: {error}"))?;
    for issue in issues {
        refresh_issue_sync_status(conn, &issue.key)?;
    }
    Ok(())
}

fn refresh_issue_sync_status(conn: &Connection, key: &str) -> Result<(), String> {
    let has_active_source: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM jira_issue_sources WHERE issue_key = ?1 AND sync_status = 'synced')",
            params![key],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to read Jira source memberships: {error}"))?;
    conn.execute(
        "UPDATE jira_issues SET sync_status = CASE WHEN ?1 THEN 'synced' ELSE 'departed' END WHERE issue_key = ?2",
        params![has_active_source, key],
    )
    .map_err(|error| format!("failed to refresh Jira issue sync status: {error}"))?;
    Ok(())
}

fn persist_rotated_refresh_token(
    secrets_dir: &std::path::Path,
    credentials: &mut Credentials,
    refresh_token: String,
) -> Result<(), AuthError> {
    credentials.refresh_token = refresh_token;
    write_credentials_to(secrets_dir, credentials)
}

async fn refresh_access_token(
    secrets_dir: &std::path::Path,
    client: &Client,
    token_url: &str,
    refresh_token: &str,
) -> Result<TokenResponse, String> {
    let response = client
        .post(token_url)
        .json(&json!({ "grant_type": "refresh_token", "client_id": CLIENT_ID, "client_secret": CLIENT_SECRET, "refresh_token": refresh_token }))
        .send()
        .await
        .map_err(|error| format!("failed to refresh Jira token: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("failed to read Jira token response: {error}"))?;

    if !status.is_success() {
        let rejected = status == reqwest::StatusCode::BAD_REQUEST
            && serde_json::from_str::<TokenErrorResponse>(&body)
                .ok()
                .and_then(|error| error.error)
                .as_deref()
                == Some("invalid_grant");
        if rejected {
            clear_rejected_refresh_credentials(secrets_dir)?;
            return Err("Jira OAuth refresh token was rejected; reconnect to Jira".to_string());
        }
        return Err(format!("Jira token refresh failed: HTTP {status}"));
    }

    serde_json::from_str::<TokenResponse>(&body)
        .map_err(|error| format!("invalid Jira token response: {error}"))
}

fn clear_rejected_refresh_credentials(secrets_dir: &std::path::Path) -> Result<(), String> {
    match std::fs::remove_file(secrets_dir.join("credentials.json")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Jira OAuth refresh token was rejected, but credentials could not be cleared: {error}"
        )),
    }
}
async fn fetch_issues(
    client: &Client,
    cloud_id: &str,
    token: &str,
    jql: &str,
) -> Result<Vec<FetchedIssue>, String> {
    let mut next_page_token: Option<String> = None;
    let mut issues = Vec::new();
    for _ in 0..100 {
        let url = format!("https://api.atlassian.com/ex/jira/{cloud_id}/rest/api/3/search/jql");
        let mut request = client.get(&url).bearer_auth(token).query(&[
            ("jql", jql),
            ("fields", "summary,description,status,priority,labels"),
            ("maxResults", "50"),
        ]);
        if let Some(page) = &next_page_token {
            request = request.query(&[("nextPageToken", page)]);
        }
        let page = request
            .send()
            .await
            .map_err(|error| format!("Jira search failed: {error}"))?
            .error_for_status()
            .map_err(|error| format!("Jira search failed: {error}"))?
            .json::<SearchResponse>()
            .await
            .map_err(|error| format!("invalid Jira search response: {error}"))?;
        issues.extend(page.issues.into_iter().map(|issue| {
            FetchedIssue {
                key: issue.key,
                summary: issue.fields.summary,
                description: plain_text(&issue.fields.description),
                status: issue.fields.status.name,
                status_category: issue
                    .fields
                    .status
                    .status_category
                    .map(|category| category.name),
                priority: issue.fields.priority.map(|priority| priority.name),
                labels: issue.fields.labels,
            }
        }));
        match page.next_page_token {
            Some(token) => next_page_token = Some(token),
            None => break,
        }
    }
    if next_page_token.is_some() {
        return Err("Jira search exceeded the 5,000-issue synchronization limit".to_string());
    }
    Ok(issues)
}
fn plain_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(values) => values
            .iter()
            .map(plain_text)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(values) => values
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| values.get("content").map(plain_text).unwrap_or_default()),
        _ => String::new(),
    }
}
fn map_status(
    jira_status: &str,
    status_map: &HashMap<String, String>,
    category: Option<&str>,
) -> String {
    if let Some(status) = status_map.get(jira_status).filter(|status| {
        matches!(
            status.as_str(),
            "todo" | "in_progress" | "in_review" | "done"
        )
    }) {
        return status.clone();
    }
    match category {
        Some("To Do") => "todo",
        Some("In Progress") => "in_progress",
        Some("Done") => "done",
        _ => "todo",
    }
    .to_string()
}
fn map_priority(priority: Option<&str>) -> i32 {
    // PlaneAI dispatches larger values first; preserve Jira's descending priority.
    match priority {
        Some("Highest") => 5,
        Some("High") => 4,
        Some("Medium") => 3,
        Some("Low") => 2,
        Some("Lowest") => 1,
        _ => 0,
    }
}
async fn host_call<R, W>(
    input: &mut R,
    output: &mut W,
    next_id: &mut u64,
    method: &str,
    params: Value,
) -> Result<Value, String>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    *next_id += 1;
    let id = format!("plugin:{next_id}");
    let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    let encoded = serde_json::to_string(&request).map_err(|error| error.to_string())?;
    if encoded.len() >= MAX_RPC_FRAME_BYTES as usize {
        return Err("host RPC request exceeded the maximum frame size".to_string());
    }
    output
        .write_all(encoded.as_bytes())
        .await
        .map_err(|error| format!("failed to write host RPC request: {error}"))?;
    output
        .write_all(b"\n")
        .await
        .map_err(|error| format!("failed to frame host RPC request: {error}"))?;
    output
        .flush()
        .await
        .map_err(|error| format!("failed to flush host RPC request: {error}"))?;
    let Some(frame) = read_json_rpc_frame(input)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("host closed plugin RPC input".to_string());
    };
    let response: Value = serde_json::from_str(&frame)
        .map_err(|error| format!("invalid host RPC response: {error}"))?;
    if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || response.get("id").and_then(Value::as_str) != Some(id.as_str())
    {
        return Err("host RPC response id did not match request".to_string());
    }
    if let Some(error) = response.get("error") {
        return Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("host task request failed")
            .to_string());
    }
    response
        .get("result")
        .cloned()
        .ok_or("host RPC response omitted result".to_string())
}

#[cfg(test)]
mod sync_tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    fn issue(key: &str, status: &str) -> FetchedIssue {
        FetchedIssue {
            key: key.to_string(),
            summary: format!("Summary {key}"),
            description: "Description".to_string(),
            status: status.to_string(),
            status_category: Some("To Do".to_string()),
            priority: Some("High".to_string()),
            labels: vec!["sync".to_string()],
        }
    }

    #[test]
    fn migrations_are_versioned_and_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_database(&conn).unwrap();
        migrate_database(&conn).unwrap();
        let version: i64 = conn
            .query_row(
                "SELECT MAX(version) FROM jira_plugin_schema_migrations",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 3);
    }

    fn plugin_for_test(data_dir: PathBuf) -> JiraPlugin {
        let secrets_dir = data_dir.join("secrets");
        std::fs::create_dir_all(&secrets_dir).unwrap();
        JiraPlugin {
            data_dir,
            secrets_dir,
            pending_auth: None,
            completion: None,
            authorization_error: None,
            completed_attempt: None,
            client: Client::builder().build().unwrap(),
            token_url: TOKEN_URL.to_string(),
            resources_url: RESOURCES_URL.to_string(),
        }
    }

    #[tokio::test]
    async fn issue_rpc_returns_only_synced_issues_and_validates_keys() {
        let data_dir = std::env::temp_dir().join(format!("planeai-jira-rpc-{}", generate_state()));
        std::fs::create_dir_all(&data_dir).unwrap();
        let mut plugin = plugin_for_test(data_dir.clone());
        let conn = plugin.database().unwrap();
        conn.execute(
            "INSERT INTO jira_issues (issue_key, summary, description, jira_status, mapped_status, source_name, sync_status, last_synced_at) VALUES ('SYNC-1', 'Synced summary', 'Synced description', 'Open', 'todo', 'source', 'synced', 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO jira_issues (issue_key, summary, description, jira_status, mapped_status, source_name, sync_status, last_synced_at) VALUES ('OLD-1', 'Departed summary', '', 'Open', 'todo', 'source', 'departed', 'now')",
            [],
        )
        .unwrap();
        drop(conn);
        let mut input = BufReader::new(tokio::io::empty());
        let mut output = tokio::io::sink();

        let synced = dispatch(
            &mut plugin,
            Request {
                jsonrpc: "2.0".to_string(),
                id: json!(1),
                method: "jira.issue.get".to_string(),
                params: json!({ "key": "SYNC-1" }),
            },
            &mut input,
            &mut output,
        )
        .await;
        assert_eq!(
            synced.result,
            Some(
                json!({ "key": "SYNC-1", "title": "Synced summary", "description": "Synced description" })
            )
        );

        for params in [
            json!({ "key": "OLD-1" }),
            json!({ "key": "MISSING-1" }),
            json!({ "key": "" }),
        ] {
            let response = dispatch(
                &mut plugin,
                Request {
                    jsonrpc: "2.0".to_string(),
                    id: json!(2),
                    method: "jira.issue.get".to_string(),
                    params,
                },
                &mut input,
                &mut output,
            )
            .await;
            assert_eq!(response.error.unwrap().code, -32000);
        }

        drop(plugin);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn migration_backfills_source_memberships_from_v1_records() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE jira_plugin_schema_migrations (version INTEGER PRIMARY KEY);
             INSERT INTO jira_plugin_schema_migrations (version) VALUES (1);
             CREATE TABLE jira_issues (
                issue_key TEXT PRIMARY KEY, summary TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
                jira_status TEXT NOT NULL, mapped_status TEXT NOT NULL, priority TEXT, labels TEXT NOT NULL DEFAULT '[]',
                source_name TEXT NOT NULL, sync_status TEXT NOT NULL DEFAULT 'synced', last_synced_at TEXT NOT NULL
             );
             INSERT INTO jira_issues VALUES ('ONE-1', 'Summary', '', 'Open', 'todo', NULL, '[]', 'one', 'synced', 'now');",
        )
        .unwrap();

        migrate_database(&conn).unwrap();
        assert_eq!(active_source_issues(&conn, "one").unwrap()[0].key, "ONE-1");
    }

    #[test]
    fn source_records_are_isolated_and_removed_sources_depart_only_their_records() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_database(&conn).unwrap();
        upsert_issue(&conn, &issue("ONE-1", "Open"), "one", "todo").unwrap();
        sync_issue_source(&conn, "ONE-1", "one").unwrap();
        upsert_issue(&conn, &issue("TWO-1", "Open"), "two", "todo").unwrap();
        sync_issue_source(&conn, "TWO-1", "two").unwrap();

        assert_eq!(active_source_issues(&conn, "one").unwrap()[0].key, "ONE-1");
        depart_source(&conn, "one").unwrap();
        assert!(active_source_issues(&conn, "one").unwrap().is_empty());
        assert_eq!(active_source_issues(&conn, "two").unwrap()[0].key, "TWO-1");
    }

    #[test]
    fn departing_one_overlapping_source_keeps_the_issue_synced() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_database(&conn).unwrap();
        upsert_issue(&conn, &issue("ONE-1", "Open"), "one", "todo").unwrap();
        sync_issue_source(&conn, "ONE-1", "one").unwrap();
        sync_issue_source(&conn, "ONE-1", "two").unwrap();

        mark_departed(&conn, "ONE-1", "one").unwrap();
        let status: String = conn
            .query_row(
                "SELECT sync_status FROM jira_issues WHERE issue_key = 'ONE-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "synced");

        mark_departed(&conn, "ONE-1", "two").unwrap();
        let status: String = conn
            .query_row(
                "SELECT sync_status FROM jira_issues WHERE issue_key = 'ONE-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "departed");
    }

    #[test]
    fn malformed_settings_are_rejected_instead_of_defaulting() {
        assert!(settings_from_value(&json!({ "sources": [] })).is_err());
    }

    #[test]
    fn settings_reject_blank_source_jql() {
        let settings = JiraSettings {
            sources: HashMap::from([(
                "empty".to_string(),
                JiraSource {
                    jql: "  ".to_string(),
                    status_map: HashMap::new(),
                    writeback: None,
                },
            )]),
            ..JiraSettings::default()
        };
        assert!(validate_settings(&settings)
            .unwrap_err()
            .contains("non-empty JQL"));
    }

    #[test]
    fn source_rename_moves_memberships_without_departing_issues() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate_database(&conn).unwrap();
        upsert_issue(&conn, &issue("ONE-1", "Open"), "old", "todo").unwrap();
        sync_issue_source(&conn, "ONE-1", "old").unwrap();

        rename_source_memberships(&mut conn, "old", "new").unwrap();
        assert!(active_source_issues(&conn, "old").unwrap().is_empty());
        assert_eq!(active_source_issues(&conn, "new").unwrap()[0].key, "ONE-1");
        depart_source(&conn, "old").unwrap();
        let status: String = conn
            .query_row(
                "SELECT sync_status FROM jira_issues WHERE issue_key = 'ONE-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "synced");
    }

    #[test]
    fn settings_detect_a_pure_source_rename() {
        let source = JiraSource {
            jql: "project = PLA".to_string(),
            status_map: HashMap::new(),
            writeback: None,
        };
        let old = JiraSettings {
            sources: HashMap::from([("old".to_string(), source.clone())]),
            ..JiraSettings::default()
        };
        let new = JiraSettings {
            sources: HashMap::from([("new".to_string(), source)]),
            ..JiraSettings::default()
        };
        assert_eq!(
            renamed_sources(&old, &new).get("old"),
            Some(&"new".to_string())
        );
    }

    #[test]
    fn task_link_distinguishes_synced_jira_tasks_from_local_key_collisions() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_database(&conn).unwrap();
        assert!(linked_task_key(&conn, "ONE-1").unwrap().is_none());
        conn.execute(
            "INSERT INTO jira_issues (issue_key, summary, description, jira_status, mapped_status, source_name, last_synced_at) VALUES ('ONE-1', 'Summary', '', 'Open', 'todo', 'source', 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO jira_task_links (task_key, issue_key) VALUES ('ONE-1', 'ONE-1')",
            [],
        )
        .unwrap();
        assert_eq!(
            linked_task_key(&conn, "ONE-1").unwrap().as_deref(),
            Some("ONE-1")
        );
    }

    #[test]
    fn status_mapping_prefers_explicit_valid_mapping_then_category_then_todo() {
        let explicit = HashMap::from([("In QA".to_string(), "in_review".to_string())]);
        assert_eq!(map_status("In QA", &explicit, Some("Done")), "in_review");
        assert_eq!(
            map_status("Other", &HashMap::new(), Some("In Progress")),
            "in_progress"
        );
        assert_eq!(map_status("Other", &HashMap::new(), None), "todo");
        let invalid = HashMap::from([("In QA".to_string(), "not-a-status".to_string())]);
        assert_eq!(map_status("In QA", &invalid, Some("Done")), "done");
    }

    #[test]
    fn priority_mapping_preserves_legacy_priority_order() {
        assert_eq!(map_priority(Some("Highest")), 5);
        assert_eq!(map_priority(Some("High")), 4);
        assert_eq!(map_priority(Some("Medium")), 3);
        assert_eq!(map_priority(Some("Low")), 2);
        assert_eq!(map_priority(Some("Lowest")), 1);
        assert_eq!(map_priority(Some("Unknown")), 0);
    }

    #[tokio::test]
    async fn host_callback_uses_a_correlated_nested_json_rpc_request() {
        let (plugin_stream, host_stream) = tokio::io::duplex(4096);
        let (plugin_read, plugin_write) = tokio::io::split(plugin_stream);
        let (host_read, mut host_write) = tokio::io::split(host_stream);
        let mut input = BufReader::new(plugin_read);
        let mut output = plugin_write;
        let host = tokio::spawn(async move {
            let mut reader = BufReader::new(host_read);
            let mut request = String::new();
            reader.read_line(&mut request).await.unwrap();
            let request: Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request["method"], "host.task.get");
            assert_eq!(request["params"]["key"], "ABC-1");
            let response =
                json!({ "jsonrpc": "2.0", "id": request["id"], "result": { "task": null } });
            host_write
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        });

        let mut next_id = 0;
        let result = host_call(
            &mut input,
            &mut output,
            &mut next_id,
            "host.task.get",
            json!({ "key": "ABC-1" }),
        )
        .await
        .unwrap();
        host.await.unwrap();
        assert_eq!(result["task"], Value::Null);
    }
}

fn configured_site(data_dir: &std::path::Path) -> Result<String, AuthError> {
    let value: Value = serde_json::from_reader(
        std::fs::File::open(data_dir.join("settings.json"))
            .map_err(|error| AuthError::Secrets(error.to_string()))?,
    )
    .map_err(|error| AuthError::Secrets(error.to_string()))?;
    value
        .get("site")
        .and_then(Value::as_str)
        .ok_or(AuthError::InvalidSite)
        .and_then(canonicalize_site)
}

fn canonicalize_site(site: &str) -> Result<String, AuthError> {
    let parsed = Url::parse(site.trim()).map_err(|_| AuthError::InvalidSite)?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return Err(AuthError::InvalidSite);
    }
    let host = parsed
        .host_str()
        .expect("host was checked above")
        .to_ascii_lowercase();
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Ok(format!("https://{host}{port}"))
}

fn write_credentials_to(
    secrets_dir: &std::path::Path,
    credentials: &Credentials,
) -> Result<(), AuthError> {
    let content =
        serde_json::to_vec(credentials).map_err(|error| AuthError::Secrets(error.to_string()))?;
    let temporary = secrets_dir.join(format!(".credentials-{}.tmp", generate_state()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&temporary)
        .map_err(|error| AuthError::Secrets(error.to_string()))?;
    if let Err(error) = file.write_all(&content).and_then(|()| file.sync_all()) {
        let _ = std::fs::remove_file(&temporary);
        return Err(AuthError::Secrets(error.to_string()));
    }
    if let Err(error) =
        planeai_paths::replace_file_atomically(&temporary, &secrets_dir.join("credentials.json"))
    {
        let _ = std::fs::remove_file(&temporary);
        return Err(AuthError::Secrets(error.to_string()));
    }
    Ok(())
}

async fn finish_authorization(
    pending: PendingAuth,
    client: &Client,
    token_url: &str,
    resources_url: &str,
    data_dir: &std::path::Path,
    secrets_dir: &std::path::Path,
) -> Result<(), AuthError> {
    let code = tokio::time::timeout(
        CALLBACK_TIMEOUT,
        wait_for_callback(&pending.listener, &pending.state),
    )
    .await
    .map_err(|_| AuthError::Timeout)??;
    let token = exchange_code(client, token_url, &code, &pending.verifier).await?;
    let refresh_token = token.refresh_token.ok_or(AuthError::MissingRefreshToken)?;
    let cloud_id =
        fetch_cloud_id(client, resources_url, &pending.site, &token.access_token).await?;
    if configured_site(data_dir)? != pending.site {
        return Err(AuthError::CallbackStart(
            "Jira site changed while authorization was in progress; reconnect for the new site"
                .to_string(),
        ));
    }
    write_credentials_to(
        secrets_dir,
        &Credentials {
            refresh_token,
            cloud_id,
            site: pending.site,
        },
    )
}

async fn exchange_code(
    client: &Client,
    token_url: &str,
    code: &str,
    verifier: &str,
) -> Result<TokenResponse, AuthError> {
    client
        .post(token_url)
        .json(&json!({
            "grant_type": "authorization_code",
            "client_id": CLIENT_ID,
            "client_secret": CLIENT_SECRET,
            "code": code,
            "redirect_uri": REDIRECT_URI,
            "code_verifier": verifier,
        }))
        .send()
        .await?
        .error_for_status()?
        .json::<TokenResponse>()
        .await
        .map_err(AuthError::from)
}

async fn fetch_cloud_id(
    client: &Client,
    resources_url: &str,
    site: &str,
    access_token: &str,
) -> Result<String, AuthError> {
    let resources = client
        .get(resources_url)
        .bearer_auth(access_token)
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<AccessibleResource>>()
        .await?;
    resources
        .iter()
        .find(|resource| canonicalize_site(&resource.url).ok().as_deref() == Some(site))
        .map(|resource| resource.id.clone())
        .ok_or_else(|| AuthError::CloudIdNotFound(site.to_string()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut plugin = JiraPlugin::from_environment().map_err(std::io::Error::other)?;
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    while let Some(line) = read_json_rpc_frame(&mut input).await? {
        let request = match serde_json::from_str::<Request>(&line) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("malformed JSON-RPC request: {error}");
                continue;
            }
        };
        let should_shutdown = is_valid_shutdown_request(&request);
        let response = dispatch(&mut plugin, request, &mut input, &mut output).await;
        output
            .write_all(encode_response_frame(response)?.as_bytes())
            .await?;
        output.write_all(b"\n").await?;
        output.flush().await?;
        if should_shutdown {
            break;
        }
    }
    Ok(())
}

async fn read_json_rpc_frame<R>(reader: &mut R) -> Result<Option<String>, std::io::Error>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let mut frame = Vec::new();
    let bytes_read = reader
        .take(MAX_RPC_FRAME_BYTES)
        .read_until(b'\n', &mut frame)
        .await?;
    if bytes_read == 0 {
        return Ok(None);
    }
    if !frame.ends_with(b"\n") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "JSON-RPC request exceeded the frame limit or was not newline terminated",
        ));
    }
    String::from_utf8(frame)
        .map(Some)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

async fn dispatch<R, W>(
    plugin: &mut JiraPlugin,
    request: Request,
    input: &mut R,
    output: &mut W,
) -> Response
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    if !matches!(
        request.id,
        Value::Null | Value::String(_) | Value::Number(_)
    ) {
        return error(Value::Null, -32600, "expected a scalar JSON-RPC id");
    }
    if request.jsonrpc != "2.0" {
        return error(request.id, -32600, "expected jsonrpc 2.0");
    }
    let result = match request.method.as_str() {
        "plugin.handshake" => match request
            .params
            .get("host_api_version")
            .and_then(Value::as_str)
        {
            Some(HOST_API_VERSION) => Ok(json!({
                "plugin_id": PLUGIN_ID,
                "plugin_name": PLUGIN_NAME,
                "plugin_version": PLUGIN_VERSION,
                "host_api_version": HOST_API_VERSION,
                "lifecycle_event_subscriptions": [],
            })),
            _ => Err("unsupported plugin host API version".to_string()),
        },
        "jira.status" => Ok(plugin.status().await),
        "jira.settings.get" => plugin.settings(),
        "jira.settings.update" => plugin.update_source_settings(&request.params),
        "jira.sources.rename" => plugin.rename_source(&request.params),
        "jira.sidebar.items" => plugin.sidebar_items(),
        "jira.issue.get" => plugin.issue(&request.params),
        "jira.syncNow" => plugin.sync_now(input, output).await,
        "plugin.taskLifecycle" => plugin.task_lifecycle(&request.params).await,
        "jira.open_browser" => request
            .params
            .get("url")
            .and_then(Value::as_str)
            .ok_or("missing browser URL".to_string())
            .and_then(|url| {
                validate_authorization_url(url)?;
                open::that(url).map_err(|error| format!("failed to open browser: {error}"))
            })
            .map(|_| json!({ "opened": true })),
        "jira.connect.start" => plugin
            .connect_start(&request.params)
            .await
            .map_err(|error| error.to_string()),
        "jira.connect.complete" => plugin
            .connect_complete(&request.params)
            .await
            .map_err(|error| error.to_string()),
        "jira.connect.cancel" => plugin
            .connect_cancel(&request.params)
            .await
            .map_err(|error| error.to_string()),
        "jira.disconnect" => plugin.disconnect().await.map_err(|error| error.to_string()),
        "plugin.shutdown" => Ok(json!({ "stopping": true })),
        _ => Err("method not found".to_string()),
    };
    match result {
        Ok(value) => success(request.id, value),
        Err(message) => error(request.id, -32000, &message),
    }
}

fn success(id: Value, result: Value) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn error(id: Value, code: i64, message: &str) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(RpcError {
            code,
            message: message.to_string(),
        }),
    }
}

fn encode_response_frame(response: Response) -> Result<String, serde_json::Error> {
    let frame = serde_json::to_string(&response)?;
    if frame.len() < MAX_RPC_FRAME_BYTES as usize {
        return Ok(frame);
    }
    let fallback = serde_json::to_string(&error(
        response.id,
        -32000,
        "plugin response exceeded the maximum frame size",
    ))?;
    if fallback.len() < MAX_RPC_FRAME_BYTES as usize {
        return Ok(fallback);
    }
    serde_json::to_string(&error(
        Value::Null,
        -32000,
        "plugin response exceeded the maximum frame size",
    ))
}

fn is_valid_shutdown_request(request: &Request) -> bool {
    request.jsonrpc == "2.0"
        && request.method == "plugin.shutdown"
        && matches!(
            request.id,
            Value::Null | Value::String(_) | Value::Number(_)
        )
}

fn generate_pkce() -> (String, String) {
    let verifier_bytes: Vec<u8> = (0..32).map(|_| rand::rng().random()).collect();
    let verifier = URL_SAFE_NO_PAD.encode(&verifier_bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn generate_state() -> String {
    let bytes: Vec<u8> = (0..16).map(|_| rand::rng().random()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

fn validate_authorization_url(value: &str) -> Result<(), String> {
    let url = Url::parse(value).map_err(|_| "invalid Jira authorization URL".to_string())?;
    let expected = Url::parse(AUTH_URL).expect("AUTH_URL is a valid URL");
    if url.scheme() != "https"
        || url.origin() != expected.origin()
        || url.path() != expected.path()
        || url.username() != ""
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(
            "Jira browser capability only permits Atlassian authorization URLs".to_string(),
        );
    }
    let query: HashMap<_, _> = url.query_pairs().collect();
    if query.get("audience").map(|value| value.as_ref()) != Some("api.atlassian.com")
        || query.get("client_id").map(|value| value.as_ref()) != Some(CLIENT_ID)
        || query.get("response_type").map(|value| value.as_ref()) != Some("code")
        || query.get("redirect_uri").map(|value| value.as_ref()) != Some(REDIRECT_URI)
        || query
            .get("code_challenge_method")
            .map(|value| value.as_ref())
            != Some("S256")
        || !query.contains_key("code_challenge")
        || !query.contains_key("state")
    {
        return Err("invalid Jira authorization URL".to_string());
    }
    Ok(())
}

fn build_auth_url(redirect_uri: &str, challenge: &str, state: &str) -> Result<Url, AuthError> {
    let mut url = Url::parse(AUTH_URL)?;
    url.query_pairs_mut()
        .append_pair("audience", "api.atlassian.com")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("scope", SCOPES)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("prompt", "consent");
    Ok(url)
}

async fn wait_for_callback(
    listener: &TcpListener,
    expected_state: &str,
) -> Result<String, AuthError> {
    loop {
        let (stream, _) = listener.accept().await?;
        let (reader, mut writer) = stream.into_split();
        let reader = BufReader::new(reader);
        let mut request_line = Vec::new();
        let bytes_read = match tokio::time::timeout(
            CALLBACK_REQUEST_TIMEOUT,
            reader
                .take(MAX_CALLBACK_REQUEST_LINE_BYTES)
                .read_until(b'\n', &mut request_line),
        )
        .await
        {
            Ok(Ok(bytes_read)) => bytes_read,
            Ok(Err(error)) => return Err(AuthError::Io(error)),
            Err(_) => {
                write_callback_response(&mut writer, "408 Request Timeout", "Request timed out.")
                    .await;
                continue;
            }
        };
        let request_line = match (bytes_read > 0 && request_line.ends_with(b"\n"))
            .then(|| String::from_utf8(request_line))
        {
            Some(Ok(line)) => line,
            _ => {
                write_callback_response(
                    &mut writer,
                    "400 Bad Request",
                    "Invalid callback request.",
                )
                .await;
                continue;
            }
        };
        let mut request_parts = request_line.split_whitespace();
        let (Some("GET"), Some(path)) = (request_parts.next(), request_parts.next()) else {
            write_callback_response(&mut writer, "400 Bad Request", "Invalid callback request.")
                .await;
            continue;
        };
        if !path.starts_with("/callback") {
            write_callback_response(&mut writer, "400 Bad Request", "Invalid callback request.")
                .await;
            continue;
        }
        let parsed = match Url::parse(&format!("http://localhost{path}")) {
            Ok(parsed) => parsed,
            Err(_) => {
                write_callback_response(
                    &mut writer,
                    "400 Bad Request",
                    "Invalid callback request.",
                )
                .await;
                continue;
            }
        };
        let params: HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        if params.get("state").map(String::as_str) != Some(expected_state) {
            write_callback_response(
                &mut writer,
                "400 Bad Request",
                "Authentication failed: state mismatch.",
            )
            .await;
            return Err(AuthError::StateMismatch);
        }
        if let Some(provider_error) = params.get("error") {
            let description = params
                .get("error_description")
                .map(|description| format!(": {description}"))
                .unwrap_or_default();
            write_callback_response(
                &mut writer,
                "400 Bad Request",
                "Authentication failed. You can close this tab.",
            )
            .await;
            return Err(AuthError::ProviderError(format!(
                "{provider_error}{description}"
            )));
        }
        let Some(code) = params.get("code").cloned() else {
            write_callback_response(
                &mut writer,
                "400 Bad Request",
                "No authorization code received.",
            )
            .await;
            continue;
        };
        write_callback_response(
            &mut writer,
            "200 OK",
            "Authentication successful! You can close this tab.",
        )
        .await;
        return Ok(code);
    }
}

async fn write_callback_response(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    status: &str,
    body: &str,
) {
    let _ = writer
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn browser_capability_accepts_only_generated_atlassian_authorization_urls() {
        let url = build_auth_url(REDIRECT_URI, "challenge", "state").unwrap();
        assert!(validate_authorization_url(url.as_str()).is_ok());
        assert!(validate_authorization_url("https://example.com/authorize?state=state").is_err());
        assert!(
            validate_authorization_url("https://auth.atlassian.com/authorize?state=state").is_err()
        );
    }

    #[test]
    fn cache_is_retained_for_a_same_site_reconnect_and_cleared_for_a_new_site() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_database(&conn).unwrap();
        ensure_cache_site(&conn, "cloud-a").unwrap();
        conn.execute("INSERT INTO jira_issues (issue_key, summary, jira_status, mapped_status, source_name, last_synced_at) VALUES ('ABC-1', 'Old site issue', 'To Do', 'todo', 'source', 'now')", []).unwrap();
        conn.execute(
            "INSERT INTO jira_task_links (task_key, issue_key) VALUES ('ABC-1', 'ABC-1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO jira_issue_sources (issue_key, source_name) VALUES ('ABC-1', 'source')",
            [],
        )
        .unwrap();

        ensure_cache_site(&conn, "cloud-a").unwrap();
        assert_eq!(
            linked_task_key(&conn, "ABC-1").unwrap(),
            Some("ABC-1".to_string())
        );

        ensure_cache_site(&conn, "cloud-b").unwrap();
        assert_eq!(linked_task_key(&conn, "ABC-1").unwrap(), None);
        assert!(cache_matches_site(&conn, "cloud-b").unwrap());
    }

    #[test]
    fn pkce_is_url_safe_and_uses_s256() {
        let (verifier, challenge) = generate_pkce();
        assert!(verifier.len() >= 43 && verifier.len() <= 128);
        assert!(verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_eq!(
            challenge,
            URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
        );
    }

    #[tokio::test]
    async fn callback_rejects_a_mismatched_state() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let wait = tokio::spawn(async move { wait_for_callback(&listener, "expected").await });
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        stream
            .write_all(b"GET /callback?code=abc&state=wrong HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        assert!(matches!(wait.await.unwrap(), Err(AuthError::StateMismatch)));
    }

    #[tokio::test]
    async fn exchange_and_cloud_lookup_use_the_plugin_backend_client() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("authorization_code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"access_token":"access","refresh_token":"refresh","expires_in":3600}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/resources"))
            .and(header("Authorization", "Bearer access"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!([{ "id":"cloud", "url":"https://example.atlassian.net" }]),
                ),
            )
            .mount(&server)
            .await;
        let root =
            std::env::temp_dir().join(format!("planeai-jira-plugin-test-{}", generate_state()));
        let plugin = JiraPlugin {
            data_dir: root.join("data"),
            secrets_dir: root.join("secrets"),
            pending_auth: None,
            completion: None,
            authorization_error: None,
            completed_attempt: None,
            last_writeback: None,
            client: Client::new(),
            token_url: format!("{}/oauth/token", server.uri()),
            resources_url: format!("{}/resources", server.uri()),
        };
        std::fs::create_dir_all(&plugin.data_dir).unwrap();
        std::fs::create_dir_all(&plugin.secrets_dir).unwrap();
        let token = exchange_code(&plugin.client, &plugin.token_url, "code", "verifier")
            .await
            .unwrap();
        assert_eq!(
            fetch_cloud_id(
                &plugin.client,
                &plugin.resources_url,
                "https://example.atlassian.net",
                &token.access_token,
            )
            .await
            .unwrap(),
            "cloud"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn rejected_refresh_clears_credentials_for_reconnect() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(
                ResponseTemplate::new(400).set_body_json(json!({"error":"invalid_grant"})),
            )
            .mount(&server)
            .await;
        let root =
            std::env::temp_dir().join(format!("planeai-jira-plugin-test-{}", generate_state()));
        let secrets_dir = root.join("secrets");
        std::fs::create_dir_all(&secrets_dir).unwrap();
        write_credentials_to(
            &secrets_dir,
            &Credentials {
                refresh_token: "rejected-refresh".to_string(),
                cloud_id: "cloud".to_string(),
                site: "https://example.atlassian.net".to_string(),
            },
        )
        .unwrap();

        let error = refresh_access_token(
            &secrets_dir,
            &Client::new(),
            &format!("{}/oauth/token", server.uri()),
            "rejected-refresh",
        )
        .await
        .unwrap_err();

        assert_eq!(
            error,
            "Jira OAuth refresh token was rejected; reconnect to Jira"
        );
        assert!(!secrets_dir.join("credentials.json").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rotated_refresh_token_replaces_persisted_credentials() {
        let root =
            std::env::temp_dir().join(format!("planeai-jira-plugin-test-{}", generate_state()));
        let secrets_dir = root.join("secrets");
        std::fs::create_dir_all(&secrets_dir).unwrap();
        let mut credentials = Credentials {
            refresh_token: "old-refresh".to_string(),
            cloud_id: "cloud".to_string(),
            site: "https://example.atlassian.net".to_string(),
        };

        write_credentials_to(&secrets_dir, &credentials).unwrap();
        persist_rotated_refresh_token(&secrets_dir, &mut credentials, "new-refresh".to_string())
            .unwrap();
        let stored: Credentials = serde_json::from_reader(
            std::fs::File::open(secrets_dir.join("credentials.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(stored.refresh_token, "new-refresh");
        assert_eq!(stored.cloud_id, "cloud");
        assert_eq!(stored.site, "https://example.atlassian.net");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn callback_ignores_malformed_requests_until_a_valid_callback_arrives() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let wait = tokio::spawn(async move { wait_for_callback(&listener, "expected").await });

        let mut malformed = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        malformed.write_all(b"not an HTTP request\n").await.unwrap();
        let mut valid = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        valid
            .write_all(b"GET /callback?code=abc&state=expected HTTP/1.1\r\n\r\n")
            .await
            .unwrap();

        assert_eq!(wait.await.unwrap().unwrap(), "abc");
    }

    #[test]
    fn credentials_are_replaced_atomically_with_private_permissions() {
        let root =
            std::env::temp_dir().join(format!("planeai-jira-plugin-test-{}", generate_state()));
        let plugin = JiraPlugin {
            data_dir: root.join("data"),
            secrets_dir: root.join("secrets"),
            pending_auth: None,
            completion: None,
            authorization_error: None,
            completed_attempt: None,
            last_writeback: None,
            client: Client::new(),
            token_url: TOKEN_URL.to_string(),
            resources_url: RESOURCES_URL.to_string(),
        };
        std::fs::create_dir_all(&plugin.secrets_dir).unwrap();
        write_credentials_to(
            &plugin.secrets_dir,
            &Credentials {
                refresh_token: "first".to_string(),
                cloud_id: "cloud-a".to_string(),
                site: "https://example.atlassian.net".to_string(),
            },
        )
        .unwrap();
        write_credentials_to(
            &plugin.secrets_dir,
            &Credentials {
                refresh_token: "second".to_string(),
                cloud_id: "cloud-b".to_string(),
                site: "https://example.atlassian.net".to_string(),
            },
        )
        .unwrap();

        let credentials = plugin.read_credentials().unwrap();
        assert_eq!(credentials.refresh_token, "second");
        assert_eq!(credentials.cloud_id, "cloud-b");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(plugin.credentials_path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert!(std::fs::read_dir(&plugin.secrets_dir)
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn cancelling_authorization_removes_credentials_written_by_the_task() {
        let root =
            std::env::temp_dir().join(format!("planeai-jira-plugin-test-{}", generate_state()));
        let secrets_dir = root.join("secrets");
        std::fs::create_dir_all(&secrets_dir).unwrap();
        let (written, observed_write) = tokio::sync::oneshot::channel();
        let task_secrets_dir = secrets_dir.clone();
        let completion = tokio::spawn(async move {
            write_credentials_to(
                &task_secrets_dir,
                &Credentials {
                    refresh_token: "refresh".to_string(),
                    cloud_id: "cloud".to_string(),
                    site: "https://example.atlassian.net".to_string(),
                },
            )?;
            let _ = written.send(());
            std::future::pending::<()>().await;
            Ok(())
        });
        let mut plugin = JiraPlugin {
            data_dir: root.join("data"),
            secrets_dir: secrets_dir.clone(),
            pending_auth: None,
            completion: Some(AuthCompletion {
                attempt_id: "test".to_string(),
                task: completion,
            }),
            authorization_error: None,
            completed_attempt: None,
            last_writeback: None,
            client: Client::new(),
            token_url: TOKEN_URL.to_string(),
            resources_url: RESOURCES_URL.to_string(),
        };
        observed_write.await.unwrap();
        plugin.connect_cancel(&Value::Null).await.unwrap();
        assert!(!secrets_dir.join("credentials.json").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn cancelling_a_reaped_attempt_removes_its_credentials() {
        let root =
            std::env::temp_dir().join(format!("planeai-jira-plugin-test-{}", generate_state()));
        let secrets_dir = root.join("secrets");
        std::fs::create_dir_all(&secrets_dir).unwrap();
        write_credentials_to(
            &secrets_dir,
            &Credentials {
                refresh_token: "refresh".to_string(),
                cloud_id: "cloud".to_string(),
                site: "https://example.atlassian.net".to_string(),
            },
        )
        .unwrap();
        let mut plugin = JiraPlugin {
            data_dir: root.join("data"),
            secrets_dir: secrets_dir.clone(),
            pending_auth: None,
            completion: None,
            authorization_error: None,
            completed_attempt: Some("completed-attempt".to_string()),
            last_writeback: None,
            client: Client::new(),
            token_url: TOKEN_URL.to_string(),
            resources_url: RESOURCES_URL.to_string(),
        };
        assert_eq!(
            plugin
                .connect_cancel(&json!({ "attempt_id": "completed-attempt" }))
                .await
                .unwrap()["cancelled"],
            true
        );
        assert!(!secrets_dir.join("credentials.json").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn stale_cancellation_does_not_remove_a_newer_attempts_credentials() {
        let root =
            std::env::temp_dir().join(format!("planeai-jira-plugin-test-{}", generate_state()));
        let secrets_dir = root.join("secrets");
        std::fs::create_dir_all(&secrets_dir).unwrap();
        write_credentials_to(
            &secrets_dir,
            &Credentials {
                refresh_token: "refresh".to_string(),
                cloud_id: "cloud".to_string(),
                site: "https://example.atlassian.net".to_string(),
            },
        )
        .unwrap();
        let mut plugin = JiraPlugin {
            data_dir: root.join("data"),
            secrets_dir: secrets_dir.clone(),
            pending_auth: None,
            completion: None,
            authorization_error: None,
            completed_attempt: Some("newer-attempt".to_string()),
            last_writeback: None,
            client: Client::new(),
            token_url: TOKEN_URL.to_string(),
            resources_url: RESOURCES_URL.to_string(),
        };

        assert_eq!(
            plugin
                .connect_cancel(&json!({ "attempt_id": "stale-attempt" }))
                .await
                .unwrap()["cancelled"],
            false
        );
        assert!(secrets_dir.join("credentials.json").exists());
        assert_eq!(plugin.completed_attempt.as_deref(), Some("newer-attempt"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn canonical_site_normalizes_equivalent_https_origins() {
        assert_eq!(
            canonicalize_site("https://EXAMPLE.atlassian.net:443/").unwrap(),
            "https://example.atlassian.net"
        );
        assert!(canonicalize_site("https://example.atlassian.net/jira/software").is_err());
        assert!(canonicalize_site("https://user@example.atlassian.net/").is_err());
    }

    #[test]
    fn oversized_responses_are_replaced_with_a_bounded_error_frame() {
        let frame = encode_response_frame(success(
            json!(1),
            json!({ "site": "x".repeat(MAX_RPC_FRAME_BYTES as usize) }),
        ))
        .unwrap();
        assert!(frame.len() < MAX_RPC_FRAME_BYTES as usize);
        assert!(frame.contains("maximum frame size"));
    }

    #[test]
    fn oversized_response_fallback_bounds_a_long_scalar_id() {
        let frame = encode_response_frame(success(
            json!("x".repeat(MAX_RPC_FRAME_BYTES as usize)),
            json!({ "site": "x".repeat(MAX_RPC_FRAME_BYTES as usize) }),
        ))
        .unwrap();
        assert!(frame.len() < MAX_RPC_FRAME_BYTES as usize);
        assert!(frame.contains("\"id\":null"));
    }

    #[test]
    fn malformed_shutdown_request_does_not_terminate_the_plugin() {
        let request = Request {
            jsonrpc: "2.0".to_string(),
            id: json!(["not-scalar"]),
            method: "plugin.shutdown".to_string(),
            params: Value::Null,
        };
        assert!(!is_valid_shutdown_request(&request));
    }

    #[tokio::test]
    async fn lifecycle_uses_first_active_source_and_isolates_remote_failure() {
        let root =
            std::env::temp_dir().join(format!("planeai-jira-lifecycle-{}", generate_state()));
        let mut plugin = JiraPlugin {
            data_dir: root.join("data"),
            secrets_dir: root.join("secrets"),
            pending_auth: None,
            completion: None,
            authorization_error: None,
            completed_attempt: None,
            last_writeback: None,
            client: Client::new(),
            token_url: TOKEN_URL.to_string(),
            resources_url: RESOURCES_URL.to_string(),
        };
        std::fs::create_dir_all(&plugin.data_dir).unwrap();
        std::fs::create_dir_all(&plugin.secrets_dir).unwrap();
        plugin.save_settings(&json!({
            "sources": {
                "z-source": {"jql": "project = Z", "writeback": {"on_start": "In Progress"}},
                "a-source": {"jql": "project = A", "writeback": {"on_start": "Development", "comment": true}}
            }
        })).unwrap();
        let conn = plugin.database().unwrap();
        upsert_issue(&conn, &issue_for_test("PLA-288"), "z-source", "todo").unwrap();
        sync_issue_source(&conn, "PLA-288", "z-source").unwrap();
        sync_issue_source(&conn, "PLA-288", "a-source").unwrap();

        let response = plugin
            .task_lifecycle(&json!({"batch": {"events": [{
                "type": "child_assigned", "child_key": "PLA-289", "parent_key": "PLA-288",
                "is_first_child_assignment": true
            }]}}))
            .await
            .unwrap();

        assert_eq!(response["outcomes"].as_array().unwrap().len(), 1);
        assert_eq!(response["outcomes"][0]["source"], "a-source");
        assert_eq!(response["outcomes"][0]["action"], "start");
        assert_eq!(response["outcomes"][0]["ok"], false);
        assert_eq!(
            plugin.status().await["last_writeback"]["source"],
            "a-source"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn lifecycle_ignores_nonfirst_children_and_handles_auto_parent_completion() {
        let root =
            std::env::temp_dir().join(format!("planeai-jira-lifecycle-{}", generate_state()));
        let mut plugin = JiraPlugin {
            data_dir: root.join("data"),
            secrets_dir: root.join("secrets"),
            pending_auth: None,
            completion: None,
            authorization_error: None,
            completed_attempt: None,
            last_writeback: None,
            client: Client::new(),
            token_url: TOKEN_URL.to_string(),
            resources_url: RESOURCES_URL.to_string(),
        };
        std::fs::create_dir_all(&plugin.data_dir).unwrap();
        std::fs::create_dir_all(&plugin.secrets_dir).unwrap();
        plugin
            .save_settings(&json!({"sources": {"source": {
                "jql": "project = PLA", "writeback": {"on_complete": "Done"}
            }}}))
            .unwrap();
        let conn = plugin.database().unwrap();
        upsert_issue(&conn, &issue_for_test("PLA-288"), "source", "todo").unwrap();
        sync_issue_source(&conn, "PLA-288", "source").unwrap();

        let response = plugin.task_lifecycle(&json!({"batch": {"events": [
            {"type": "child_assigned", "child_key": "PLA-289", "parent_key": "PLA-288", "is_first_child_assignment": false},
            {"type": "status_changed", "task_key": "PLA-288", "previous_status": "in_progress", "new_status": "done", "cause": "automatic_parent_completion"}
        ]}})).await.unwrap();
        assert_eq!(response["outcomes"].as_array().unwrap().len(), 1);
        assert_eq!(response["outcomes"][0]["action"], "complete");
        let _ = std::fs::remove_dir_all(root);
    }

    fn issue_for_test(key: &str) -> FetchedIssue {
        FetchedIssue {
            key: key.to_string(),
            summary: "summary".to_string(),
            description: String::new(),
            status: "Open".to_string(),
            status_category: None,
            priority: None,
            labels: vec![],
        }
    }
}
