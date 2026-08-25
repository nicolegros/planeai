use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use planeai_tasks::model::{CreateParams, Status, UpdateParams};
use planeai_tasks::provider::TaskProvider;
use planeai_tasks::sqlite::SqliteRepository;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{timeout, timeout_at, Duration, Instant};

use crate::commands;
use crate::task_lifecycle::TaskLifecycleBatch;

const HOST_API_VERSION: &str = "planeai.plugin-host.v1";
const RPC_TIMEOUT: Duration = Duration::from_secs(5);
// A Jira lifecycle writeback may refresh credentials, look up a transition,
// perform that transition, and add a comment. Each network request is bounded
// by the Jira client at 20 seconds, so this must cover the full sequence.
const JIRA_LIFECYCLE_RPC_TIMEOUT: Duration = Duration::from_secs(90);
// Synchronization can legitimately fetch up to 100 Jira pages before its first
// nested host task request. Keep a finite watchdog, but allow that bounded fetch.
const JIRA_SYNC_RPC_TIMEOUT: Duration = Duration::from_secs(40 * 60);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const PROCESS_MONITOR_INTERVAL: Duration = Duration::from_secs(1);
const MAX_RPC_FRAME_BYTES: u64 = 64 * 1024;
const JIRA_PLUGIN_ID: &str = "jira";
const JIRA_BACKEND_ENTRYPOINT: &str = "planeai-plugin-jira";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub schema: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub host_api_version: String,
    pub source_kind: PluginSourceKind,
    #[serde(default)]
    pub backend_entrypoint: Option<String>,
    #[serde(default)]
    pub backend_entrypoints: HashMap<String, String>,
    #[serde(default)]
    pub ui_contributions: Vec<PluginUiContribution>,
    #[serde(default)]
    pub capabilities: Vec<PluginHostCapability>,
    #[serde(default, rename = "ui_entrypoint")]
    legacy_ui_entrypoint: LegacyUiEntrypoint,
}

/// Keeps the legacy field's three wire states distinct: missing fields are
/// accepted for bundled-manifest migration, while local manifests can reject a
/// deliberately supplied `null` just as they reject a legacy string value.
#[derive(Debug, Clone, Default)]
enum LegacyUiEntrypoint {
    #[default]
    Absent,
    Null,
    Value(String),
}

impl<'de> Deserialize<'de> for LegacyUiEntrypoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match Option::<String>::deserialize(deserializer)? {
            Some(entrypoint) => Self::Value(entrypoint),
            None => Self::Null,
        })
    }
}

impl LegacyUiEntrypoint {
    fn as_deref(&self) -> Option<&str> {
        match self {
            Self::Value(entrypoint) => Some(entrypoint),
            Self::Absent | Self::Null => None,
        }
    }

    fn is_present(&self) -> bool {
        !matches!(self, Self::Absent)
    }
}

impl PluginManifest {
    pub fn effective_ui_contributions(&self) -> Vec<PluginUiContribution> {
        if !self.ui_contributions.is_empty() {
            return self.ui_contributions.clone();
        }
        self.legacy_ui_entrypoint
            .as_deref()
            .filter(|entrypoint| !entrypoint.trim().is_empty())
            .map(|entrypoint| {
                vec![PluginUiContribution {
                    id: "legacy-main-pane".to_string(),
                    label: self.name.clone(),
                    placement: PluginUiPlacement::MainPane,
                    entrypoint: entrypoint.to_string(),
                    order: None,
                    shortcut: None,
                }]
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub enum PluginHostCapability {
    #[serde(rename = "settings")]
    Settings,
    #[serde(rename = "tasks.read")]
    TasksRead,
    #[serde(rename = "task-events")]
    TaskEvents,
    #[serde(rename = "tasks.create")]
    TasksCreate,
    #[serde(rename = "tasks.update")]
    TasksUpdate,
    #[serde(rename = "storage")]
    Storage,
    #[serde(rename = "sidebar.navigation")]
    SidebarNavigation,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum PluginUiPlacement {
    #[serde(rename = "sidebar.header")]
    SidebarHeader,
    #[serde(rename = "sidebar.navigation")]
    SidebarNavigation,
    #[serde(rename = "sidebar.section")]
    SidebarSection,
    #[serde(rename = "sidebar.footer")]
    SidebarFooter,
    #[serde(rename = "preferences")]
    Preferences,
    #[serde(rename = "main-pane")]
    MainPane,
}

impl PluginUiPlacement {
    fn is_sidebar(self) -> bool {
        matches!(
            self,
            Self::SidebarHeader
                | Self::SidebarNavigation
                | Self::SidebarSection
                | Self::SidebarFooter
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginUiContribution {
    pub id: String,
    pub label: String,
    pub placement: PluginUiPlacement,
    pub entrypoint: String,
    #[serde(default)]
    pub order: Option<i32>,
    #[serde(default)]
    pub shortcut: Option<String>,
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
        if self.source_kind == PluginSourceKind::Local && self.legacy_ui_entrypoint.is_present() {
            return Err(
                "local plugin manifests must use ui_contributions instead of legacy ui_entrypoint"
                    .to_string(),
            );
        }
        match self.source_kind {
            PluginSourceKind::Builtin => {
                if self.backend_entrypoint.as_deref() != Some(JIRA_BACKEND_ENTRYPOINT) {
                    return Err(format!(
                        "builtin plugin {} has an untrusted backend entrypoint",
                        self.id
                    ));
                }
            }
            PluginSourceKind::Local if self.backend_entrypoints.is_empty() => {
                return Err("local plugin backend_entrypoints is required".to_string());
            }
            PluginSourceKind::Local => {
                let platform = self
                    .backend_entrypoints
                    .keys()
                    .next()
                    .expect("nonempty local backend entrypoints were checked");
                let manifest = serde_json::json!({
                    "schema": self.schema,
                    "id": self.id,
                    "name": self.name,
                    "version": self.version,
                    "host_api_version": self.host_api_version,
                    "source_kind": "local",
                    "backend_entrypoint": self.backend_entrypoint,
                    "backend_entrypoints": self.backend_entrypoints,
                    "ui_contributions": self.ui_contributions,
                    "capabilities": self.capabilities,
                });
                planeai_plugin_contract::validate_local_manifest(&manifest, platform)
                    .map_err(|error| error.to_string())?;
                return Ok(());
            }
        }
        if self
            .legacy_ui_entrypoint
            .as_deref()
            .is_some_and(|entrypoint| entrypoint.trim().is_empty())
        {
            return Err("legacy ui_entrypoint must not be empty".to_string());
        }
        validate_ui_contributions(&self.id, &self.effective_ui_contributions())?;
        validate_capabilities(self.source_kind, &self.capabilities)
    }
}

fn validate_capabilities(
    source_kind: PluginSourceKind,
    capabilities: &[PluginHostCapability],
) -> Result<(), String> {
    let unique = capabilities.iter().collect::<HashSet<_>>();
    if unique.len() != capabilities.len() {
        return Err("plugin manifest declares duplicate capabilities".to_string());
    }
    if source_kind == PluginSourceKind::Local
        && capabilities.iter().any(|capability| {
            !matches!(
                capability,
                PluginHostCapability::Settings
                    | PluginHostCapability::TasksRead
                    | PluginHostCapability::TaskEvents
            )
        })
    {
        return Err(
            "local plugins may only request settings, tasks.read, or task-events capabilities"
                .to_string(),
        );
    }
    Ok(())
}

fn validate_ui_contributions(
    plugin_id: &str,
    contributions: &[PluginUiContribution],
) -> Result<(), String> {
    let mut ids = HashSet::new();
    let mut shortcuts = HashSet::new();
    for contribution in contributions {
        if contribution.id.trim().is_empty()
            || !contribution
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(
                "UI contribution id must contain lowercase letters, digits, or hyphens".to_string(),
            );
        }
        if !ids.insert(&contribution.id) {
            return Err(format!(
                "plugin {plugin_id} defines duplicate UI contribution {}",
                contribution.id
            ));
        }
        if contribution.label.trim().is_empty() || contribution.entrypoint.trim().is_empty() {
            return Err("UI contribution label and entrypoint are required".to_string());
        }
        crate::plugin_packages::validate_package_path(
            &contribution.entrypoint,
            "UI contribution entrypoint",
        )?;
        if !matches!(contribution.placement, PluginUiPlacement::MainPane)
            && contribution.shortcut.is_some()
        {
            return Err(
                "UI contribution shortcuts are only valid for main-pane contributions".to_string(),
            );
        }
        if !contribution.placement.is_sidebar() && contribution.order.is_some() {
            return Err(
                "UI contribution order is only valid for sidebar contributions".to_string(),
            );
        }
        if let Some(shortcut) = &contribution.shortcut {
            validate_shortcut(shortcut)?;
            if !shortcuts.insert(shortcut) {
                return Err(format!(
                    "plugin {plugin_id} defines duplicate UI contribution shortcut {shortcut}"
                ));
            }
        }
    }
    Ok(())
}

fn validate_shortcut(shortcut: &str) -> Result<(), String> {
    let parts = shortcut.split('+').collect::<Vec<_>>();
    let key = parts.last().copied().unwrap_or_default();
    let modifiers = &parts[1..parts.len().saturating_sub(1)];
    let valid_key = key.len() == 1 && key.as_bytes()[0].is_ascii_uppercase();
    if parts.len() < 2
        || parts.first() != Some(&"Mod")
        || !valid_key
        || modifiers
            .iter()
            .any(|modifier| !matches!(*modifier, "Shift" | "Alt"))
        || modifiers.windows(2).any(|pair| pair[0] == pair[1])
    {
        return Err("UI contribution shortcut must use Mod+[Shift+][Alt+]A-Z syntax".to_string());
    }
    let canonical = match modifiers {
        [] => format!("Mod+{key}"),
        ["Shift"] => format!("Mod+Shift+{key}"),
        ["Alt"] => format!("Mod+Alt+{key}"),
        ["Shift", "Alt"] => format!("Mod+Shift+Alt+{key}"),
        _ => {
            return Err(
                "UI contribution shortcut modifiers must be ordered Shift then Alt".to_string(),
            )
        }
    };
    if canonical != shortcut {
        return Err(
            "UI contribution shortcut modifiers must be ordered Shift then Alt".to_string(),
        );
    }
    if matches!(
        key,
        "B" | "D" | "E" | "K" | "N" | "P" | "R" | "S" | "T" | "U" | "W"
    ) {
        return Err(format!(
            "UI contribution shortcut {shortcut} is reserved by PlaneAI"
        ));
    }
    Ok(())
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
    pub ui_contributions: Vec<PluginUiContribution>,
    pub capabilities: Vec<PluginHostCapability>,
    pub installed_hash: Option<String>,
    pub installed_path: Option<String>,
    pub original_display_path: Option<String>,
    pub enabled: bool,
    pub state: PluginRuntimeState,
    pub last_error: Option<String>,
    pub log_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PluginHandshake {
    plugin_id: String,
    plugin_name: String,
    plugin_version: String,
    host_api_version: String,
    #[serde(default)]
    lifecycle_event_subscriptions: HashSet<String>,
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcNotification<'a> {
    jsonrpc: &'static str,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Debug)]
struct JsonRpcCallbackRequest {
    id: Value,
    method: String,
    params: Value,
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
    encode_json_rpc_frame(value, "request")
}

fn encode_json_rpc_cancel_request_line(request_id: u64) -> Result<String, String> {
    let value = serde_json::to_string(&JsonRpcNotification {
        jsonrpc: "2.0",
        method: "$/cancelRequest",
        params: serde_json::json!({ "id": request_id }),
    })
    .map_err(|e| format!("failed to encode JSON-RPC cancellation notification: {e}"))?;
    encode_json_rpc_frame(value, "cancellation notification")
}

fn encode_json_rpc_frame(value: String, kind: &str) -> Result<String, String> {
    let frame = format!("{value}\n");
    if frame.len() > MAX_RPC_FRAME_BYTES as usize {
        return Err(format!("plugin JSON-RPC {kind} exceeded the frame limit"));
    }
    Ok(frame)
}

fn parse_json_rpc_response(
    line: &str,
    expected_id: u64,
) -> Result<(JsonRpcResponse, Option<Value>), String> {
    let value: Value =
        serde_json::from_str(line).map_err(|e| format!("malformed JSON-RPC response: {e}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "malformed JSON-RPC response: expected an object".to_string())?;
    let has_result = object.contains_key("result");
    if has_result == object.contains_key("error") {
        return Err(
            "malformed JSON-RPC response: expected exactly one of result or error".to_string(),
        );
    }
    // Preserve member presence separately from its JSON value: `result: null`
    // is a valid JSON-RPC success response and must not look like a missing field.
    let result = has_result.then(|| object["result"].clone());
    let response: JsonRpcResponse =
        serde_json::from_value(value).map_err(|e| format!("malformed JSON-RPC response: {e}"))?;
    if response.jsonrpc != "2.0" {
        return Err("malformed JSON-RPC response: expected jsonrpc 2.0".to_string());
    }
    if response.id.as_u64() != Some(expected_id) {
        return Err("malformed JSON-RPC response: response id did not match request".to_string());
    }
    Ok((response, result))
}

fn decode_json_rpc_response(line: &str, expected_id: u64) -> Result<Value, String> {
    let (response, result) = parse_json_rpc_response(line, expected_id)?;
    if let Some(error) = response.error {
        return Err(format!(
            "plugin RPC error {}: {}",
            error.code, error.message
        ));
    }
    result.ok_or_else(|| "malformed JSON-RPC response: missing result".to_string())
}

fn decode_json_rpc_cancellation_response(line: &str, expected_id: u64) -> Result<String, String> {
    let (response, _) = parse_json_rpc_response(line, expected_id)?;
    match response.error {
        Some(error) if error.code == -32800 => Ok(format!(
            "plugin RPC error {}: {}",
            error.code, error.message
        )),
        Some(error) => Err(format!(
            "plugin cancellation response used error code {}; expected -32800",
            error.code
        )),
        None => Err("plugin cancellation response must use error code -32800".to_string()),
    }
}

fn timed_out_request_error(method: &str) -> String {
    format!("plugin RPC {method} timed out")
}

fn cancellation_acknowledgement_error(
    method: &str,
    request_id: u64,
    frame: Option<&str>,
) -> String {
    frame
        .and_then(|frame| decode_json_rpc_cancellation_response(frame, request_id).ok())
        .unwrap_or_else(|| timed_out_request_error(method))
}

fn is_rpc_timeout_error(error: &str) -> bool {
    error.starts_with("plugin RPC ") && error.ends_with(" timed out")
}

pub fn bundled_manifests() -> Result<Vec<PluginManifest>, String> {
    let manifest: PluginManifest =
        serde_json::from_str(include_str!("../plugins/jira/planeai-plugin.json"))
            .map_err(|e| format!("failed to parse bundled Jira plugin manifest: {e}"))?;
    manifest.validate()?;
    Ok(vec![manifest])
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
            ui_contributions TEXT NOT NULL DEFAULT '[]',
            capabilities TEXT NOT NULL DEFAULT '[]',
            installed_hash TEXT,
            installed_path TEXT,
            original_display_path TEXT,
            enabled INTEGER NOT NULL DEFAULT 0,
            runtime_state TEXT NOT NULL DEFAULT 'disabled'
                CHECK (runtime_state IN ('disabled', 'starting', 'running', 'stopping', 'error')),
            last_error TEXT,
            log_path TEXT,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );",
    )?;
    for (column, definition) in [
        ("installed_hash", "TEXT"),
        ("installed_path", "TEXT"),
        ("original_display_path", "TEXT"),
        ("ui_contributions", "TEXT NOT NULL DEFAULT '[]'"),
        ("capabilities", "TEXT NOT NULL DEFAULT '[]'"),
    ] {
        let has_column = conn
            .prepare("PRAGMA table_info(plugin_inventory)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .any(|existing| existing == column);
        if !has_column {
            conn.execute_batch(&format!(
                "ALTER TABLE plugin_inventory ADD COLUMN {column} {definition}"
            ))?;
        }
    }
    let legacy_ui_entrypoint = conn
        .prepare("PRAGMA table_info(plugin_inventory)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|column| column == "ui_entrypoint");
    if legacy_ui_entrypoint {
        conn.execute(
            "UPDATE plugin_inventory SET ui_contributions = json_array(json_object('id', 'legacy-main-pane', 'label', name, 'placement', 'main-pane', 'entrypoint', ui_entrypoint, 'order', null, 'shortcut', null)) WHERE source_kind = 'local' AND ui_contributions = '[]' AND ui_entrypoint IS NOT NULL AND trim(ui_entrypoint) != ''",
            [],
        )?;
    }
    Ok(())
}

fn validate_shortcut_collisions(
    conn: &Connection,
    manifest: &PluginManifest,
) -> Result<(), String> {
    let effective_contributions = manifest.effective_ui_contributions();
    let declared = effective_contributions
        .iter()
        .filter_map(|item| item.shortcut.as_deref())
        .collect::<HashSet<_>>();
    if declared.is_empty() {
        return Ok(());
    }
    let mut statement = conn
        .prepare("SELECT id, ui_contributions FROM plugin_inventory WHERE id != ?1")
        .map_err(|error| error.to_string())?;
    let existing = statement
        .query_map([&manifest.id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    for row in existing {
        let (plugin_id, json) = row.map_err(|error| error.to_string())?;
        let contributions: Vec<PluginUiContribution> =
            serde_json::from_str(&json).map_err(|error| {
                format!("failed to parse persisted UI contributions for {plugin_id}: {error}")
            })?;
        if let Some(shortcut) = contributions
            .iter()
            .filter_map(|item| item.shortcut.as_deref())
            .find(|shortcut| declared.contains(shortcut))
        {
            return Err(format!(
                "UI contribution shortcut {shortcut} conflicts with installed plugin {plugin_id}"
            ));
        }
    }
    Ok(())
}

pub fn sync_inventory(conn: &Connection, manifests: &[PluginManifest]) -> Result<(), String> {
    for manifest in manifests {
        manifest.validate()?;
        validate_shortcut_collisions(conn, manifest)?;
        let ui_contributions = serde_json::to_string(&manifest.effective_ui_contributions())
            .map_err(|error| format!("failed to serialize plugin UI contributions: {error}"))?;
        let capabilities = serde_json::to_string(&manifest.capabilities)
            .map_err(|error| format!("failed to serialize plugin capabilities: {error}"))?;
        conn.execute(
            "INSERT INTO plugin_inventory (
                id, name, version, host_api_version, source_kind, backend_entrypoint, ui_contributions, capabilities
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                version = excluded.version,
                host_api_version = excluded.host_api_version,
                source_kind = excluded.source_kind,
                backend_entrypoint = excluded.backend_entrypoint,
                ui_contributions = excluded.ui_contributions,
                capabilities = excluded.capabilities,
                updated_at = CURRENT_TIMESTAMP",
            params![
                manifest.id,
                manifest.name,
                manifest.version,
                manifest.host_api_version,
                manifest.source_kind.as_str(),
                manifest.backend_entrypoint.as_deref().unwrap_or_default(),
                ui_contributions,
                capabilities,
            ],
        )
        .map_err(|e| format!("failed to persist plugin inventory: {e}"))?;
    }
    Ok(())
}

pub fn reconcile_interrupted_runs(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE plugin_inventory
         SET runtime_state = 'error',
             last_error = 'PlaneAI stopped while this plugin runtime was active',
             updated_at = CURRENT_TIMESTAMP
         WHERE runtime_state IN ('starting', 'running', 'stopping')",
        [],
    )
}

pub fn insert_local_inventory(
    conn: &Connection,
    manifest: &PluginManifest,
    backend_entrypoint: &str,
    content_hash: &str,
    package_dir: &Path,
    original_display_path: &str,
) -> Result<PluginInventory, String> {
    if get_inventory(conn, &manifest.id)
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err(format!("plugin id is already installed: {}", manifest.id));
    }
    manifest.validate()?;
    validate_shortcut_collisions(conn, manifest)?;
    let ui_contributions = serde_json::to_string(&manifest.effective_ui_contributions())
        .map_err(|error| format!("failed to serialize plugin UI contributions: {error}"))?;
    let capabilities = serde_json::to_string(&manifest.capabilities)
        .map_err(|error| format!("failed to serialize plugin capabilities: {error}"))?;
    conn.execute(
        "INSERT INTO plugin_inventory (
            id, name, version, host_api_version, source_kind, backend_entrypoint, ui_contributions, capabilities,
            installed_hash, installed_path, original_display_path, enabled, runtime_state
        ) VALUES (?1, ?2, ?3, ?4, 'local', ?5, ?6, ?7, ?8, ?9, ?10, 0, 'disabled')",
        params![
            manifest.id,
            manifest.name,
            manifest.version,
            manifest.host_api_version,
            backend_entrypoint,
            ui_contributions,
            capabilities,
            content_hash,
            package_dir.display().to_string(),
            original_display_path,
        ],
    )
    .map_err(|error| format!("failed to persist imported local plugin: {error}"))?;
    get_inventory(conn, &manifest.id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "new local plugin inventory record was not found".to_string())
}

pub fn list_inventory(conn: &Connection) -> rusqlite::Result<Vec<PluginInventory>> {
    let mut statement = conn.prepare(
        "SELECT id, name, version, host_api_version, source_kind, backend_entrypoint,
                ui_contributions, capabilities, installed_hash, installed_path, original_display_path, enabled, runtime_state, last_error, log_path
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
                ui_contributions, capabilities, installed_hash, installed_path, original_display_path, enabled, runtime_state, last_error, log_path
         FROM plugin_inventory WHERE id = ?1",
        [plugin_id],
        row_to_inventory,
    )
    .optional()
}

pub fn delete_local_inventory(conn: &Connection, plugin_id: &str) -> Result<(), String> {
    let changed = conn
        .execute(
            "DELETE FROM plugin_inventory WHERE id = ?1 AND source_kind = 'local'",
            [plugin_id],
        )
        .map_err(|error| format!("failed to remove plugin inventory: {error}"))?;
    if changed != 1 {
        return Err(format!(
            "local plugin inventory entry not found: {plugin_id}"
        ));
    }
    Ok(())
}

fn row_to_inventory(row: &rusqlite::Row<'_>) -> rusqlite::Result<PluginInventory> {
    let id: String = row.get(0)?;
    let source_kind: String = row.get(4)?;
    let state: String = row.get(12)?;
    let ui_contributions = serde_json::from_str::<Vec<PluginUiContribution>>(
        &row.get::<_, String>(6)?,
    )
    .map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(error))
    })?;
    validate_ui_contributions(&id, &ui_contributions).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            7,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        )
    })?;
    let capabilities = serde_json::from_str::<Vec<PluginHostCapability>>(&row.get::<_, String>(7)?)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                7,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    validate_capabilities(PluginSourceKind::from_db(&source_kind), &capabilities).map_err(
        |error| {
            rusqlite::Error::FromSqlConversionFailure(
                7,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
            )
        },
    )?;
    Ok(PluginInventory {
        id,
        name: row.get(1)?,
        version: row.get(2)?,
        host_api_version: row.get(3)?,
        source_kind: PluginSourceKind::from_db(&source_kind),
        backend_entrypoint: row.get(5)?,
        ui_contributions,
        capabilities,
        installed_hash: row.get(8)?,
        installed_path: row.get(9)?,
        original_display_path: row.get(10)?,
        enabled: row.get(11)?,
        state: PluginRuntimeState::from_db(&state),
        last_error: row.get(13)?,
        log_path: row.get(14)?,
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
    processes: Arc<AsyncMutex<HashMap<String, Arc<RuntimeProcess>>>>,
    lifecycle: Arc<AsyncMutex<()>>,
    shutting_down: AtomicBool,
    exit_permitted: AtomicBool,
}

struct RuntimeProcess {
    child: AsyncMutex<Child>,
    stdin: AsyncMutex<ChildStdin>,
    stdout: AsyncMutex<BufReader<ChildStdout>>,
    request_lock: AsyncMutex<()>,
    next_request_id: AtomicU64,
    plugin_id: String,
    data_dir: PathBuf,
    capabilities: HashSet<PluginHostCapability>,
    lifecycle_event_subscriptions: AsyncMutex<HashSet<String>>,
}

fn is_host_controlled_plugin_method(method: &str) -> bool {
    matches!(
        method,
        "plugin.handshake" | "plugin.shutdown" | "plugin.taskLifecycle" | "$/cancelRequest"
    )
}

fn request_timeout(method: &str) -> Duration {
    match method {
        "jira.syncNow" => JIRA_SYNC_RPC_TIMEOUT,
        "plugin.taskLifecycle" => JIRA_LIFECYCLE_RPC_TIMEOUT,
        _ => RPC_TIMEOUT,
    }
}

impl RuntimeProcess {
    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        self.request_with_timeout(method, params, request_timeout(method))
            .await
    }

    async fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        request_timeout: Duration,
    ) -> Result<Value, String> {
        // JSON-RPC responses share one stdout stream. Serialize normal host
        // requests while stdin remains independently writable for cancellation.
        let _request = self.request_lock.lock().await;
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed) + 1;
        let frame = encode_json_rpc_line(request_id, method, params)?;
        self.write_request_frame(&frame).await?;

        match timeout(request_timeout, self.read_matching_response()).await {
            Ok(Ok(frame)) => decode_json_rpc_frame(&frame, request_id),
            Ok(Err(error)) => Err(error),
            Err(_) => self.cancel_timed_out_request(method, request_id).await,
        }
    }

    async fn cancel_timed_out_request(
        &self,
        method: &str,
        request_id: u64,
    ) -> Result<Value, String> {
        let frame = encode_json_rpc_cancel_request_line(request_id)?;
        if self.write_cancellation_frame(&frame).await.is_err() {
            return Err(cancellation_acknowledgement_error(method, request_id, None));
        }
        match timeout(SHUTDOWN_TIMEOUT, self.read_matching_response()).await {
            Ok(Ok(frame)) => Err(cancellation_acknowledgement_error(
                method,
                request_id,
                Some(&frame),
            )),
            // The original timeout remains the externally meaningful error when
            // the plugin cannot acknowledge cancellation correctly or in time.
            Ok(Err(_)) | Err(_) => {
                Err(cancellation_acknowledgement_error(method, request_id, None))
            }
        }
    }

    async fn write_request_frame(&self, frame: &str) -> Result<(), String> {
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| format!("failed to write plugin JSON-RPC request: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("failed to flush plugin JSON-RPC request: {e}"))
    }

    async fn write_shutdown_frame(&self, frame: &str, deadline: Instant) -> Result<(), String> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let mut stdin = timeout(remaining, self.stdin.lock())
            .await
            .map_err(|_| "plugin shutdown request timed out".to_string())?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        timeout(remaining, async {
            stdin
                .write_all(frame.as_bytes())
                .await
                .map_err(|error| format!("failed to write plugin shutdown request: {error}"))?;
            stdin
                .flush()
                .await
                .map_err(|error| format!("failed to flush plugin shutdown request: {error}"))
        })
        .await
        .map_err(|_| "plugin shutdown request timed out".to_string())?
    }

    async fn write_cancellation_frame(&self, frame: &str) -> Result<(), String> {
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| format!("failed to write plugin cancellation notification: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("failed to flush plugin cancellation notification: {e}"))
    }

    async fn read_matching_response(&self) -> Result<String, String> {
        loop {
            let mut bytes = Vec::new();
            let bytes_read = {
                let mut stdout = self.stdout.lock().await;
                (&mut *stdout)
                    .take(MAX_RPC_FRAME_BYTES + 1)
                    .read_until(b'\n', &mut bytes)
                    .await
                    .map_err(|e| format!("failed to read plugin JSON-RPC response: {e}"))?
            };
            if bytes_read == 0 {
                return Err("plugin process closed stdout unexpectedly".to_string());
            }
            validate_plugin_response_frame(&bytes)?;
            let frame = String::from_utf8(bytes)
                .map_err(|e| format!("plugin JSON-RPC response was not valid UTF-8: {e}"))?;
            let value: Value = serde_json::from_str(frame.trim_end())
                .map_err(|e| format!("malformed plugin JSON-RPC frame: {e}"))?;
            if value.get("method").is_some() {
                self.handle_plugin_request(parse_json_rpc_callback_request(value)?)
                    .await?;
                continue;
            }
            return Ok(frame);
        }
    }

    async fn handle_plugin_request(&self, request: JsonRpcCallbackRequest) -> Result<(), String> {
        let JsonRpcCallbackRequest { id, method, params } = request;
        let result = execute_host_task(
            &self.plugin_id,
            &self.capabilities,
            &self.data_dir,
            &method,
            params,
        )
        .await;
        let response = match result {
            Ok(result) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(message) => {
                serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": message } })
            }
        };
        let mut frame = serde_json::to_string(&response)
            .map_err(|error| format!("failed to encode host callback response: {error}"))?;
        if !host_callback_response_fits(&frame) {
            frame = serde_json::to_string(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32000,
                    "message": "host callback response exceeded the frame limit",
                },
            }))
            .map_err(|error| format!("failed to encode bounded host callback error: {error}"))?;
        }
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(frame.as_bytes())
            .await
            .map_err(|error| format!("failed to write host callback response: {error}"))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|error| format!("failed to frame host callback response: {error}"))?;
        stdin
            .flush()
            .await
            .map_err(|error| format!("failed to flush host callback response: {error}"))
    }

    async fn exited(&self) -> Result<Option<String>, String> {
        self.child
            .lock()
            .await
            .try_wait()
            .map_err(|e| format!("failed to check plugin process: {e}"))
            .map(|status| {
                status.map(|value| format!("plugin process exited unexpectedly ({value})"))
            })
    }

    async fn request_shutdown(&self, deadline: Instant) -> Result<Value, String> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let _request = timeout(remaining, self.request_lock.lock())
            .await
            .map_err(|_| "plugin shutdown request timed out".to_string())?;
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed) + 1;
        let frame = encode_json_rpc_line(request_id, "plugin.shutdown", Value::Null)?;
        self.write_shutdown_frame(&frame, deadline).await?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let frame = timeout(remaining, self.read_matching_response())
            .await
            .map_err(|_| "plugin shutdown request timed out".to_string())??;
        decode_json_rpc_frame(&frame, request_id)
    }

    async fn stop(&self) -> Result<(), String> {
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        let _ = self.request_shutdown(deadline).await;
        let mut child = self.child.lock().await;
        let remaining = deadline.saturating_duration_since(Instant::now());
        match timeout(remaining, child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(format!("failed waiting for plugin shutdown: {e}")),
            Err(_) => {
                child
                    .start_kill()
                    .map_err(|e| format!("plugin did not stop and could not be killed: {e}"))?;
                match timeout(SHUTDOWN_TIMEOUT, child.wait()).await {
                    Ok(Ok(_)) => Ok(()),
                    Ok(Err(e)) => Err(format!("failed waiting for killed plugin: {e}")),
                    Err(_) => Err("plugin did not exit after being killed".to_string()),
                }
            }
        }
    }
}

fn read_plugin_settings(data_dir: &Path) -> Result<Value, String> {
    let path = data_dir.join("settings.json");
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let settings: Value =
        serde_json::from_reader(std::fs::File::open(&path).map_err(|error| {
            format!("failed to read plugin settings {}: {error}", path.display())
        })?)
        .map_err(|error| format!("failed to parse plugin settings: {error}"))?;
    if !settings.is_object() {
        return Err("plugin settings must be a JSON object".to_string());
    }
    Ok(settings)
}

fn replace_plugin_settings(data_dir: &Path, settings: Value) -> Result<Value, String> {
    if !settings.is_object() {
        return Err("plugin settings must be a JSON object".to_string());
    }
    std::fs::create_dir_all(data_dir)
        .map_err(|error| format!("failed to create plugin settings directory: {error}"))?;
    let temporary = data_dir.join(format!(".settings-{}.tmp", uuid::Uuid::new_v4()));
    let path = data_dir.join("settings.json");
    let serialized = serde_json::to_vec_pretty(&settings)
        .map_err(|error| format!("failed to serialize plugin settings: {error}"))?;
    if let Err(error) = std::fs::write(&temporary, serialized) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("failed to write plugin settings: {error}"));
    }
    if let Err(error) = planeai_paths::replace_file_atomically(&temporary, &path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("failed to save plugin settings: {error}"));
    }
    Ok(settings)
}

async fn execute_host_task(
    plugin_id: &str,
    capabilities: &HashSet<PluginHostCapability>,
    data_dir: &Path,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let required = match method {
        "host.settings.get" | "host.settings.replace" => PluginHostCapability::Settings,
        "host.tasks.read" | "host.task.get" => PluginHostCapability::TasksRead,
        "host.task.create" => PluginHostCapability::TasksCreate,
        "host.task.update" => PluginHostCapability::TasksUpdate,
        _ => return Err("host method not found".to_string()),
    };
    if !capabilities.contains(&required)
        || (matches!(method, "host.task.create" | "host.task.update")
            && plugin_id != JIRA_PLUGIN_ID)
    {
        return Err("plugin capability is not granted".to_string());
    }
    if matches!(method, "host.settings.get" | "host.settings.replace") {
        let data_dir = data_dir.to_path_buf();
        let method = method.to_string();
        return commands::blocking(move || match method.as_str() {
            "host.settings.get" => read_plugin_settings(&data_dir)
                .map(|settings| serde_json::json!({ "settings": settings })),
            "host.settings.replace" => {
                let settings = params.get("settings").cloned().unwrap_or(params);
                replace_plugin_settings(&data_dir, settings)
                    .map(|settings| serde_json::json!({ "settings": settings }))
            }
            _ => unreachable!(),
        })
        .await;
    }
    if matches!(method, "host.tasks.read" | "host.task.get") {
        let key = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or("task read requires key")?
            .to_string();
        return commands::blocking(move || {
            let path = planeai_paths::db_path();
            let repo = SqliteRepository::open_read_only(
                path.to_str().ok_or("invalid PlaneAI task database path")?,
                "PLUGIN",
            )
            .map_err(|error| error.to_string())?;
            let task = match repo.get(&key) {
                Ok(task) => Some(task),
                Err(planeai_tasks::provider::Error::NotFound) => None,
                Err(error) => return Err(error.to_string()),
            };
            Ok(serde_json::json!({ "task": task }))
        })
        .await;
    }
    let data_dir = data_dir.to_path_buf();
    let method = method.to_string();
    commands::blocking(move || {
        let settings: Value = serde_json::from_reader(
            std::fs::File::open(data_dir.join("settings.json")).map_err(|error| {
                format!("failed to read plugin settings for task capability: {error}")
            })?,
        )
        .map_err(|error| format!("failed to parse plugin settings for task capability: {error}"))?;
        let site = settings
            .get("site")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let prefix = site
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('.')
            .next()
            .filter(|value| !value.is_empty())
            .map(planeai_tasks::sqlite::derive_prefix)
            .unwrap_or_else(|| "JIRA".to_string());
        let path = planeai_paths::db_path();
        let repo = SqliteRepository::open(
            path.to_str().ok_or("invalid PlaneAI task database path")?,
            &prefix,
        )
        .map_err(|error| error.to_string())?;
        match method.as_str() {
            "host.task.create" => {
                let status = params
                    .get("status")
                    .and_then(Value::as_str)
                    .map(|value| Status::parse(value).ok_or("invalid task status"))
                    .transpose()?;
                let task = repo
                    .create(CreateParams {
                        key: params
                            .get("key")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        title: params
                            .get("title")
                            .and_then(Value::as_str)
                            .ok_or("task create requires title")?
                            .to_string(),
                        description: params
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        status,
                        priority: params.get("priority").and_then(Value::as_i64).unwrap_or(0)
                            as i32,
                        tags: params
                            .get("tags")
                            .cloned()
                            .map(serde_json::from_value)
                            .transpose()
                            .map_err(|error| format!("invalid task tags: {error}"))?
                            .unwrap_or_default(),
                        ..Default::default()
                    })
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(task).map_err(|error| error.to_string())
            }
            "host.task.update" => {
                let key = params
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or("task update requires key")?;
                let status = params
                    .get("status")
                    .and_then(Value::as_str)
                    .map(|value| Status::parse(value).ok_or("invalid task status"))
                    .transpose()?;
                let task = repo
                    .update(
                        key,
                        UpdateParams {
                            title: params
                                .get("title")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                            description: params
                                .get("description")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                            status,
                            priority: params
                                .get("priority")
                                .and_then(Value::as_i64)
                                .map(|value| value as i32),
                            tags: params
                                .get("tags")
                                .cloned()
                                .map(serde_json::from_value)
                                .transpose()
                                .map_err(|error| format!("invalid task tags: {error}"))?,
                            ..Default::default()
                        },
                    )
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(task).map_err(|error| error.to_string())
            }
            _ => Err("host method not found".to_string()),
        }
    })
    .await
}

fn lifecycle_subscription_is_granted(
    capabilities: &HashSet<PluginHostCapability>,
    subscriptions: &HashSet<String>,
) -> bool {
    capabilities.contains(&PluginHostCapability::TaskEvents)
        && subscriptions.contains("task.lifecycle")
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

    /// Read public structured settings for one installed plugin. Secrets remain
    /// outside this API in the backend-only plugin secrets directory.
    pub async fn settings(&self, plugin_id: &str) -> Result<Value, String> {
        self.inventory(plugin_id)
            .await?
            .ok_or_else(|| format!("plugin inventory entry not found: {plugin_id}"))?;
        let root = plugin_state_root(&self.app, plugin_id).await?;
        commands::blocking(move || read_plugin_settings(&root.join("data"))).await
    }

    /// Replace public structured settings atomically. Plugin secrets are never
    /// accepted here and therefore cannot be returned over UI RPC.
    pub async fn update_settings(&self, plugin_id: &str, settings: Value) -> Result<Value, String> {
        if !settings.is_object() {
            return Err("plugin settings must be a JSON object".to_string());
        }
        // Jira settings govern cache ownership and connected-site invariants, so only
        // its sidecar may persist them. Generic host persistence would bypass both.
        if plugin_id == JIRA_PLUGIN_ID {
            return self.call(plugin_id, "jira.settings.update", settings).await;
        }
        self.inventory(plugin_id)
            .await?
            .ok_or_else(|| format!("plugin inventory entry not found: {plugin_id}"))?;
        let root = plugin_state_root(&self.app, plugin_id).await?;
        commands::blocking(move || replace_plugin_settings(&root.join("data"), settings)).await
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

    pub async fn install_local(&self, source_path: String) -> Result<PluginInventory, String> {
        let _lifecycle = self.lifecycle.lock().await;
        let app = self.app.clone();
        let imported = commands::blocking(move || {
            use tauri::Manager as _;
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("failed to resolve PlaneAI plugin storage: {error}"))?;
            crate::plugin_packages::import_local_package(&app_data, Path::new(&source_path))
        })
        .await?;
        let manifest = imported.manifest.clone();
        let backend = imported.backend_entrypoint.clone();
        let hash = imported.content_hash.clone();
        let package_dir = imported.package_dir.clone();
        let original_path = imported.original_display_path.clone();
        let inventory = self
            .with_db(move |conn| {
                insert_local_inventory(
                    conn,
                    &manifest,
                    &backend,
                    &hash,
                    &package_dir,
                    &original_path,
                )
            })
            .await?;
        self.emit_change(&inventory.id).await;
        Ok(inventory)
    }

    pub async fn remove(&self, plugin_id: &str) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().await;
        let inventory = self
            .inventory(plugin_id)
            .await?
            .ok_or_else(|| format!("plugin inventory entry not found: {plugin_id}"))?;
        if inventory.source_kind != PluginSourceKind::Local {
            return Err(format!("builtin plugin {plugin_id} cannot be removed"));
        }
        if inventory.state != PluginRuntimeState::Disabled {
            self.disable_inner(plugin_id).await?;
        }
        let id = inventory.id.clone();
        let app = self.app.clone();
        let package_path = inventory.installed_path.clone().map(PathBuf::from);
        let cleanup = commands::blocking(move || {
            use tauri::Manager as _;
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("failed to resolve PlaneAI plugin state: {error}"))?;
            crate::plugin_packages::remove_local_artifacts(&app_data, &id, package_path.as_deref())
        })
        .await;
        if let Err(error) = cleanup {
            self.update_state(
                plugin_id,
                false,
                PluginRuntimeState::Error,
                Some(error.clone()),
            )
            .await?;
            return Err(error);
        }
        let remove_id = plugin_id.to_string();
        self.with_db(move |conn| delete_local_inventory(conn, &remove_id))
            .await
    }

    pub async fn call(
        &self,
        plugin_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        self.call_internal(plugin_id, method, params, false).await
    }

    async fn call_internal(
        &self,
        plugin_id: &str,
        method: &str,
        params: Value,
        host_controlled: bool,
    ) -> Result<Value, String> {
        if !host_controlled && is_host_controlled_plugin_method(method) {
            return Err("plugin lifecycle methods are reserved for PlaneAI".to_string());
        }
        let process = {
            let _lifecycle = self.lifecycle.lock().await;
            let inventory = self
                .inventory(plugin_id)
                .await?
                .ok_or_else(|| format!("plugin inventory entry not found: {plugin_id}"))?;
            if inventory.state != PluginRuntimeState::Running {
                return Err(format!("plugin {plugin_id} is not running"));
            }
            self.processes.lock().await.get(plugin_id).cloned()
        };
        let process =
            process.ok_or_else(|| format!("plugin runtime was not available: {plugin_id}"))?;
        let result = process.request(method, params).await;
        let Err(error) = result else {
            return result;
        };
        if !is_rpc_timeout_error(&error) {
            return Err(error);
        }

        let _lifecycle = self.lifecycle.lock().await;
        let owns_process = {
            let mut active_processes = self.processes.lock().await;
            remove_current_process(&mut active_processes, plugin_id, &process)
        };
        if owns_process {
            if let Err(stop_error) = stop_process(process).await {
                tracing::warn!(plugin_id, %stop_error, "failed to stop timed-out plugin runtime");
            }
            if let Err(state_error) = self
                .update_state(
                    plugin_id,
                    true,
                    PluginRuntimeState::Error,
                    Some(error.clone()),
                )
                .await
            {
                tracing::warn!(plugin_id, %state_error, "failed to record timed-out plugin runtime");
            }
        }
        Err(error)
    }

    /// Deliver a committed lifecycle batch in the background. Delivery failures
    /// are deliberately isolated from the local task transaction.
    pub fn dispatch_task_lifecycle(self: &Arc<Self>, batch: TaskLifecycleBatch) {
        if self.shutting_down.load(Ordering::Acquire) {
            tracing::warn!(batch_id = %batch.batch_id, "skipped lifecycle delivery while plugin runtime is shutting down");
            return;
        }
        let supervisor = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let processes = supervisor
                .processes
                .lock()
                .await
                .iter()
                .map(|(plugin_id, process)| (plugin_id.clone(), process.clone()))
                .collect::<Vec<_>>();
            for (plugin_id, process) in processes {
                let subscriptions = process.lifecycle_event_subscriptions.lock().await;
                let subscribed =
                    lifecycle_subscription_is_granted(&process.capabilities, &subscriptions);
                drop(subscriptions);
                if !subscribed {
                    continue;
                }
                if let Err(error) = supervisor
                    .call_internal(
                        &plugin_id,
                        "plugin.taskLifecycle",
                        serde_json::json!({ "batch": batch }),
                        true,
                    )
                    .await
                {
                    tracing::warn!(plugin_id, batch_id = %batch.batch_id, %error, "task lifecycle delivery failed");
                }
            }
        });
    }

    pub async fn local_ui_source(
        &self,
        plugin_id: &str,
        contribution_id: &str,
    ) -> Result<String, String> {
        let inventory = self
            .inventory(plugin_id)
            .await?
            .ok_or_else(|| format!("plugin inventory entry not found: {plugin_id}"))?;
        if inventory.source_kind != PluginSourceKind::Local {
            return Err("builtin plugins use their bundled UI entrypoints".to_string());
        }
        let package = inventory
            .installed_path
            .ok_or_else(|| format!("local plugin {plugin_id} has no imported package path"))?;
        let entrypoint = inventory
            .ui_contributions
            .iter()
            .find(|contribution| contribution.id == contribution_id)
            .map(|contribution| contribution.entrypoint.clone())
            .ok_or_else(|| {
                format!("plugin {plugin_id} has no UI contribution {contribution_id}")
            })?;
        commands::blocking(move || {
            std::fs::read_to_string(Path::new(&package).join(entrypoint))
                .map_err(|error| format!("failed to read imported plugin UI bundle: {error}"))
        })
        .await
    }

    pub async fn start_enabled(&self) {
        let _lifecycle = self.lifecycle.lock().await;
        let enabled = match self.list().await {
            Ok(inventory) => inventory
                .into_iter()
                .filter(|plugin| plugin.enabled)
                .collect::<Vec<_>>(),
            Err(error) => {
                tracing::warn!(%error, "failed to list enabled plugins at startup");
                return;
            }
        };
        for plugin in enabled {
            if let Err(error) = self.enable_inner(&plugin.id).await {
                tracing::warn!(plugin_id = %plugin.id, %error, "failed to restore enabled plugin at startup");
            }
        }
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
            if let Err(error) = self.stop_for_shutdown_inner(&plugin_id).await {
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
            if let Err(error) = self.stop_for_shutdown_inner(&plugin_id).await {
                tracing::warn!(plugin_id, %error, "failed to stop plugin runtime during shutdown");
            }
        }
    }

    async fn stop_for_shutdown_inner(&self, plugin_id: &str) -> Result<(), String> {
        let inventory = self
            .inventory(plugin_id)
            .await?
            .ok_or_else(|| format!("plugin inventory entry not found: {plugin_id}"))?;
        self.update_state(
            plugin_id,
            inventory.enabled,
            PluginRuntimeState::Stopping,
            inventory.last_error.clone(),
        )
        .await?;
        if let Some(process) = self.processes.lock().await.remove(plugin_id) {
            if let Err(error) = stop_process(process).await {
                self.update_state(
                    plugin_id,
                    inventory.enabled,
                    PluginRuntimeState::Error,
                    Some(error.clone()),
                )
                .await?;
                return Err(error);
            }
        }
        self.update_state(
            plugin_id,
            inventory.enabled,
            PluginRuntimeState::Disabled,
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn enable(&self, plugin_id: &str) -> Result<PluginInventory, String> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("plugin runtime is shutting down".to_string());
        }
        self.enable_inner(plugin_id).await
    }

    async fn enable_inner(&self, plugin_id: &str) -> Result<PluginInventory, String> {
        let inventory = self
            .inventory(plugin_id)
            .await?
            .ok_or_else(|| format!("plugin inventory entry not found: {plugin_id}"))?;
        let app = self.app.clone();
        let id = inventory.id.clone();
        let log_path = plugin_log_path(&app, &id).await?;

        let starting_id = id.clone();
        let starting_log = log_path.display().to_string();
        self.with_db(move |conn| mark_starting(conn, &starting_id, &starting_log))
            .await?;
        self.emit_change(&id).await;

        let capabilities = inventory
            .capabilities
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        let binary_inventory = inventory.clone();
        let binary = match commands::blocking(move || {
            resolve_plugin_binary(&app, &binary_inventory)
        })
        .await
        {
            Ok(binary) => binary,
            Err(error) => {
                self.update_state(&id, true, PluginRuntimeState::Error, Some(error.clone()))
                    .await?;
                return Err(error);
            }
        };

        // Bundled plugins own durable state too; Jira settings and credentials
        // must survive restarts under the same plugin namespace as local plugins.
        let state_path = Some(plugin_state_root(&self.app, &id).await?);
        let process = match spawn_runtime(
            &binary,
            &log_path,
            state_path.as_deref(),
            &id,
            capabilities.clone(),
        )
        .await
        {
            Ok(process) => process,
            Err(error) => {
                self.update_state(&id, true, PluginRuntimeState::Error, Some(error.clone()))
                    .await?;
                return Err(error);
            }
        };
        let process = Arc::new(process);

        let handshake_result = process
            .request(
                "plugin.handshake",
                serde_json::json!({
                    "host_api_version": HOST_API_VERSION,
                    "host_capabilities": capabilities,
                }),
            )
            .await;
        let handshake: PluginHandshake = match handshake_result.and_then(|value| {
            serde_json::from_value::<PluginHandshake>(value)
                .map_err(|e| format!("invalid plugin handshake: {e}"))
        }) {
            Ok(value)
                if value.plugin_id == inventory.id
                    && value.plugin_name == inventory.name
                    && value.plugin_version == inventory.version
                    && value.host_api_version == HOST_API_VERSION =>
            {
                value
            }
            Ok(_) => {
                let error = "plugin handshake identity or host API version did not match manifest"
                    .to_string();
                let _ = stop_process(process).await;
                self.update_state(&id, true, PluginRuntimeState::Error, Some(error.clone()))
                    .await?;
                return Err(error);
            }
            Err(error) => {
                let _ = stop_process(process).await;
                self.update_state(&id, true, PluginRuntimeState::Error, Some(error.clone()))
                    .await?;
                return Err(error);
            }
        };
        {
            let mut subscriptions = process.lifecycle_event_subscriptions.lock().await;
            *subscriptions = handshake.lifecycle_event_subscriptions;
            if !process
                .capabilities
                .contains(&PluginHostCapability::TaskEvents)
            {
                subscriptions.clear();
            }
        }
        tracing::info!(plugin_id = %handshake.plugin_id, version = %handshake.plugin_version, "plugin runtime handshake completed");

        // Publish the ready runtime before emitting the running lifecycle event so
        // mounted UI contributions cannot observe `running` without a callable handle.
        self.processes
            .lock()
            .await
            .insert(id.clone(), process.clone());
        if let Err(error) = self
            .update_state(&id, true, PluginRuntimeState::Running, None)
            .await
        {
            self.processes.lock().await.remove(&id);
            if let Err(stop_error) = stop_process(process).await {
                tracing::warn!(plugin_id = %id, %stop_error, "failed to stop plugin after startup persistence failure");
            }
            if let Err(recovery_error) = self
                .update_state(&id, true, PluginRuntimeState::Error, Some(error.clone()))
                .await
            {
                tracing::warn!(plugin_id = %id, %recovery_error, "failed to record plugin startup persistence failure");
            }
            return Err(error);
        }
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

    fn monitor_process(&self, plugin_id: String, process: Arc<RuntimeProcess>) {
        let db = self.db.clone();
        let app = self.app.clone();
        let processes = self.processes.clone();
        let lifecycle = self.lifecycle.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(PROCESS_MONITOR_INTERVAL).await;
                let outcome = process.exited().await;
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
}

fn host_callback_response_fits(frame: &str) -> bool {
    frame.len() < MAX_RPC_FRAME_BYTES as usize
}

fn validate_plugin_response_frame(frame: &[u8]) -> Result<(), String> {
    if frame.len() > MAX_RPC_FRAME_BYTES as usize {
        return Err("plugin JSON-RPC response exceeded the frame limit".to_string());
    }
    if !frame.ends_with(b"\n") {
        return Err("plugin JSON-RPC response was not newline terminated".to_string());
    }
    Ok(())
}

fn parse_json_rpc_callback_request(value: Value) -> Result<JsonRpcCallbackRequest, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "malformed plugin JSON-RPC callback: expected an object".to_string())?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err("malformed plugin JSON-RPC callback: expected jsonrpc 2.0".to_string());
    }
    if object.contains_key("result") || object.contains_key("error") {
        return Err(
            "malformed plugin JSON-RPC callback: request cannot contain result or error"
                .to_string(),
        );
    }
    let id = object
        .get("id")
        .filter(|id| matches!(id, Value::String(_) | Value::Number(_)))
        .cloned()
        .ok_or_else(|| {
            "malformed plugin JSON-RPC callback: id must be a string or number".to_string()
        })?;
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .filter(|method| !method.is_empty())
        .ok_or_else(|| {
            "malformed plugin JSON-RPC callback: method must be a nonempty string".to_string()
        })?
        .to_owned();
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    if !params.is_null() && !params.is_array() && !params.is_object() {
        return Err(
            "malformed plugin JSON-RPC callback: params must be null, an array, or an object"
                .to_string(),
        );
    }
    Ok(JsonRpcCallbackRequest { id, method, params })
}

fn decode_json_rpc_frame(frame: &str, request_id: u64) -> Result<Value, String> {
    let line = frame
        .strip_suffix('\n')
        .ok_or_else(|| "plugin JSON-RPC response was not newline terminated".to_string())?;
    decode_json_rpc_response(line, request_id)
}

fn resolve_plugin_binary(app: &AppHandle, inventory: &PluginInventory) -> Result<PathBuf, String> {
    match inventory.source_kind {
        PluginSourceKind::Builtin => resolve_trusted_binary(app, &inventory.backend_entrypoint),
        PluginSourceKind::Local => {
            let package = inventory.installed_path.as_deref().ok_or_else(|| {
                format!("local plugin {} has no imported package path", inventory.id)
            })?;
            let binary = Path::new(package).join(&inventory.backend_entrypoint);
            if binary.is_file() {
                Ok(binary)
            } else {
                Err(format!(
                    "imported backend for plugin {} is missing: {}",
                    inventory.id,
                    binary.display()
                ))
            }
        }
    }
}

async fn plugin_state_root(app: &AppHandle, plugin_id: &str) -> Result<PathBuf, String> {
    use tauri::Manager as _;
    let root = crate::plugin_packages::state_root(
        &app.path()
            .app_data_dir()
            .map_err(|error| format!("failed to resolve plugin state directory: {error}"))?,
        plugin_id,
    );
    for directory in [root.join("data"), root.join("secrets"), root.join("logs")] {
        tokio::fs::create_dir_all(directory)
            .await
            .map_err(|error| format!("failed to create plugin state directory: {error}"))?;
    }
    Ok(root)
}

async fn plugin_log_path(app: &AppHandle, plugin_id: &str) -> Result<PathBuf, String> {
    Ok(plugin_state_root(app, plugin_id)
        .await?
        .join("logs")
        .join("stderr.log"))
}

async fn spawn_runtime(
    binary: &Path,
    log_path: &Path,
    state_root: Option<&Path>,
    plugin_id: &str,
    capabilities: HashSet<PluginHostCapability>,
) -> Result<RuntimeProcess, String> {
    let mut command = Command::new(binary);
    if let Some(state_root) = state_root {
        command
            .env("PLANEAI_PLUGIN_DATA_DIR", state_root.join("data"))
            .env("PLANEAI_PLUGIN_SECRETS_DIR", state_root.join("secrets"));
    }
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
    let data_dir = state_root
        .map(|root| root.join("data"))
        .ok_or("plugin runtime state root was not provided")?;
    Ok(RuntimeProcess {
        child: AsyncMutex::new(child),
        stdin: AsyncMutex::new(stdin),
        stdout: AsyncMutex::new(BufReader::new(stdout)),
        request_lock: AsyncMutex::new(()),
        next_request_id: AtomicU64::new(0),
        plugin_id: plugin_id.to_string(),
        data_dir,
        capabilities,
        lifecycle_event_subscriptions: AsyncMutex::new(HashSet::new()),
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

async fn stop_process(process: Arc<RuntimeProcess>) -> Result<(), String> {
    process.stop().await
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
        let enabled = get_inventory(&conn, &update_id)
            .map_err(|e| e.to_string())?
            .map(|inventory| inventory.enabled)
            .ok_or_else(|| format!("plugin inventory entry not found: {update_id}"))?;
        set_state(
            &conn,
            &update_id,
            enabled,
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
    fn legacy_inventory_schema_migrates_a_local_ui_entrypoint_to_a_main_pane_contribution() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE plugin_inventory (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                version TEXT NOT NULL,
                host_api_version TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                backend_entrypoint TEXT NOT NULL,
                ui_entrypoint TEXT,
                installed_hash TEXT,
                installed_path TEXT,
                original_display_path TEXT,
                enabled INTEGER NOT NULL DEFAULT 0,
                runtime_state TEXT NOT NULL DEFAULT 'disabled',
                last_error TEXT,
                log_path TEXT,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            INSERT INTO plugin_inventory (
                id, name, version, host_api_version, source_kind, backend_entrypoint, ui_entrypoint
            ) VALUES ('legacy', 'Legacy', '1.0.0', 'planeai.plugin-host.v1', 'local', 'bin/plugin', 'ui/entry.js');",
        )
        .unwrap();

        migrate(&conn).unwrap();
        let ui_contributions: String = conn
            .query_row(
                "SELECT ui_contributions FROM plugin_inventory WHERE id = 'legacy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let contributions: Vec<PluginUiContribution> =
            serde_json::from_str(&ui_contributions).unwrap();
        assert_eq!(contributions[0].id, "legacy-main-pane");
        assert_eq!(contributions[0].entrypoint, "ui/entry.js");
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
    fn local_ui_contributions_round_trip_through_inventory() {
        let conn = database();
        let manifest = PluginManifest {
            schema: "planeai.plugin.v1".into(),
            id: "sidebar-test".into(),
            name: "Sidebar test".into(),
            version: "1.0.0".into(),
            host_api_version: HOST_API_VERSION.into(),
            source_kind: PluginSourceKind::Local,
            backend_entrypoint: None,
            backend_entrypoints: HashMap::from([(
                crate::plugin_packages::current_platform_key().to_string(),
                "bin/plugin".into(),
            )]),
            ui_contributions: vec![PluginUiContribution {
                id: "log".into(),
                label: "Log".into(),
                placement: PluginUiPlacement::SidebarFooter,
                entrypoint: "ui/log.js".into(),
                order: Some(0),
                shortcut: None,
            }],
            capabilities: vec![],
            legacy_ui_entrypoint: LegacyUiEntrypoint::Absent,
        };
        let package = tempfile::TempDir::new().unwrap();
        insert_local_inventory(
            &conn,
            &manifest,
            "bin/plugin",
            "hash",
            package.path(),
            "/source",
        )
        .unwrap();
        let inventory = get_inventory(&conn, "sidebar-test").unwrap().unwrap();
        assert_eq!(inventory.ui_contributions, manifest.ui_contributions);

        let mut invalid_shortcut = manifest.clone();
        invalid_shortcut.ui_contributions[0].shortcut = Some("Mod+L".into());
        assert!(invalid_shortcut
            .validate()
            .unwrap_err()
            .contains("shortcuts"));

        let mut invalid_preferences = manifest;
        invalid_preferences.ui_contributions[0].placement = PluginUiPlacement::Preferences;
        invalid_preferences.ui_contributions[0].order = Some(0);
        assert!(invalid_preferences
            .validate()
            .unwrap_err()
            .contains("order"));
        invalid_preferences.ui_contributions[0].order = None;
        invalid_preferences.ui_contributions[0].shortcut = Some("Mod+L".into());
        assert!(invalid_preferences
            .validate()
            .unwrap_err()
            .contains("shortcuts"));
    }

    #[test]
    fn invalid_persisted_ui_contributions_are_not_silently_discarded() {
        let conn = database();
        conn.execute(
            "UPDATE plugin_inventory SET ui_contributions = 'not-json' WHERE id = 'jira'",
            [],
        )
        .unwrap();
        assert!(get_inventory(&conn, "jira").is_err());
    }

    #[test]
    fn semantically_invalid_persisted_ui_contributions_are_rejected() {
        let conn = database();
        conn.execute(
            "UPDATE plugin_inventory SET ui_contributions = ?1 WHERE id = 'jira'",
            [r#"[{"id":"bad","label":"Bad","placement":"sidebar.footer","entrypoint":"ui/bad.js","shortcut":"Mod+B"}]"#],
        )
        .unwrap();
        assert!(get_inventory(&conn, "jira").is_err());
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
            backend_entrypoint: None,
            backend_entrypoints: HashMap::from([(
                crate::plugin_packages::current_platform_key().to_string(),
                "local-test".into(),
            )]),
            ui_contributions: vec![],
            capabilities: vec![],
            legacy_ui_entrypoint: LegacyUiEntrypoint::Absent,
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
    fn duplicate_local_plugin_ids_are_rejected_without_replacing_inventory() {
        let conn = database();
        let manifest = PluginManifest {
            schema: "planeai.plugin.v1".into(),
            id: "fixture".into(),
            name: "Fixture".into(),
            version: "1.0.0".into(),
            host_api_version: HOST_API_VERSION.into(),
            source_kind: PluginSourceKind::Local,
            backend_entrypoint: None,
            backend_entrypoints: HashMap::from([(
                crate::plugin_packages::current_platform_key().to_string(),
                "bin/plugin".into(),
            )]),
            ui_contributions: vec![],
            capabilities: vec![],
            legacy_ui_entrypoint: LegacyUiEntrypoint::Absent,
        };
        let package = tempfile::TempDir::new().unwrap();
        insert_local_inventory(
            &conn,
            &manifest,
            "bin/plugin",
            "content-hash",
            package.path(),
            "/original/package",
        )
        .unwrap();
        let error = insert_local_inventory(
            &conn,
            &manifest,
            "bin/plugin",
            "different-hash",
            package.path(),
            "/different/package",
        )
        .unwrap_err();
        assert!(error.contains("already installed"));
        let inventory = get_inventory(&conn, "fixture").unwrap().unwrap();
        assert_eq!(inventory.installed_hash.as_deref(), Some("content-hash"));
        assert_eq!(
            inventory.original_display_path.as_deref(),
            Some("/original/package")
        );
        delete_local_inventory(&conn, "fixture").unwrap();
        assert!(get_inventory(&conn, "fixture").unwrap().is_none());
        assert!(delete_local_inventory(&conn, "jira")
            .unwrap_err()
            .contains("local plugin"));
        assert!(get_inventory(&conn, "jira").unwrap().is_some());
    }

    #[test]
    fn startup_reconciles_interrupted_runtime_without_losing_diagnostics() {
        let conn = database();
        set_state(&conn, "jira", true, PluginRuntimeState::Running, None).unwrap();
        assert_eq!(reconcile_interrupted_runs(&conn).unwrap(), 1);
        let jira = get_inventory(&conn, "jira").unwrap().unwrap();
        assert!(jira.enabled);
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
    fn shutdown_uses_the_documented_three_second_grace() {
        assert_eq!(SHUTDOWN_TIMEOUT, Duration::from_secs(3));
    }

    #[test]
    fn jira_sync_and_lifecycle_use_extended_rpc_timeouts() {
        assert_eq!(request_timeout("jira.syncNow"), JIRA_SYNC_RPC_TIMEOUT);
        assert_eq!(
            request_timeout("plugin.taskLifecycle"),
            JIRA_LIFECYCLE_RPC_TIMEOUT
        );
        assert_eq!(request_timeout("jira.status"), RPC_TIMEOUT);
    }

    #[test]
    fn local_manifest_validates_backend_paths_for_every_declared_platform() {
        let manifest = PluginManifest {
            schema: "planeai.plugin.v1".into(),
            id: "fixture".into(),
            name: "Fixture".into(),
            version: "1".into(),
            host_api_version: HOST_API_VERSION.into(),
            source_kind: PluginSourceKind::Local,
            backend_entrypoint: None,
            backend_entrypoints: HashMap::from([
                (
                    crate::plugin_packages::current_platform_key().to_string(),
                    "bin/plugin".into(),
                ),
                ("windows-x64".into(), "../unsafe.exe".into()),
            ]),
            ui_contributions: vec![],
            capabilities: vec![],
            legacy_ui_entrypoint: LegacyUiEntrypoint::Absent,
        };
        assert!(manifest
            .validate()
            .unwrap_err()
            .contains("backend entrypoint"));
    }

    #[test]
    fn v1_manifest_rejects_unknown_fields_and_supplied_local_legacy_ui_entrypoints() {
        let unknown = serde_json::from_str::<PluginManifest>(
            r#"{"schema":"planeai.plugin.v1","id":"fixture","name":"Fixture","version":"1","host_api_version":"planeai.plugin-host.v1","source_kind":"local","backend_entrypoints":{},"unexpected":true}"#,
        );
        assert!(unknown.is_err());

        let legacy: PluginManifest = serde_json::from_str(
            r#"{"schema":"planeai.plugin.v1","id":"fixture","name":"Fixture","version":"1","host_api_version":"planeai.plugin-host.v1","source_kind":"local","backend_entrypoints":{},"ui_entrypoint":"ui/legacy.js"}"#,
        ).unwrap();
        assert!(legacy.validate().unwrap_err().contains("ui_contributions"));

        let explicit_null: PluginManifest = serde_json::from_str(
            r#"{"schema":"planeai.plugin.v1","id":"fixture","name":"Fixture","version":"1","host_api_version":"planeai.plugin-host.v1","source_kind":"local","backend_entrypoints":{},"ui_entrypoint":null}"#,
        ).unwrap();
        assert!(matches!(
            &explicit_null.legacy_ui_entrypoint,
            LegacyUiEntrypoint::Null
        ));
        assert!(explicit_null
            .validate()
            .unwrap_err()
            .contains("ui_contributions"));

        let absent_builtin: PluginManifest = serde_json::from_str(
            r#"{"schema":"planeai.plugin.v1","id":"jira","name":"Jira","version":"1","host_api_version":"planeai.plugin-host.v1","source_kind":"builtin","backend_entrypoint":"planeai-plugin-jira"}"#,
        ).unwrap();
        assert!(matches!(
            &absent_builtin.legacy_ui_entrypoint,
            LegacyUiEntrypoint::Absent
        ));
        absent_builtin.validate().unwrap();
    }

    #[test]
    fn local_capabilities_are_persisted_and_task_events_gate_delivery() {
        let conn = database();
        let mut manifest = PluginManifest {
            schema: "planeai.plugin.v1".into(),
            id: "capability-test".into(),
            name: "Capability test".into(),
            version: "1.0.0".into(),
            host_api_version: HOST_API_VERSION.into(),
            source_kind: PluginSourceKind::Local,
            backend_entrypoint: None,
            backend_entrypoints: HashMap::from([(
                crate::plugin_packages::current_platform_key().to_string(),
                "bin/plugin".into(),
            )]),
            ui_contributions: vec![],
            capabilities: vec![
                PluginHostCapability::Settings,
                PluginHostCapability::TasksRead,
                PluginHostCapability::TaskEvents,
            ],
            legacy_ui_entrypoint: LegacyUiEntrypoint::Absent,
        };
        let package = tempfile::TempDir::new().unwrap();
        insert_local_inventory(
            &conn,
            &manifest,
            "bin/plugin",
            "hash",
            package.path(),
            "/source",
        )
        .unwrap();
        assert_eq!(
            get_inventory(&conn, "capability-test")
                .unwrap()
                .unwrap()
                .capabilities,
            manifest.capabilities
        );
        let subscriptions = HashSet::from(["task.lifecycle".to_string()]);
        assert!(lifecycle_subscription_is_granted(
            &HashSet::from([PluginHostCapability::TaskEvents]),
            &subscriptions
        ));
        assert!(!lifecycle_subscription_is_granted(
            &HashSet::new(),
            &subscriptions
        ));
        manifest
            .capabilities
            .push(PluginHostCapability::TasksCreate);
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn plugin_settings_helpers_read_replace_and_validate_object_values() {
        let data = tempfile::TempDir::new().unwrap();
        assert_eq!(
            read_plugin_settings(data.path()).unwrap(),
            serde_json::json!({})
        );
        assert!(
            replace_plugin_settings(data.path(), serde_json::json!(["invalid"]))
                .unwrap_err()
                .contains("JSON object")
        );

        let settings = serde_json::json!({ "mode": "local" });
        assert_eq!(
            replace_plugin_settings(data.path(), settings.clone()).unwrap(),
            settings
        );
        assert_eq!(read_plugin_settings(data.path()).unwrap(), settings);

        let replacement = serde_json::json!({ "mode": "remote" });
        assert_eq!(
            replace_plugin_settings(data.path(), replacement.clone()).unwrap(),
            replacement
        );
        assert_eq!(read_plugin_settings(data.path()).unwrap(), replacement);
        assert!(!std::fs::read_dir(data.path())
            .unwrap()
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".settings-")));

        std::fs::write(data.path().join("settings.json"), "[]").unwrap();
        assert!(read_plugin_settings(data.path())
            .unwrap_err()
            .contains("JSON object"));
    }

    #[tokio::test]
    async fn generic_settings_callbacks_work_for_a_local_plugin() {
        let data = tempfile::TempDir::new().unwrap();
        let capabilities = HashSet::from([PluginHostCapability::Settings]);
        let result = execute_host_task(
            "local-test",
            &capabilities,
            data.path(),
            "host.settings.replace",
            serde_json::json!({"settings":{"mode":"local"}}),
        )
        .await
        .unwrap();
        assert_eq!(result["settings"]["mode"], "local");
        let result = execute_host_task(
            "local-test",
            &capabilities,
            data.path(),
            "host.settings.get",
            Value::Null,
        )
        .await
        .unwrap();
        assert_eq!(result["settings"]["mode"], "local");
        assert!(execute_host_task(
            "local-test",
            &HashSet::new(),
            data.path(),
            "host.settings.get",
            Value::Null
        )
        .await
        .unwrap_err()
        .contains("not granted"));
    }

    #[test]
    fn callback_response_frame_limit_includes_its_newline_terminator() {
        assert!(host_callback_response_fits(
            &"x".repeat(MAX_RPC_FRAME_BYTES as usize - 1)
        ));
        assert!(!host_callback_response_fits(
            &"x".repeat(MAX_RPC_FRAME_BYTES as usize)
        ));
    }

    #[test]
    fn callback_requests_require_json_rpc_version_scalar_ids_and_valid_request_shape() {
        let callback = parse_json_rpc_callback_request(serde_json::json!({
            "jsonrpc": "2.0",
            "id": "settings",
            "method": "host.settings.get",
            "params": null,
        }))
        .unwrap();
        assert_eq!(callback.id, "settings");
        assert_eq!(callback.method, "host.settings.get");
        assert_eq!(callback.params, Value::Null);

        for callback in [
            serde_json::json!({ "id": 1, "method": "host.settings.get" }),
            serde_json::json!({ "jsonrpc": "2.0", "method": "host.settings.get" }),
            serde_json::json!({ "jsonrpc": "2.0", "id": null, "method": "host.settings.get" }),
            serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "", "params": null }),
            serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "host.settings.get", "result": {} }),
            serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "host.settings.get", "params": true }),
        ] {
            assert!(parse_json_rpc_callback_request(callback).is_err());
        }
    }

    #[test]
    fn host_controlled_plugin_methods_are_not_exposed_to_plugin_ui_calls() {
        for method in [
            "plugin.handshake",
            "plugin.shutdown",
            "plugin.taskLifecycle",
            "$/cancelRequest",
        ] {
            assert!(is_host_controlled_plugin_method(method), "{method}");
        }
        assert!(!is_host_controlled_plugin_method("fixture.status"));
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
        assert_eq!(
            decode_json_rpc_response(r#"{"jsonrpc":"2.0","id":7,"result":null}"#, 7).unwrap(),
            Value::Null
        );
        let exact_limit = [vec![b'x'; MAX_RPC_FRAME_BYTES as usize - 1], vec![b'\n']].concat();
        assert!(validate_plugin_response_frame(&exact_limit).is_ok());
        let over_limit = [vec![b'x'; MAX_RPC_FRAME_BYTES as usize], vec![b'\n']].concat();
        assert!(validate_plugin_response_frame(&over_limit).is_err());
        assert!(validate_plugin_response_frame(&vec![b'x'; MAX_RPC_FRAME_BYTES as usize]).is_err());
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
        let cancellation = encode_json_rpc_cancel_request_line(7).unwrap();
        let cancellation: Value = serde_json::from_str(cancellation.trim_end()).unwrap();
        assert_eq!(cancellation["method"], "$/cancelRequest");
        assert_eq!(cancellation["params"]["id"], 7);
        assert!(decode_json_rpc_cancellation_response(
            r#"{"jsonrpc":"2.0","id":7,"error":{"code":-32800,"message":"cancelled"}}"#,
            7,
        )
        .unwrap()
        .contains("-32800"));
        assert!(decode_json_rpc_cancellation_response(
            r#"{"jsonrpc":"2.0","id":7,"error":{"code":-32603,"message":"failed"}}"#,
            7,
        )
        .unwrap_err()
        .contains("expected -32800"));
        for response in [
            r#"{"jsonrpc":"2.0","id":7,"result":{},"error":{"code":-32800,"message":"cancelled"}}"#,
            r#"{"jsonrpc":"2.0","id":7}"#,
        ] {
            assert!(decode_json_rpc_response(response, 7)
                .unwrap_err()
                .contains("exactly one"));
        }

        let timeout_error = timed_out_request_error("jira.status");
        for acknowledgement in [
            Some("not json"),
            Some(r#"{"jsonrpc":"2.0","id":8,"error":{"code":-32800,"message":"cancelled"}}"#),
            Some(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#),
            None,
        ] {
            let error = cancellation_acknowledgement_error("jira.status", 7, acknowledgement);
            assert_eq!(error, timeout_error);
            assert!(is_rpc_timeout_error(&error));
        }
        assert_eq!(
            cancellation_acknowledgement_error(
                "jira.status",
                7,
                Some(r#"{"jsonrpc":"2.0","id":7,"error":{"code":-32800,"message":"cancelled"}}"#),
            ),
            "plugin RPC error -32800: cancelled"
        );
    }
}

#[cfg(test)]
mod placement_tests {
    use super::*;

    #[test]
    fn only_actual_sidebar_placements_are_sidebar_contributions() {
        assert!(PluginUiPlacement::SidebarSection.is_sidebar());
        assert!(PluginUiPlacement::SidebarNavigation.is_sidebar());
        assert!(!PluginUiPlacement::Preferences.is_sidebar());
        assert!(!PluginUiPlacement::MainPane.is_sidebar());
    }
}
