use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{timeout, Duration};

use crate::commands;

const HOST_API_VERSION: &str = "planeai.plugin-host.v1";
const RPC_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const PROCESS_MONITOR_INTERVAL: Duration = Duration::from_secs(1);
const MAX_RPC_FRAME_BYTES: u64 = 64 * 1024;
const JIRA_PLUGIN_ID: &str = "jira";
const JIRA_BACKEND_ENTRYPOINT: &str = "planeai-plugin-jira";

#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    pub schema: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub host_api_version: String,
    pub source_kind: PluginSourceKind,
    pub backend_entrypoint: String,
    pub ui_entrypoint: Option<String>,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != "planeai.plugin.v1" {
            return Err(format!(
                "unsupported plugin manifest schema: {}",
                self.schema
            ));
        }
        if self.id.trim().is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err("plugin id must contain lowercase letters, digits, or hyphens".to_string());
        }
        if self.name.trim().is_empty() || self.version.trim().is_empty() {
            return Err("plugin name and version are required".to_string());
        }
        if self.host_api_version != HOST_API_VERSION {
            return Err(format!(
                "plugin {} requires unsupported host API {}",
                self.id, self.host_api_version
            ));
        }
        if self.backend_entrypoint.trim().is_empty() {
            return Err("plugin backend_entrypoint is required".to_string());
        }
        if self.source_kind == PluginSourceKind::Builtin
            && self.backend_entrypoint != JIRA_BACKEND_ENTRYPOINT
        {
            return Err(format!(
                "builtin plugin {} has an untrusted backend entrypoint",
                self.id
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginSourceKind {
    Builtin,
    Local,
}

impl PluginSourceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Local => "local",
        }
    }

    fn from_db(value: &str) -> Self {
        match value {
            "local" => Self::Local,
            _ => Self::Builtin,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginRuntimeState {
    Disabled,
    Starting,
    Running,
    Stopping,
    Error,
}

impl PluginRuntimeState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Error => "error",
        }
    }

    fn from_db(value: &str) -> Self {
        match value {
            "starting" => Self::Starting,
            "running" => Self::Running,
            "stopping" => Self::Stopping,
            "error" => Self::Error,
            _ => Self::Disabled,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginInventory {
    pub id: String,
    pub name: String,
    pub version: String,
    pub host_api_version: String,
    pub source_kind: PluginSourceKind,
    pub backend_entrypoint: String,
    pub ui_entrypoint: Option<String>,
    pub enabled: bool,
    pub state: PluginRuntimeState,
    pub last_error: Option<String>,
    pub log_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JiraPluginStatus {
    pub plugin_id: String,
    pub plugin_name: String,
    pub plugin_version: String,
    pub host_api_version: String,
    pub runtime_state: PluginRuntimeState,
    pub last_error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PluginHandshake {
    plugin_id: String,
    plugin_name: String,
    plugin_version: String,
    host_api_version: String,
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

/// Encode one complete JSON-RPC message frame. The process transport is JSONL,
/// so every request and response must have exactly one trailing newline.
pub fn encode_json_rpc_line(id: u64, method: &str, params: Value) -> Result<String, String> {
    let value = serde_json::to_string(&JsonRpcRequest {
        jsonrpc: "2.0",
        id,
        method,
        params,
    })
    .map_err(|e| format!("failed to encode JSON-RPC request: {e}"))?;
    let frame = format!("{value}\n");
    if frame.len() > MAX_RPC_FRAME_BYTES as usize {
        return Err("plugin JSON-RPC request exceeded the frame limit".to_string());
    }
    Ok(frame)
}

fn decode_json_rpc_response(line: &str, expected_id: u64) -> Result<Value, String> {
    let response: JsonRpcResponse =
        serde_json::from_str(line).map_err(|e| format!("malformed JSON-RPC response: {e}"))?;
    if response.jsonrpc != "2.0" {
        return Err("malformed JSON-RPC response: expected jsonrpc 2.0".to_string());
    }
    if response.id.as_u64() != Some(expected_id) {
        return Err("malformed JSON-RPC response: response id did not match request".to_string());
    }
    if let Some(error) = response.error {
        return Err(format!(
            "plugin RPC error {}: {}",
            error.code, error.message
        ));
    }
    response
        .result
        .ok_or_else(|| "malformed JSON-RPC response: missing result".to_string())
}

pub fn bundled_manifests() -> Result<Vec<PluginManifest>, String> {
    let manifest: PluginManifest =
        serde_json::from_str(include_str!("../plugins/jira/planeai-plugin.json"))
            .map_err(|e| format!("failed to parse bundled Jira plugin manifest: {e}"))?;
    manifest.validate()?;
    Ok(vec![manifest])
}

fn bundled_manifest(plugin_id: &str) -> Result<PluginManifest, String> {
    bundled_manifests()?
        .into_iter()
        .find(|manifest| manifest.id == plugin_id)
        .ok_or_else(|| format!("unknown bundled plugin: {plugin_id}"))
}

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS plugin_inventory (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            version TEXT NOT NULL,
            host_api_version TEXT NOT NULL,
            source_kind TEXT NOT NULL CHECK (source_kind IN ('builtin', 'local')),
            backend_entrypoint TEXT NOT NULL,
            ui_entrypoint TEXT,
            enabled INTEGER NOT NULL DEFAULT 0,
            runtime_state TEXT NOT NULL DEFAULT 'disabled'
                CHECK (runtime_state IN ('disabled', 'starting', 'running', 'stopping', 'error')),
            last_error TEXT,
            log_path TEXT,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );",
    )
}

pub fn sync_inventory(conn: &Connection, manifests: &[PluginManifest]) -> Result<(), String> {
    for manifest in manifests {
        manifest.validate()?;
        conn.execute(
            "INSERT INTO plugin_inventory (
                id, name, version, host_api_version, source_kind, backend_entrypoint, ui_entrypoint
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                version = excluded.version,
                host_api_version = excluded.host_api_version,
                source_kind = excluded.source_kind,
                backend_entrypoint = excluded.backend_entrypoint,
                ui_entrypoint = excluded.ui_entrypoint,
                updated_at = CURRENT_TIMESTAMP",
            params![
                manifest.id,
                manifest.name,
                manifest.version,
                manifest.host_api_version,
                manifest.source_kind.as_str(),
                manifest.backend_entrypoint,
                manifest.ui_entrypoint,
            ],
        )
        .map_err(|e| format!("failed to persist plugin inventory: {e}"))?;
    }
    Ok(())
}

pub fn reconcile_interrupted_runs(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE plugin_inventory
         SET enabled = 0,
             runtime_state = 'error',
             last_error = 'PlaneAI stopped while this plugin runtime was active',
             updated_at = CURRENT_TIMESTAMP
         WHERE runtime_state IN ('starting', 'running', 'stopping')",
        [],
    )
}

pub fn list_inventory(conn: &Connection) -> rusqlite::Result<Vec<PluginInventory>> {
    let mut statement = conn.prepare(
        "SELECT id, name, version, host_api_version, source_kind, backend_entrypoint,
                ui_entrypoint, enabled, runtime_state, last_error, log_path
         FROM plugin_inventory ORDER BY name COLLATE NOCASE",
    )?;
    let rows = statement.query_map([], row_to_inventory)?;
    rows.collect()
}

pub fn get_inventory(
    conn: &Connection,
    plugin_id: &str,
) -> rusqlite::Result<Option<PluginInventory>> {
    conn.query_row(
        "SELECT id, name, version, host_api_version, source_kind, backend_entrypoint,
                ui_entrypoint, enabled, runtime_state, last_error, log_path
         FROM plugin_inventory WHERE id = ?1",
        [plugin_id],
        row_to_inventory,
    )
    .optional()
}

fn row_to_inventory(row: &rusqlite::Row<'_>) -> rusqlite::Result<PluginInventory> {
    let source_kind: String = row.get(4)?;
    let state: String = row.get(8)?;
    Ok(PluginInventory {
        id: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        host_api_version: row.get(3)?,
        source_kind: PluginSourceKind::from_db(&source_kind),
        backend_entrypoint: row.get(5)?,
        ui_entrypoint: row.get(6)?,
        enabled: row.get(7)?,
        state: PluginRuntimeState::from_db(&state),
        last_error: row.get(9)?,
        log_path: row.get(10)?,
    })
}

fn mark_starting(conn: &Connection, plugin_id: &str, log_path: &str) -> Result<(), String> {
    let changed = conn
        .execute(
            "UPDATE plugin_inventory
             SET enabled = 1, runtime_state = 'starting', last_error = NULL, log_path = ?2,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1 AND runtime_state IN ('disabled', 'error')",
            params![plugin_id, log_path],
        )
        .map_err(|e| format!("failed to start plugin: {e}"))?;
    if changed != 1 {
        return Err(format!(
            "plugin {plugin_id} is already starting, running, or stopping"
        ));
    }
    Ok(())
}

fn set_state(
    conn: &Connection,
    plugin_id: &str,
    enabled: bool,
    state: PluginRuntimeState,
    last_error: Option<&str>,
) -> Result<(), String> {
    let changed = conn
        .execute(
            "UPDATE plugin_inventory
             SET enabled = ?2, runtime_state = ?3, last_error = ?4, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
            params![plugin_id, enabled, state.as_str(), last_error],
        )
        .map_err(|e| format!("failed to update plugin state: {e}"))?;
    if changed != 1 {
        return Err(format!("plugin inventory entry not found: {plugin_id}"));
    }
    Ok(())
}

pub struct PluginRuntimeHandle(pub Arc<PluginRuntimeSupervisor>);

impl PluginRuntimeHandle {
    pub fn new(db: Arc<Mutex<Connection>>, app: AppHandle) -> Self {
        Self(Arc::new(PluginRuntimeSupervisor {
            db,
            app,
            processes: Arc::new(AsyncMutex::new(HashMap::new())),
            lifecycle: Arc::new(AsyncMutex::new(())),
            shutting_down: AtomicBool::new(false),
            exit_permitted: AtomicBool::new(false),
        }))
    }
}

pub struct PluginRuntimeSupervisor {
    db: Arc<Mutex<Connection>>,
    app: AppHandle,
    processes: Arc<AsyncMutex<HashMap<String, Arc<AsyncMutex<RuntimeProcess>>>>>,
    lifecycle: Arc<AsyncMutex<()>>,
    shutting_down: AtomicBool,
    exit_permitted: AtomicBool,
}

struct RuntimeProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_request_id: u64,
}

impl RuntimeProcess {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_request_id += 1;
        let request_id = self.next_request_id;
        let frame = encode_json_rpc_line(request_id, method, params)?;
        self.stdin
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| format!("failed to write plugin JSON-RPC request: {e}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("failed to flush plugin JSON-RPC request: {e}"))?;

        let mut bytes = Vec::new();
        let bytes_read = timeout(
            RPC_TIMEOUT,
            (&mut self.stdout)
                .take(MAX_RPC_FRAME_BYTES)
                .read_until(b'\n', &mut bytes),
        )
        .await
        .map_err(|_| format!("plugin RPC {method} timed out"))?
        .map_err(|e| format!("failed to read plugin JSON-RPC response: {e}"))?;
        if bytes_read == 0 {
            return Err("plugin process closed stdout unexpectedly".to_string());
        }
        let frame = String::from_utf8(bytes)
            .map_err(|e| format!("plugin JSON-RPC response was not valid UTF-8: {e}"))?;
        decode_json_rpc_frame(&frame, request_id)
    }

    fn exited(&mut self) -> Result<Option<String>, String> {
        self.child
            .try_wait()
            .map_err(|e| format!("failed to check plugin process: {e}"))
            .map(|status| {
                status.map(|value| format!("plugin process exited unexpectedly ({value})"))
            })
    }

    async fn stop(&mut self) -> Result<(), String> {
        let _ = self.request("plugin.shutdown", Value::Null).await;
        match timeout(SHUTDOWN_TIMEOUT, self.child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(format!("failed waiting for plugin shutdown: {e}")),
            Err(_) => {
                self.child
                    .start_kill()
                    .map_err(|e| format!("plugin did not stop and could not be killed: {e}"))?;
                match timeout(SHUTDOWN_TIMEOUT, self.child.wait()).await {
                    Ok(Ok(_)) => Ok(()),
                    Ok(Err(e)) => Err(format!("failed waiting for killed plugin: {e}")),
                    Err(_) => Err("plugin did not exit after being killed".to_string()),
                }
            }
        }
    }
}

impl PluginRuntimeSupervisor {
    async fn with_db<T, F>(&self, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, String> + Send + 'static,
    {
        let db = self.db.clone();
        commands::blocking(move || {
            let conn = db.lock().map_err(|e| e.to_string())?;
            operation(&conn)
        })
        .await
    }

    async fn update_state(
        &self,
        plugin_id: &str,
        enabled: bool,
        state: PluginRuntimeState,
        last_error: Option<String>,
    ) -> Result<(), String> {
        let update_id = plugin_id.to_string();
        let error = last_error.clone();
        self.with_db(move |conn| set_state(conn, &update_id, enabled, state, error.as_deref()))
            .await?;
        self.emit_change(plugin_id).await;
        Ok(())
    }

    async fn emit_change(&self, plugin_id: &str) {
        match self.inventory(plugin_id).await {
            Ok(Some(inventory)) => {
                if let Err(error) = self.app.emit("plugin-runtime-changed", inventory) {
                    tracing::warn!(plugin_id, %error, "failed to emit plugin runtime update");
                }
            }
            Ok(None) => tracing::warn!(plugin_id, "plugin runtime update had no inventory entry"),
            Err(error) => tracing::warn!(plugin_id, %error, "failed to load plugin runtime update"),
        }
    }

    pub async fn list(&self) -> Result<Vec<PluginInventory>, String> {
        self.with_db(|conn| list_inventory(conn).map_err(|e| e.to_string()))
            .await
    }

    pub async fn inventory(&self, plugin_id: &str) -> Result<Option<PluginInventory>, String> {
        let id = plugin_id.to_string();
        self.with_db(move |conn| get_inventory(conn, &id).map_err(|e| e.to_string()))
            .await
    }

    pub fn begin_shutdown(&self) -> bool {
        self.shutting_down
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn permit_exit(&self) {
        self.exit_permitted.store(true, Ordering::Release);
    }

    pub fn exit_is_permitted(&self) -> bool {
        self.exit_permitted.load(Ordering::Acquire)
    }

    pub async fn shutdown_all(&self) {
        let _lifecycle = self.lifecycle.lock().await;
        self.shutdown_all_inner().await;
    }

    pub async fn shutdown_for_update(&self) -> Result<Vec<String>, String> {
        let _lifecycle = self.lifecycle.lock().await;
        let enabled_plugin_ids = match self.list().await {
            Ok(inventory) => inventory
                .into_iter()
                .filter(|plugin| plugin.enabled)
                .map(|plugin| plugin.id)
                .collect::<Vec<_>>(),
            Err(error) => {
                self.shutting_down.store(false, Ordering::Release);
                return Err(error);
            }
        };
        let plugin_ids = self
            .processes
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for plugin_id in plugin_ids {
            if let Err(error) = self.disable_inner(&plugin_id).await {
                tracing::warn!(plugin_id, %error, "failed to stop plugin runtime before update");
                self.restore_after_failed_update_inner(&enabled_plugin_ids)
                    .await;
                return Err(format!(
                    "failed to stop plugin runtime before update: {error}"
                ));
            }
        }
        Ok(enabled_plugin_ids)
    }

    pub async fn restore_after_failed_update(&self, plugin_ids: &[String]) {
        let _lifecycle = self.lifecycle.lock().await;
        self.restore_after_failed_update_inner(plugin_ids).await;
    }

    async fn restore_after_failed_update_inner(&self, plugin_ids: &[String]) {
        self.shutting_down.store(false, Ordering::Release);
        for plugin_id in plugin_ids {
            if let Err(error) = self.enable_inner(plugin_id).await {
                tracing::warn!(plugin_id, %error, "failed to restore plugin runtime after update install failure");
            }
        }
    }

    async fn shutdown_all_inner(&self) {
        let plugin_ids = self
            .processes
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for plugin_id in plugin_ids {
            if let Err(error) = self.disable_inner(&plugin_id).await {
                tracing::warn!(plugin_id, %error, "failed to stop plugin runtime during shutdown");
            }
        }
    }

    pub async fn enable(&self, plugin_id: &str) -> Result<PluginInventory, String> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("plugin runtime is shutting down".to_string());
        }
        self.enable_inner(plugin_id).await
    }

    async fn enable_inner(&self, plugin_id: &str) -> Result<PluginInventory, String> {
        let manifest = bundled_manifest(plugin_id)?;
        let app = self.app.clone();
        let entrypoint = manifest.backend_entrypoint.clone();
        let id = manifest.id.clone();
        let log_path = plugin_log_path(&app, &id).await?;

        let starting_id = id.clone();
        let starting_log = log_path.display().to_string();
        self.with_db(move |conn| mark_starting(conn, &starting_id, &starting_log))
            .await?;
        self.emit_change(&id).await;

        let binary =
            match commands::blocking(move || resolve_trusted_binary(&app, &entrypoint)).await {
                Ok(binary) => binary,
                Err(error) => {
                    self.update_state(&id, false, PluginRuntimeState::Error, Some(error.clone()))
                        .await?;
                    return Err(error);
                }
            };

        let process = match spawn_runtime(&binary, &log_path).await {
            Ok(process) => process,
            Err(error) => {
                self.update_state(&id, false, PluginRuntimeState::Error, Some(error.clone()))
                    .await?;
                return Err(error);
            }
        };
        let process = Arc::new(AsyncMutex::new(process));

        let handshake_result = {
            let mut runtime = process.lock().await;
            runtime
                .request(
                    "plugin.handshake",
                    serde_json::json!({ "host_api_version": HOST_API_VERSION }),
                )
                .await
        };
        let handshake: PluginHandshake = match handshake_result.and_then(|value| {
            serde_json::from_value::<PluginHandshake>(value)
                .map_err(|e| format!("invalid plugin handshake: {e}"))
        }) {
            Ok(value)
                if value.plugin_id == manifest.id
                    && value.plugin_name == manifest.name
                    && value.plugin_version == manifest.version
                    && value.host_api_version == HOST_API_VERSION =>
            {
                value
            }
            Ok(_) => {
                let error = "plugin handshake identity or host API version did not match manifest"
                    .to_string();
                let _ = stop_process(process).await;
                self.update_state(&id, false, PluginRuntimeState::Error, Some(error.clone()))
                    .await?;
                return Err(error);
            }
            Err(error) => {
                let _ = stop_process(process).await;
                self.update_state(&id, false, PluginRuntimeState::Error, Some(error.clone()))
                    .await?;
                return Err(error);
            }
        };
        tracing::info!(plugin_id = %handshake.plugin_id, version = %handshake.plugin_version, "plugin runtime handshake completed");

        if let Err(error) = self
            .update_state(&id, true, PluginRuntimeState::Running, None)
            .await
        {
            if let Err(stop_error) = stop_process(process).await {
                tracing::warn!(plugin_id = %id, %stop_error, "failed to stop plugin after startup persistence failure");
            }
            if let Err(recovery_error) = self
                .update_state(&id, false, PluginRuntimeState::Error, Some(error.clone()))
                .await
            {
                tracing::warn!(plugin_id = %id, %recovery_error, "failed to record plugin startup persistence failure");
            }
            return Err(error);
        }
        self.processes
            .lock()
            .await
            .insert(id.clone(), process.clone());
        self.monitor_process(id.clone(), process);
        self.inventory(&id)
            .await?
            .ok_or_else(|| format!("plugin inventory entry not found after startup: {id}"))
    }

    pub async fn disable(&self, plugin_id: &str) -> Result<PluginInventory, String> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("plugin runtime is shutting down".to_string());
        }
        self.disable_inner(plugin_id).await
    }

    async fn disable_inner(&self, plugin_id: &str) -> Result<PluginInventory, String> {
        bundled_manifest(plugin_id)?;
        let inventory = self
            .inventory(plugin_id)
            .await?
            .ok_or_else(|| format!("plugin inventory entry not found: {plugin_id}"))?;
        if inventory.state == PluginRuntimeState::Disabled {
            return Ok(inventory);
        }

        self.update_state(
            plugin_id,
            inventory.enabled,
            PluginRuntimeState::Stopping,
            inventory.last_error.clone(),
        )
        .await?;
        let process = self.processes.lock().await.remove(plugin_id);
        if let Some(process) = process {
            if let Err(error) = stop_process(process).await {
                self.update_state(
                    plugin_id,
                    false,
                    PluginRuntimeState::Error,
                    Some(error.clone()),
                )
                .await?;
                return Err(error);
            }
        }
        self.update_state(plugin_id, false, PluginRuntimeState::Disabled, None)
            .await?;
        self.inventory(plugin_id)
            .await?
            .ok_or_else(|| format!("plugin inventory entry not found after shutdown: {plugin_id}"))
    }

    pub async fn reload(&self, plugin_id: &str) -> Result<PluginInventory, String> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("plugin runtime is shutting down".to_string());
        }
        self.disable_inner(plugin_id).await?;
        self.enable_inner(plugin_id).await
    }

    fn monitor_process(&self, plugin_id: String, process: Arc<AsyncMutex<RuntimeProcess>>) {
        let db = self.db.clone();
        let app = self.app.clone();
        let processes = self.processes.clone();
        let lifecycle = self.lifecycle.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(PROCESS_MONITOR_INTERVAL).await;
                let outcome = {
                    let mut runtime = process.lock().await;
                    runtime.exited()
                };
                let (error, stop_process_after_removal) = match outcome {
                    Ok(Some(error)) => (error, false),
                    Ok(None) => continue,
                    Err(error) => (error, true),
                };

                let _lifecycle = lifecycle.lock().await;
                let owns_process = {
                    let mut active_processes = processes.lock().await;
                    remove_current_process(&mut active_processes, &plugin_id, &process)
                };
                if !owns_process {
                    return;
                }
                if stop_process_after_removal {
                    if let Err(stop_error) = stop_process(process.clone()).await {
                        tracing::warn!(plugin_id, %stop_error, "failed to stop unhealthy plugin runtime");
                    }
                }
                if let Err(update_error) =
                    record_monitored_process_failure(db, app, plugin_id.clone(), error).await
                {
                    tracing::warn!(plugin_id, %update_error, "failed to record plugin runtime exit");
                }
                return;
            }
        });
    }

    pub async fn jira_status(&self, plugin_id: &str) -> Result<JiraPluginStatus, String> {
        if plugin_id != JIRA_PLUGIN_ID {
            return Err(format!("plugin does not expose jira.status: {plugin_id}"));
        }
        let _lifecycle = self.lifecycle.lock().await;
        let inventory = self
            .inventory(plugin_id)
            .await?
            .ok_or_else(|| format!("plugin inventory entry not found: {plugin_id}"))?;
        if inventory.state != PluginRuntimeState::Running {
            return Ok(status_from_inventory(&inventory));
        }

        let process = self.processes.lock().await.get(plugin_id).cloned();
        let Some(process) = process else {
            let error = "plugin runtime was not available".to_string();
            self.update_state(plugin_id, false, PluginRuntimeState::Error, Some(error))
                .await?;
            let inventory = self
                .inventory(plugin_id)
                .await?
                .expect("inventory remains present");
            return Ok(status_from_inventory(&inventory));
        };

        let response = {
            let mut runtime = process.lock().await;
            match runtime.exited()? {
                Some(error) => Err(error),
                None => runtime.request("jira.status", Value::Null).await,
            }
        };
        match response.and_then(|value| {
            serde_json::from_value::<JiraPluginStatus>(value)
                .map_err(|e| format!("invalid jira.status response: {e}"))
        }) {
            Ok(status)
                if status.plugin_id == inventory.id
                    && status.plugin_version == inventory.version
                    && status.host_api_version == inventory.host_api_version =>
            {
                Ok(status)
            }
            Ok(_) => {
                self.fail_runtime(plugin_id, "jira.status identity did not match manifest")
                    .await
            }
            Err(error) => self.fail_runtime(plugin_id, &error).await,
        }
    }

    async fn fail_runtime(&self, plugin_id: &str, error: &str) -> Result<JiraPluginStatus, String> {
        let process = self.processes.lock().await.remove(plugin_id);
        if let Some(process) = process {
            if let Err(stop_error) = stop_process(process).await {
                tracing::warn!(plugin_id, %stop_error, "failed to stop unhealthy plugin runtime");
            }
        }
        self.update_state(
            plugin_id,
            false,
            PluginRuntimeState::Error,
            Some(error.to_string()),
        )
        .await?;
        let inventory = self
            .inventory(plugin_id)
            .await?
            .expect("inventory remains present");
        Ok(status_from_inventory(&inventory))
    }
}

fn decode_json_rpc_frame(frame: &str, request_id: u64) -> Result<Value, String> {
    let line = frame
        .strip_suffix('\n')
        .ok_or_else(|| "plugin JSON-RPC response was not newline terminated".to_string())?;
    decode_json_rpc_response(line, request_id)
}

fn status_from_inventory(inventory: &PluginInventory) -> JiraPluginStatus {
    JiraPluginStatus {
        plugin_id: inventory.id.clone(),
        plugin_name: inventory.name.clone(),
        plugin_version: inventory.version.clone(),
        host_api_version: inventory.host_api_version.clone(),
        runtime_state: inventory.state,
        last_error: inventory.last_error.clone(),
    }
}

async fn plugin_log_path(app: &AppHandle, plugin_id: &str) -> Result<PathBuf, String> {
    use tauri::Manager as _;
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve plugin log directory: {e}"))?
        .join("plugins");
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|e| format!("failed to create plugin log directory: {e}"))?;
    Ok(directory.join(format!("{plugin_id}.log")))
}

async fn spawn_runtime(binary: &Path, log_path: &Path) -> Result<RuntimeProcess, String> {
    let mut command = Command::new(binary);
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    planeai_core::command::no_window_tokio(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("failed to spawn plugin runtime {}: {e}", binary.display()))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "plugin runtime did not provide stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "plugin runtime did not provide stdout".to_string())?;
    if let Some(stderr) = child.stderr.take() {
        drain_stderr(stderr, log_path.to_path_buf());
    }
    Ok(RuntimeProcess {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        next_request_id: 0,
    })
}

fn drain_stderr(mut stderr: ChildStderr, log_path: PathBuf) {
    tokio::spawn(async move {
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .await;
        let Ok(mut file) = file else {
            tracing::warn!(path = %log_path.display(), "failed to open plugin stderr log");
            return;
        };
        if let Err(error) = tokio::io::copy(&mut stderr, &mut file).await {
            tracing::warn!(path = %log_path.display(), %error, "failed to drain plugin stderr");
        }
    });
}

async fn stop_process(process: Arc<AsyncMutex<RuntimeProcess>>) -> Result<(), String> {
    let mut runtime = process.lock().await;
    runtime.stop().await
}

fn remove_current_process<T>(
    processes: &mut HashMap<String, Arc<T>>,
    plugin_id: &str,
    expected: &Arc<T>,
) -> bool {
    match processes.get(plugin_id) {
        Some(current) if Arc::ptr_eq(current, expected) => processes.remove(plugin_id).is_some(),
        _ => false,
    }
}

async fn record_monitored_process_failure(
    db: Arc<Mutex<Connection>>,
    app: AppHandle,
    plugin_id: String,
    error: String,
) -> Result<(), String> {
    let update_id = plugin_id.clone();
    let inventory = commands::blocking(move || {
        let conn = db.lock().map_err(|e| e.to_string())?;
        set_state(
            &conn,
            &update_id,
            false,
            PluginRuntimeState::Error,
            Some(&error),
        )?;
        get_inventory(&conn, &update_id).map_err(|e| e.to_string())
    })
    .await?
    .ok_or_else(|| format!("plugin inventory entry not found: {plugin_id}"))?;
    if let Err(emit_error) = app.emit("plugin-runtime-changed", inventory) {
        tracing::warn!(plugin_id, %emit_error, "failed to emit plugin runtime update");
    }
    Ok(())
}

fn packaged_linux_sidecar_path(resource_dir: &Path, binary_name: &str) -> Option<PathBuf> {
    let lib_dir = resource_dir.parent()?;
    let usr_dir = lib_dir.parent()?;
    (lib_dir.file_name()? == "lib" && usr_dir.file_name()? == "usr")
        .then(|| usr_dir.join("bin").join(binary_name))
}

fn packaged_windows_sidecar_path(
    resource_dir: &Path,
    executable: &Path,
    binary_name: &str,
) -> Option<PathBuf> {
    let executable_dir = executable.parent()?;
    (resource_dir.file_name()? == "resources" && resource_dir.parent()? == executable_dir)
        .then(|| executable_dir.join(binary_name))
}

fn is_packaged_macos_binary_dir(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "MacOS")
        && path.parent().is_some_and(|contents_dir| {
            contents_dir
                .file_name()
                .is_some_and(|name| name == "Contents")
                && contents_dir
                    .parent()
                    .is_some_and(|app_dir| app_dir.extension().is_some_and(|ext| ext == "app"))
        })
}

fn resolve_trusted_binary(app: &AppHandle, entrypoint: &str) -> Result<PathBuf, String> {
    use tauri::Manager as _;
    if entrypoint != JIRA_BACKEND_ENTRYPOINT {
        return Err(format!("untrusted plugin backend entrypoint: {entrypoint}"));
    }
    let binary_name = if cfg!(windows) {
        format!("{entrypoint}.exe")
    } else {
        entrypoint.to_string()
    };
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled = resource_dir.join(&binary_name);
        if bundled.is_file() {
            return Ok(bundled);
        }
        if cfg!(target_os = "linux") {
            if let Some(bundled) = packaged_linux_sidecar_path(&resource_dir, &binary_name) {
                if bundled.is_file() {
                    return Ok(bundled);
                }
            }
        }
        if cfg!(target_os = "windows") {
            if let Ok(executable) = std::env::current_exe() {
                if let Some(bundled) =
                    packaged_windows_sidecar_path(&resource_dir, &executable, &binary_name)
                {
                    if bundled.is_file() {
                        return Ok(bundled);
                    }
                }
            }
        }
    }
    if cfg!(target_os = "macos") {
        if let Ok(executable) = std::env::current_exe() {
            if let Some(macos_dir) = executable.parent() {
                let bundled = macos_dir.join(&binary_name);
                if is_packaged_macos_binary_dir(macos_dir) && bundled.is_file() {
                    return Ok(bundled);
                }
            }
        }
    }
    if cfg!(debug_assertions) {
        if let Ok(executable) = std::env::current_exe() {
            if let Some(directory) = executable.parent() {
                let sibling = directory.join(&binary_name);
                if sibling.is_file() {
                    return Ok(sibling);
                }
            }
        }
    }
    Err(format!(
        "bundled plugin runtime {binary_name} was not found; it must be packaged with PlaneAI"
    ))
}

trait OptionalRow<T> {
    fn optional(self) -> rusqlite::Result<Option<T>>;
}

impl<T> OptionalRow<T> for rusqlite::Result<T> {
    fn optional(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        sync_inventory(&conn, &bundled_manifests().unwrap()).unwrap();
        conn
    }

    #[test]
    fn bundled_jira_manifest_is_valid_and_disabled_by_default() {
        let manifest = bundled_manifests().unwrap().pop().unwrap();
        assert_eq!(manifest.id, "jira");
        assert_eq!(manifest.source_kind, PluginSourceKind::Builtin);
        let conn = database();
        let jira = get_inventory(&conn, "jira").unwrap().unwrap();
        assert!(!jira.enabled);
        assert_eq!(jira.state, PluginRuntimeState::Disabled);
    }

    #[test]
    fn inventory_persists_lifecycle_transitions() {
        let conn = database();
        mark_starting(&conn, "jira", "/tmp/jira.log").unwrap();
        assert_eq!(
            get_inventory(&conn, "jira").unwrap().unwrap().state,
            PluginRuntimeState::Starting
        );
        set_state(&conn, "jira", true, PluginRuntimeState::Running, None).unwrap();
        set_state(&conn, "jira", true, PluginRuntimeState::Stopping, None).unwrap();
        set_state(&conn, "jira", false, PluginRuntimeState::Disabled, None).unwrap();
        let jira = get_inventory(&conn, "jira").unwrap().unwrap();
        assert!(!jira.enabled);
        assert_eq!(jira.state, PluginRuntimeState::Disabled);
        assert_eq!(jira.log_path.as_deref(), Some("/tmp/jira.log"));
    }

    #[test]
    fn plugin_failures_are_isolated_to_their_inventory_record() {
        let conn = database();
        let local = PluginManifest {
            schema: "planeai.plugin.v1".into(),
            id: "local-test".into(),
            name: "Local test".into(),
            version: "1.0.0".into(),
            host_api_version: HOST_API_VERSION.into(),
            source_kind: PluginSourceKind::Local,
            backend_entrypoint: "local-test".into(),
            ui_entrypoint: None,
        };
        sync_inventory(&conn, &[local]).unwrap();
        set_state(
            &conn,
            "jira",
            false,
            PluginRuntimeState::Error,
            Some("bad rpc"),
        )
        .unwrap();
        let local = get_inventory(&conn, "local-test").unwrap().unwrap();
        assert_eq!(local.state, PluginRuntimeState::Disabled);
        assert!(local.last_error.is_none());
    }

    #[test]
    fn startup_reconciles_interrupted_runtime_without_losing_diagnostics() {
        let conn = database();
        set_state(&conn, "jira", true, PluginRuntimeState::Running, None).unwrap();
        assert_eq!(reconcile_interrupted_runs(&conn).unwrap(), 1);
        let jira = get_inventory(&conn, "jira").unwrap().unwrap();
        assert_eq!(jira.state, PluginRuntimeState::Error);
        assert!(jira.last_error.unwrap().contains("stopped"));
    }

    #[test]
    fn derives_linux_packaged_sidecar_paths_from_resource_directories() {
        assert_eq!(
            packaged_linux_sidecar_path(Path::new("/usr/lib/planeai"), "planeai-plugin-jira"),
            Some(PathBuf::from("/usr/bin/planeai-plugin-jira"))
        );
        assert_eq!(
            packaged_linux_sidecar_path(Path::new("/tmp/resources"), "planeai-plugin-jira"),
            None
        );
    }

    #[test]
    fn derives_windows_packaged_sidecar_paths_from_resource_directories() {
        let executable = Path::new("/Program Files/planeai/planeai.exe");
        assert_eq!(
            packaged_windows_sidecar_path(
                Path::new("/Program Files/planeai/resources"),
                executable,
                "planeai-plugin-jira.exe"
            ),
            Some(PathBuf::from(
                "/Program Files/planeai/planeai-plugin-jira.exe"
            ))
        );
        assert_eq!(
            packaged_windows_sidecar_path(
                Path::new("/tmp/resources"),
                executable,
                "planeai-plugin-jira.exe"
            ),
            None
        );
    }

    #[test]
    fn recognizes_only_packaged_macos_binary_directories() {
        assert!(is_packaged_macos_binary_dir(Path::new(
            "/Applications/planeai.app/Contents/MacOS"
        )));
        assert!(!is_packaged_macos_binary_dir(Path::new(
            "/tmp/planeai/target/release"
        )));
    }

    #[test]
    fn process_monitor_does_not_remove_a_replacement_runtime() {
        let original = Arc::new(());
        let replacement = Arc::new(());
        let mut processes = HashMap::from([("jira".to_string(), replacement.clone())]);

        assert!(!remove_current_process(&mut processes, "jira", &original));
        assert!(Arc::ptr_eq(processes.get("jira").unwrap(), &replacement));
        assert!(remove_current_process(&mut processes, "jira", &replacement));
        assert!(processes.is_empty());
    }

    #[test]
    fn json_rpc_uses_newline_frames_and_validates_responses() {
        let frame = encode_json_rpc_line(7, "jira.status", Value::Null).unwrap();
        assert!(frame.ends_with('\n'));
        assert!(
            encode_json_rpc_line(7, &"x".repeat(MAX_RPC_FRAME_BYTES as usize), Value::Null)
                .unwrap_err()
                .contains("exceeded")
        );
        let value = decode_json_rpc_frame(
            r#"{"jsonrpc":"2.0","id":7,"result":{"ok":true}}
"#,
            7,
        )
        .unwrap();
        assert_eq!(value["ok"], true);
        assert!(decode_json_rpc_response("not json", 7)
            .unwrap_err()
            .contains("malformed"));
        assert!(
            decode_json_rpc_frame(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#, 7)
                .unwrap_err()
                .contains("newline terminated")
        );
        assert!(
            decode_json_rpc_response(r#"{"jsonrpc":"2.0","id":8,"result":{}}"#, 7)
                .unwrap_err()
                .contains("did not match")
        );
    }
}
