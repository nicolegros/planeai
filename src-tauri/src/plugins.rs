use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;
use std::time::Instant;

use planeai_tasks::model::{CreateParams, Status, UpdateParams};
use planeai_tasks::provider::TaskProvider;
use planeai_tasks::sqlite::SqliteRepository;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, Mutex as AsyncMutex, Notify};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};
use tokio_util::sync::CancellationToken;

use crate::commands;
use crate::plugin_rpc::*;
use crate::task_lifecycle::TaskLifecycleBatch;
use planeai_plugin_contract::provider::is_host_controlled_method as is_host_controlled_plugin_method;
use planeai_plugin_contract::supports_host_api_version;
pub use planeai_plugin_contract::ProviderFeature;
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
const MAX_PLUGIN_SESSION_PROMPT_CHARS: usize = 100_000;
const FRAME_QUEUE_CAPACITY: usize = 256;
const JIRA_PLUGIN_ID: &str = "jira";
const GITHUB_PLUGIN_ID: &str = "github";
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
    #[serde(default)]
    pub background_service: Option<PluginBackgroundService>,
    #[serde(default)]
    pub providers: Vec<PluginProvider>,
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
    #[serde(rename = "projects.read")]
    ProjectsRead,
    #[serde(rename = "sessions.read")]
    SessionsRead,
    #[serde(rename = "sessions.repository-context")]
    SessionsRepositoryContext,
    #[serde(rename = "sessions.prompt")]
    SessionsPrompt,
    #[serde(rename = "session-events")]
    SessionEvents,
    #[serde(rename = "sessions.actions")]
    SessionActions,
    #[serde(rename = "sessions.advisories")]
    SessionAdvisories,
    #[serde(rename = "sessions.complete")]
    SessionsComplete,
    #[serde(rename = "tasks.read")]
    TasksRead,
    #[serde(rename = "task-events")]
    TaskEvents,
    #[serde(rename = "tasks.create")]
    TasksCreate,
    #[serde(rename = "tasks.transition")]
    TasksTransition,
    #[serde(rename = "tasks.update")]
    TasksUpdate,
    #[serde(rename = "storage")]
    Storage,
    #[serde(rename = "sidebar.navigation")]
    SidebarNavigation,
    #[serde(rename = "providers")]
    Providers,
}

/// A session runtime a plugin offers as a provider; its UI replaces the agent terminal.
/// Fields and features from newer hosts are ignored, so such a plugin still loads.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PluginProvider {
    pub id: String,
    pub label: String,
    pub entrypoint: String,
    #[serde(
        default,
        deserialize_with = "planeai_plugin_contract::deserialize_provider_features"
    )]
    pub supports: Vec<ProviderFeature>,
}

impl PluginProvider {
    pub fn supports(&self, feature: ProviderFeature) -> bool {
        self.supports.contains(&feature)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginBackgroundService {
    pub method: String,
    pub interval_setting: String,
    pub default_interval_ms: u64,
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
    #[serde(rename = "session.panel")]
    SessionPanel,
    #[serde(rename = "session.indicator")]
    SessionIndicator,
    #[serde(rename = "titlebar")]
    Titlebar,
    #[serde(rename = "interaction")]
    Interaction,
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
        if !supports_host_api_version(&self.host_api_version) {
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
                    "background_service": self.background_service,
                    "providers": (!self.providers.is_empty()).then_some(&self.providers),
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
        if !self.providers.is_empty() {
            return Err("bundled plugins cannot declare providers".to_string());
        }
        validate_ui_contributions(&self.id, &self.effective_ui_contributions())?;
        validate_background_service(self.background_service.as_ref())?;
        validate_capabilities(self.source_kind, &self.capabilities)
    }
}

fn validate_background_service(service: Option<&PluginBackgroundService>) -> Result<(), String> {
    let Some(service) = service else {
        return Ok(());
    };
    if service.method.trim().is_empty() || service.interval_setting.trim().is_empty() {
        return Err("background service method and interval_setting are required".to_string());
    }
    if service.method.starts_with("plugin.") || service.method.starts_with('$') {
        return Err("background service method is reserved for PlaneAI".to_string());
    }
    if service.default_interval_ms == 0 {
        return Err("background service default_interval_ms must be positive".to_string());
    }
    Ok(())
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
                    | PluginHostCapability::ProjectsRead
                    | PluginHostCapability::SessionsRead
                    | PluginHostCapability::SessionsRepositoryContext
                    | PluginHostCapability::SessionsPrompt
                    | PluginHostCapability::SessionEvents
                    | PluginHostCapability::SessionActions
                    | PluginHostCapability::SessionAdvisories
                    | PluginHostCapability::SessionsComplete
                    | PluginHostCapability::TasksRead
                    | PluginHostCapability::TasksCreate
                    | PluginHostCapability::TasksTransition
                    | PluginHostCapability::TaskEvents
                    | PluginHostCapability::Providers
            )
        })
    {
        return Err(
            "local plugins may only request documented local plugin capabilities".to_string(),
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
        if !matches!(
            contribution.placement,
            PluginUiPlacement::MainPane | PluginUiPlacement::SessionPanel
        ) && contribution.shortcut.is_some()
        {
            return Err(
                "UI contribution shortcuts are only valid for main-pane or session-panel contributions".to_string(),
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
    pub background_service: Option<PluginBackgroundService>,
    pub providers: Vec<PluginProvider>,
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
            background_service TEXT,
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
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS plugin_task_operations (
            plugin_id TEXT NOT NULL,
            operation_id TEXT NOT NULL,
            task_key TEXT,
            PRIMARY KEY (plugin_id, operation_id)
        );",
    )?;
    for (column, definition) in [
        ("installed_hash", "TEXT"),
        ("installed_path", "TEXT"),
        ("original_display_path", "TEXT"),
        ("ui_contributions", "TEXT NOT NULL DEFAULT '[]'"),
        ("capabilities", "TEXT NOT NULL DEFAULT '[]'"),
        ("background_service", "TEXT"),
        ("providers", "TEXT NOT NULL DEFAULT '[]'"),
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

/// Repairs local inventories whose `session.panel` contributions were rewritten to
/// `main-pane` by a former startup migration, using the immutable installed package manifest.
pub fn restore_session_panel_placements(conn: &Connection) -> Result<usize, String> {
    let mut statement = conn
        .prepare(
            "SELECT id, installed_path, ui_contributions FROM plugin_inventory
             WHERE source_kind = 'local' AND installed_path IS NOT NULL",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    let mut restored = 0;
    for (plugin_id, installed_path, json) in rows {
        let manifest_path = Path::new(&installed_path).join(crate::plugin_packages::MANIFEST_FILE);
        let Ok(manifest_json) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let manifest: PluginManifest = match serde_json::from_str(&manifest_json) {
            Ok(manifest) => manifest,
            Err(error) => {
                tracing::warn!(%plugin_id, %error, "skipping unreadable installed plugin manifest");
                continue;
            }
        };
        let session_panels = manifest
            .effective_ui_contributions()
            .into_iter()
            .filter(|contribution| contribution.placement == PluginUiPlacement::SessionPanel)
            .map(|contribution| contribution.id)
            .collect::<HashSet<_>>();
        let mut contributions: Vec<PluginUiContribution> =
            serde_json::from_str(&json).map_err(|error| {
                format!("failed to parse persisted UI contributions for {plugin_id}: {error}")
            })?;
        let mut changed = false;
        for contribution in &mut contributions {
            if contribution.placement == PluginUiPlacement::MainPane
                && session_panels.contains(&contribution.id)
            {
                contribution.placement = PluginUiPlacement::SessionPanel;
                changed = true;
            }
        }
        if !changed {
            continue;
        }
        let json = serde_json::to_string(&contributions)
            .map_err(|error| format!("failed to serialize plugin UI contributions: {error}"))?;
        conn.execute(
            "UPDATE plugin_inventory SET ui_contributions = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![plugin_id, json],
        )
        .map_err(|error| format!("failed to restore plugin UI contributions: {error}"))?;
        restored += 1;
    }
    Ok(restored)
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
        let background_service = serde_json::to_string(&manifest.background_service)
            .map_err(|error| format!("failed to serialize plugin background service: {error}"))?;
        conn.execute(
            "INSERT INTO plugin_inventory (
                id, name, version, host_api_version, source_kind, backend_entrypoint, ui_contributions, capabilities, background_service
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                version = excluded.version,
                host_api_version = excluded.host_api_version,
                source_kind = excluded.source_kind,
                backend_entrypoint = excluded.backend_entrypoint,
                ui_contributions = excluded.ui_contributions,
                capabilities = excluded.capabilities,
                background_service = excluded.background_service,
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
                background_service,
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

/// The manifest fields stored as JSON columns of `plugin_inventory`.
struct ManifestColumns {
    ui_contributions: String,
    capabilities: String,
    background_service: String,
    providers: String,
}

impl ManifestColumns {
    fn of(manifest: &PluginManifest) -> Result<Self, String> {
        fn json<T: Serialize>(value: &T, what: &str) -> Result<String, String> {
            serde_json::to_string(value)
                .map_err(|error| format!("failed to serialize plugin {what}: {error}"))
        }
        Ok(Self {
            ui_contributions: json(&manifest.effective_ui_contributions(), "UI contributions")?,
            capabilities: json(&manifest.capabilities, "capabilities")?,
            background_service: json(&manifest.background_service, "background service")?,
            providers: json(&manifest.providers, "providers")?,
        })
    }
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
    let ManifestColumns {
        ui_contributions,
        capabilities,
        background_service,
        providers,
    } = ManifestColumns::of(manifest)?;
    conn.execute(
        "INSERT INTO plugin_inventory (
            id, name, version, host_api_version, source_kind, backend_entrypoint, ui_contributions, capabilities, background_service,
            installed_hash, installed_path, original_display_path, enabled, runtime_state, providers
        ) VALUES (?1, ?2, ?3, ?4, 'local', ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, 'disabled', ?12)",
        params![
            manifest.id,
            manifest.name,
            manifest.version,
            manifest.host_api_version,
            backend_entrypoint,
            ui_contributions,
            capabilities,
            background_service,
            content_hash,
            package_dir.display().to_string(),
            original_display_path,
            providers,
        ],
    )
    .map_err(|error| format!("failed to persist imported local plugin: {error}"))?;
    get_inventory(conn, &manifest.id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "new local plugin inventory record was not found".to_string())
}

pub fn replace_local_inventory(
    conn: &Connection,
    manifest: &PluginManifest,
    backend_entrypoint: &str,
    content_hash: &str,
    package_dir: &Path,
    original_display_path: &str,
) -> Result<PluginInventory, String> {
    let existing = get_inventory(conn, &manifest.id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("plugin inventory entry not found: {}", manifest.id))?;
    if existing.source_kind != PluginSourceKind::Local {
        return Err(format!(
            "plugin id is reserved by a bundled plugin: {}",
            manifest.id
        ));
    }
    manifest.validate()?;
    validate_shortcut_collisions(conn, manifest)?;
    let ManifestColumns {
        ui_contributions,
        capabilities,
        background_service,
        providers,
    } = ManifestColumns::of(manifest)?;
    conn.execute(
        "UPDATE plugin_inventory SET
            name = ?2, version = ?3, host_api_version = ?4, backend_entrypoint = ?5,
            ui_contributions = ?6, capabilities = ?7, background_service = ?8,
            installed_hash = ?9, installed_path = ?10, original_display_path = ?11,
            enabled = 0, runtime_state = 'disabled', last_error = NULL, log_path = NULL,
            providers = ?12, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1 AND source_kind = 'local'",
        params![
            manifest.id,
            manifest.name,
            manifest.version,
            manifest.host_api_version,
            backend_entrypoint,
            ui_contributions,
            capabilities,
            background_service,
            content_hash,
            package_dir.display().to_string(),
            original_display_path,
            providers,
        ],
    )
    .map_err(|error| format!("failed to replace local plugin inventory: {error}"))?;
    get_inventory(conn, &manifest.id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "replaced local plugin inventory record was not found".to_string())
}

pub fn list_inventory(conn: &Connection) -> rusqlite::Result<Vec<PluginInventory>> {
    let mut statement = conn.prepare(
        "SELECT id, name, version, host_api_version, source_kind, backend_entrypoint,
                ui_contributions, capabilities, background_service, installed_hash, installed_path, original_display_path, enabled, runtime_state, last_error, log_path, providers
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
                ui_contributions, capabilities, background_service, installed_hash, installed_path, original_display_path, enabled, runtime_state, last_error, log_path, providers
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
    let state: String = row.get(13)?;
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
        background_service: match row.get::<_, Option<String>>(8)? {
            Some(json) => serde_json::from_str(&json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
            None => None,
        },
        installed_hash: row.get(9)?,
        installed_path: row.get(10)?,
        original_display_path: row.get(11)?,
        enabled: row.get(12)?,
        state: PluginRuntimeState::from_db(&state),
        last_error: row.get(14)?,
        log_path: row.get(15)?,
        providers: serde_json::from_str(&row.get::<_, String>(16)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                16,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
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
            background_workers: Arc::new(AsyncMutex::new(HashMap::new())),
            next_generation: AtomicU64::new(0),
            lifecycle: Arc::new(AsyncMutex::new(())),
            shutting_down: AtomicBool::new(false),
            exit_permitted: AtomicBool::new(false),
            provider_sessions: Arc::new(crate::plugin_providers::ProviderSessions::default()),
        }))
    }
}

pub struct PluginRuntimeSupervisor {
    db: Arc<Mutex<Connection>>,
    app: AppHandle,
    processes: Arc<AsyncMutex<HashMap<String, Arc<RuntimeProcess>>>>,
    background_workers: Arc<AsyncMutex<HashMap<String, BackgroundWorker>>>,
    next_generation: AtomicU64,
    lifecycle: Arc<AsyncMutex<()>>,
    shutting_down: AtomicBool,
    exit_permitted: AtomicBool,
    provider_sessions: Arc<crate::plugin_providers::ProviderSessions>,
}

struct BackgroundWorker {
    generation: u64,
    cancel: CancellationToken,
    wake: Arc<Notify>,
    task: JoinHandle<()>,
}

static NEXT_RUNTIME_INSTANCE: AtomicU64 = AtomicU64::new(1);

struct RuntimeProcess {
    app: AppHandle,
    /// Distinguishes sidecar restarts of the same plugin.
    instance: u64,
    child: AsyncMutex<Child>,
    stdin: AsyncMutex<ChildStdin>,
    /// Frames from the stdout reader task, minus the host notifications it handles itself.
    frames: AsyncMutex<mpsc::Receiver<Result<String, String>>>,
    request_lock: AsyncMutex<()>,
    next_request_id: AtomicU64,
    plugin_id: String,
    data_dir: PathBuf,
    capabilities: HashSet<PluginHostCapability>,
    lifecycle_event_subscriptions: AsyncMutex<HashSet<String>>,
    session_actions: AsyncMutex<Vec<PluginSessionAction>>,
    /// Set for provider plugins: the sessions this instance may drive.
    provider_sessions: Option<Arc<crate::plugin_providers::ProviderSessions>>,
    /// Why the stdout reader stopped, once it has: nothing the sidecar sends is read anymore.
    stdout_failure: Arc<std::sync::OnceLock<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionActionsRequest {
    actions: Vec<PluginSessionAction>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSessionAction {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RegisteredPluginSessionAction {
    pub plugin_id: String,
    pub id: String,
    pub label: String,
    pub providers: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PluginSessionAdvisorySeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PluginSessionAdvisory {
    session_id: String,
    message: String,
    severity: PluginSessionAdvisorySeverity,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PluginSessionCompletion {
    session_id: String,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginSessionPrompt {
    session_id: String,
    text: String,
}

/// A callback's params read as `T` and checked by `validate`. A bad payload is the plugin's
/// mistake: it answers -32602 and leaves the runtime running.
fn callback_params<T: serde::de::DeserializeOwned, U>(
    params: Value,
    subject: &str,
    validate: impl FnOnce(T) -> Result<U, String>,
) -> Result<U, CallbackError> {
    serde_json::from_value(params)
        .map_err(|error| format!("invalid {subject}: {error}"))
        .and_then(validate)
        .map_err(CallbackError::InvalidParams)
}

fn validate_plugin_session_actions(
    request: SessionActionsRequest,
) -> Result<Vec<PluginSessionAction>, String> {
    if request.actions.len() > 32 {
        return Err("plugin may register at most 32 session actions".to_string());
    }
    let mut ids = HashSet::new();
    for action in &request.actions {
        if action.id.is_empty()
            || !action.id.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            })
        {
            return Err(
                "plugin session action id must contain lowercase letters, digits, or hyphens"
                    .to_string(),
            );
        }
        if !ids.insert(&action.id) {
            return Err("plugin session action ids must be unique".to_string());
        }
        if action.label.trim().is_empty() || action.label.chars().count() > 80 {
            return Err("plugin session action label must be 1 to 80 characters".to_string());
        }
        let mut providers = HashSet::new();
        for provider in &action.providers {
            if provider.trim().is_empty()
                || provider.chars().count() > 80
                || !providers.insert(provider)
            {
                return Err(
                    "plugin session action providers must be unique nonempty strings".to_string(),
                );
            }
        }
    }
    Ok(request.actions)
}

fn validate_plugin_session_advisory(
    advisory: PluginSessionAdvisory,
) -> Result<PluginSessionAdvisory, String> {
    if advisory.session_id.trim().is_empty()
        || advisory.message.trim().is_empty()
        || advisory.message.chars().count() > 1_000
    {
        return Err("plugin session advisory requires a session_id and a message of at most 1000 characters".to_string());
    }
    Ok(advisory)
}

fn validate_plugin_session_prompt(
    prompt: PluginSessionPrompt,
) -> Result<PluginSessionPrompt, String> {
    if prompt.session_id.trim().is_empty()
        || prompt.text.trim().is_empty()
        || prompt.text.chars().count() > MAX_PLUGIN_SESSION_PROMPT_CHARS
    {
        return Err(format!(
            "plugin session prompt requires a session_id and nonempty text of at most {MAX_PLUGIN_SESSION_PROMPT_CHARS} characters"
        ));
    }
    Ok(prompt)
}

fn dispatch_plugin_session_prompt(
    capabilities: &HashSet<PluginHostCapability>,
    params: Value,
    send: impl FnOnce(&str, &str) -> Result<(), String>,
) -> Result<Value, CallbackError> {
    if !capabilities.contains(&PluginHostCapability::SessionsPrompt) {
        return Err(CallbackError::NotGranted);
    }
    let prompt = callback_params(
        params,
        "plugin session prompt",
        validate_plugin_session_prompt,
    )?;
    send(&prompt.session_id, &prompt.text)?;
    Ok(serde_json::json!({ "delivered": true }))
}

fn validate_plugin_session_completion(
    completion: PluginSessionCompletion,
) -> Result<PluginSessionCompletion, String> {
    if completion.session_id.trim().is_empty()
        || completion
            .message
            .as_ref()
            .is_some_and(|message| message.trim().is_empty() || message.chars().count() > 280)
    {
        return Err("plugin session completion requires a session_id and an optional nonempty message of at most 280 characters".to_string());
    }
    Ok(completion)
}

fn request_timeout(method: &str) -> Duration {
    match method {
        "jira.syncNow" => JIRA_SYNC_RPC_TIMEOUT,
        "plugin.taskLifecycle" | "plugin.sessionLifecycle" => JIRA_LIFECYCLE_RPC_TIMEOUT,
        _ => planeai_plugin_contract::provider::request_timeout(method).unwrap_or(RPC_TIMEOUT),
    }
}

impl RuntimeProcess {
    async fn request(&self, method: &str, params: Value) -> Result<Value, PluginRpcError> {
        self.request_with_timeout(method, params, request_timeout(method))
            .await
    }

    async fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        request_timeout: Duration,
    ) -> Result<Value, PluginRpcError> {
        let deadline = Instant::now() + request_timeout;
        // JSON-RPC responses share one stdout stream. Serialize normal host
        // requests while stdin remains independently writable for cancellation.
        let remaining = deadline.saturating_duration_since(Instant::now());
        let _request = timeout(remaining, self.request_lock.lock())
            .await
            .map_err(|_| request_queue_timeout_error(method))?;
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed) + 1;
        let frame = encode_request(request_id, method, params)?;
        self.write_request_frame(&frame, deadline)
            .await
            .map_err(PluginRpcError::Broken)?;

        let remaining = deadline.saturating_duration_since(Instant::now());
        match timeout(remaining, self.read_matching_response()).await {
            Ok(Ok(frame)) => decode_json_rpc_frame(&frame, request_id),
            Ok(Err(error)) => Err(PluginRpcError::Broken(error)),
            Err(_) => Err(self.cancel_timed_out_request(method, request_id).await),
        }
    }

    /// How a request that timed out ends, once cancelled.
    async fn cancel_timed_out_request(&self, method: &str, request_id: u64) -> PluginRpcError {
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        let frame = match encode_json_rpc_cancel_request_line(request_id) {
            Ok(frame) => frame,
            Err(error) => return PluginRpcError::Broken(error),
        };
        if self
            .write_cancellation_frame(&frame, deadline)
            .await
            .is_err()
        {
            return cancellation_acknowledgement_error(method, request_id, None);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        match timeout(remaining, self.read_matching_response()).await {
            Ok(Ok(frame)) => cancellation_acknowledgement_error(method, request_id, Some(&frame)),
            // The original timeout remains the externally meaningful error when
            // the plugin cannot acknowledge cancellation correctly or in time.
            Ok(Err(_)) | Err(_) => cancellation_acknowledgement_error(method, request_id, None),
        }
    }

    async fn write_request_frame(&self, frame: &str, deadline: Instant) -> Result<(), String> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let mut stdin = timeout(remaining, self.stdin.lock())
            .await
            .map_err(|_| "plugin RPC request timed out".to_string())?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        timeout(remaining, async {
            stdin
                .write_all(frame.as_bytes())
                .await
                .map_err(|error| format!("failed to write plugin JSON-RPC request: {error}"))?;
            stdin
                .flush()
                .await
                .map_err(|error| format!("failed to flush plugin JSON-RPC request: {error}"))
        })
        .await
        .map_err(|_| "plugin RPC request timed out".to_string())?
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

    async fn write_cancellation_frame(&self, frame: &str, deadline: Instant) -> Result<(), String> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let mut stdin = timeout(remaining, self.stdin.lock())
            .await
            .map_err(|_| "plugin cancellation notification timed out".to_string())?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        timeout(remaining, async {
            stdin.write_all(frame.as_bytes()).await.map_err(|error| {
                format!("failed to write plugin cancellation notification: {error}")
            })?;
            stdin.flush().await.map_err(|error| {
                format!("failed to flush plugin cancellation notification: {error}")
            })
        })
        .await
        .map_err(|_| "plugin cancellation notification timed out".to_string())?
    }

    /// The next response frame, serving the sidecar's callbacks until it arrives. Fails only
    /// when stdout or the protocol can no longer be trusted.
    async fn read_matching_response(&self) -> Result<String, String> {
        loop {
            let frame = self
                .frames
                .lock()
                .await
                .recv()
                .await
                .unwrap_or_else(|| Err(STDOUT_CLOSED_ERROR.to_string()))?;
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

    async fn serve_callback(&self, method: &str, params: Value) -> Result<Value, CallbackError> {
        match method {
            "host.sessions.actions" => {
                if !self
                    .capabilities
                    .contains(&PluginHostCapability::SessionActions)
                {
                    Err(CallbackError::NotGranted)
                } else {
                    let actions = callback_params(
                        params,
                        "plugin session actions",
                        validate_plugin_session_actions,
                    )?;
                    *self.session_actions.lock().await = actions.clone();
                    self.app
                        .emit(
                            "plugin-session-actions",
                            serde_json::json!({ "plugin_id": self.plugin_id, "actions": actions }),
                        )
                        .map_err(|error| {
                            format!("failed to emit plugin session actions: {error}")
                        })?;
                    Ok(serde_json::json!({ "accepted": true }))
                }
            }
            "host.sessions.advisory" => {
                if !self
                    .capabilities
                    .contains(&PluginHostCapability::SessionAdvisories)
                {
                    Err(CallbackError::NotGranted)
                } else {
                    let advisory = callback_params(
                        params,
                        "plugin session advisory",
                        validate_plugin_session_advisory,
                    )?;
                    self.app
                        .emit(
                            "plugin-session-advisory",
                            serde_json::json!({ "plugin_id": self.plugin_id, "advisory": advisory }),
                        )
                        .map_err(|error| format!("failed to emit plugin advisory: {error}"))?;
                    Ok(serde_json::json!({ "accepted": true }))
                }
            }
            "host.sessions.prompt" => {
                let capabilities = self.capabilities.clone();
                commands::blocking(move || {
                    let conn = Connection::open(planeai_paths::db_path())
                        .map_err(|error| error.to_string())?;
                    let ops =
                        crate::session_ops::real_prompt_ops(planeai_paths::notify_socket_path());
                    Ok(dispatch_plugin_session_prompt(
                        &capabilities,
                        params,
                        |session_id, text| {
                            crate::session_ops::send_prompt(&conn, session_id, text, &ops)
                                .map(|_| ())
                        },
                    ))
                })
                .await
                .map_err(CallbackError::Internal)
                .and_then(std::convert::identity)
            }
            "host.sessions.complete" => {
                if !self
                    .capabilities
                    .contains(&PluginHostCapability::SessionsComplete)
                {
                    Err(CallbackError::NotGranted)
                } else {
                    let completion = callback_params(
                        params,
                        "plugin session completion",
                        validate_plugin_session_completion,
                    )?;
                    self.app
                        .emit(
                            "plugin-session-completed",
                            serde_json::json!({ "plugin_id": self.plugin_id, "completion": completion }),
                        )
                        .map_err(|error| format!("failed to emit plugin completion: {error}"))?;
                    Ok(serde_json::json!({ "accepted": true }))
                }
            }
            _ => {
                let result = execute_host_task(
                    &self.plugin_id,
                    &self.capabilities,
                    &self.data_dir,
                    method,
                    params,
                )
                .await;
                announce_created_task(&self.app, method, &result);
                if method == "host.sessions.transitionLinkedTask" {
                    if let Ok(value) = &result {
                        if let Some(sessions) =
                            value.get("archived_sessions").and_then(Value::as_array)
                        {
                            let runtime = self.app.state::<PluginRuntimeHandle>();
                            let pty_state = self.app.state::<crate::state::PtyState>();
                            let mut archived_any_session = false;
                            for session in sessions {
                                if let Ok(session) =
                                    serde_json::from_value::<crate::db::Session>(session.clone())
                                {
                                    pty_state.0.detach(&session.id);
                                    runtime.0.dispatch_session_lifecycle(
                                        crate::commands::sessions::lifecycle::session_transition(
                                            &session,
                                            &session.status,
                                            "archived",
                                        ),
                                    );
                                    archived_any_session = true;
                                }
                            }
                            if archived_any_session {
                                let _ = self.app.emit("sessions-changed", ());
                            }
                        }
                    }
                }
                result.map(|mut value| {
                    if let Some(object) = value.as_object_mut() {
                        object.remove("archived_sessions");
                    }
                    value
                })
            }
        }
    }

    async fn handle_plugin_request(&self, request: JsonRpcCallbackRequest) -> Result<(), String> {
        let JsonRpcCallbackRequest { id, method, params } = request;
        let result = self.serve_callback(&method, params).await;
        let frame = encode_host_callback_response(id, result)?;
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

    /// Why the runtime ended: `Ok` when its process exited, `Err` when it must be stopped.
    async fn exited(&self) -> Result<Option<String>, String> {
        let exited = self
            .child
            .lock()
            .await
            .try_wait()
            .map_err(|e| format!("failed to check plugin process: {e}"))?
            .map(|value| format!("plugin process exited unexpectedly ({value})"));
        // A live process whose stdout can no longer be read would never answer or report again.
        match (exited, self.stdout_failure.get()) {
            (None, Some(failure)) => Err(failure.clone()),
            (exited, _) => Ok(exited),
        }
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
        decode_json_rpc_frame(&frame, request_id).map_err(String::from)
    }

    /// This instance no longer drives sessions; see `plugin_providers::runtime_stopped`.
    fn release_provider_sessions(&self) {
        if let Some(sessions) = &self.provider_sessions {
            crate::plugin_providers::runtime_stopped(
                &self.app,
                sessions,
                &self.plugin_id,
                self.instance,
            );
        }
    }

    async fn stop(&self) -> Result<(), String> {
        self.release_provider_sessions();
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

fn settings_path(params: &Value) -> Result<Option<Vec<String>>, String> {
    let Some(path) = params.get("path") else {
        return Ok(None);
    };
    let path = path
        .as_array()
        .ok_or("settings path must be an array of object keys")?;
    if path.is_empty() || path.len() > 8 {
        return Err("settings path must contain between 1 and 8 object keys".to_string());
    }
    path.iter()
        .map(|segment| {
            segment
                .as_str()
                .filter(|segment| !segment.is_empty() && segment.len() <= 256)
                .map(str::to_string)
                .ok_or_else(|| {
                    "settings path keys must be non-empty strings up to 256 bytes".to_string()
                })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn plugin_settings_at_path(settings: &Value, path: &[String]) -> Value {
    path.iter()
        .try_fold(settings, |current, segment| current.get(segment))
        .cloned()
        .unwrap_or(Value::Null)
}

fn merge_plugin_settings_patch(
    target: &mut serde_json::Map<String, Value>,
    patch: &serde_json::Map<String, Value>,
) {
    for (key, value) in patch {
        if value.is_null() {
            target.remove(key);
        } else if let Some(value) = value.as_object() {
            let target_value = target
                .entry(key.clone())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            if !target_value.is_object() {
                *target_value = Value::Object(serde_json::Map::new());
            }
            merge_plugin_settings_patch(
                target_value
                    .as_object_mut()
                    .expect("object target was initialized above"),
                value,
            );
        } else {
            target.insert(key.clone(), value.clone());
        }
    }
}

fn patch_plugin_settings(data_dir: &Path, patch: Value) -> Result<(), String> {
    let patch = patch
        .as_object()
        .ok_or("plugin settings patch must be a JSON object")?;
    let mut settings = read_plugin_settings(data_dir)?;
    merge_plugin_settings_patch(
        settings
            .as_object_mut()
            .expect("read_plugin_settings validates JSON objects"),
        patch,
    );
    replace_plugin_settings(data_dir, settings).map(|_| ())
}

fn legacy_jira_issue_matches(
    conn: &Connection,
    issue_key: &str,
    summary: &str,
) -> Result<bool, String> {
    let has_legacy_issues_table: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'jira_issues')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect legacy Jira issue storage: {error}"))?;
    if !has_legacy_issues_table {
        return Ok(false);
    }
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM jira_issues issue
            JOIN tasks task ON task.key = issue.issue_key
            WHERE issue.issue_key = ?1 AND issue.summary = ?2 AND task.title = issue.summary
        )",
        params![issue_key, summary],
        |row| row.get(0),
    )
    .map_err(|error| format!("failed to match legacy Jira issue: {error}"))
}

/// A host task a sidecar or UI calls, gated by the plugin's capabilities.
async fn execute_host_task(
    plugin_id: &str,
    capabilities: &HashSet<PluginHostCapability>,
    data_dir: &Path,
    method: &str,
    params: Value,
) -> Result<Value, CallbackError> {
    let required = match method {
        "host.settings.get" | "host.settings.replace" | "host.settings.patch" => {
            PluginHostCapability::Settings
        }
        "host.projects.list" => PluginHostCapability::ProjectsRead,
        "host.sessions.list" => PluginHostCapability::SessionsRead,
        "host.sessions.repositoryContext" => PluginHostCapability::SessionsRepositoryContext,
        "host.sessions.transitionLinkedTask" => PluginHostCapability::TasksTransition,
        "host.tasks.read" | "host.task.get" | "host.jira.legacyIssue.matches" => {
            PluginHostCapability::TasksRead
        }
        "host.tasks.create" | "host.tasks.createChild" => PluginHostCapability::TasksCreate,
        "host.task.create" => PluginHostCapability::TasksCreate,
        "host.task.update" => PluginHostCapability::TasksUpdate,
        _ => return Err(CallbackError::NotFound),
    };
    if !capabilities.contains(&required)
        || (matches!(
            method,
            "host.task.create" | "host.task.update" | "host.jira.legacyIssue.matches"
        ) && plugin_id != JIRA_PLUGIN_ID)
    {
        return Err(CallbackError::NotGranted);
    }
    run_host_task(plugin_id, data_dir, method, params).await
}

/// A top-level task emits no lifecycle event, so the task list hears of it directly.
fn announce_created_task(app: &AppHandle, method: &str, result: &Result<Value, CallbackError>) {
    if method == "host.tasks.create" && result.is_ok() {
        if let Err(error) = app.emit("tasks-changed", ()) {
            tracing::warn!(%error, "failed to announce a plugin-created task");
        }
    }
}

fn invalid_params(message: &str) -> CallbackError {
    CallbackError::InvalidParams(message.to_string())
}

/// Runs a host task's storage work off the async runtime; its failures are internal.
async fn blocking_host_task<F>(work: F) -> Result<Value, CallbackError>
where
    F: FnOnce() -> Result<Value, String> + Send + 'static,
{
    commands::blocking(work)
        .await
        .map_err(CallbackError::Internal)
}

/// Runs a host task whose params are checked first, so a bad request answers -32602.
async fn run_host_task(
    plugin_id: &str,
    data_dir: &Path,
    method: &str,
    params: Value,
) -> Result<Value, CallbackError> {
    if method == "host.projects.list" {
        return blocking_host_task(|| {
            let path = planeai_paths::db_path();
            let conn = Connection::open(path).map_err(|error| error.to_string())?;
            let projects = crate::db::list_projects(&conn)
                .map_err(|error| error.to_string())?
                .into_iter()
                .filter(|project| !project.hidden)
                .map(|project| {
                    serde_json::json!({
                        "id": project.id,
                        "name": project.name,
                        "path": project.path,
                        "hidden": project.hidden,
                    })
                })
                .collect::<Vec<_>>();
            Ok(serde_json::json!({ "projects": projects }))
        })
        .await;
    }

    if method == "host.sessions.list" {
        return blocking_host_task(|| {
            let path = planeai_paths::db_path();
            let conn = Connection::open(path).map_err(|error| error.to_string())?;
            let sessions = crate::db::list_sessions(&conn)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|session| {
                    serde_json::json!({
                        "id": session.id,
                        "project_id": session.project_id,
                        "name": session.name,
                        "branch": session.branch,
                        "status": session.status,
                        "created_at": session.created_at,
                        "provider": session.provider,
                        "backend": session.backend,
                        "task_key": session.task_key,
                    })
                })
                .collect::<Vec<_>>();
            Ok(serde_json::json!({ "sessions": sessions }))
        })
        .await;
    }

    if method == "host.sessions.repositoryContext" {
        let session_id = params
            .get("session_id")
            .or_else(|| params.get("sessionId"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| invalid_params("repository context requires session_id"))?
            .to_string();
        return blocking_host_task(move || {
            let path = planeai_paths::db_path();
            let conn = Connection::open(path).map_err(|error| error.to_string())?;
            let session = crate::db::get_session(&conn, &session_id)
                .map_err(|error| error.to_string())?
                .ok_or("session not found")?;
            let project = crate::db::get_project(&conn, &session.project_id)
                .map_err(|error| error.to_string())?
                .ok_or("session project not found")?;
            if project.hidden {
                return Err("session project is hidden".to_string());
            }
            let working_tree_path = session
                .worktree_path
                .clone()
                .unwrap_or(project.path.clone());
            Ok(serde_json::json!({
                "session_id": session.id,
                "project_id": project.id,
                "working_tree_path": working_tree_path,
                "branch": session.branch,
                "base_branch": session.base_branch,
                "session_status": session.status,
                "linked_task_key": session.task_key,
            }))
        })
        .await;
    }

    if method == "host.sessions.transitionLinkedTask" {
        let session_id = params
            .get("session_id")
            .or_else(|| params.get("sessionId"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| invalid_params("linked task transition requires session_id"))?
            .to_string();
        let status = params
            .get("status")
            .and_then(Value::as_str)
            .and_then(Status::parse)
            .ok_or_else(|| invalid_params("linked task transition requires a valid status"))?;
        return blocking_host_task(move || transition_linked_plugin_task(&session_id, status))
            .await;
    }

    if matches!(method, "host.tasks.create" | "host.tasks.createChild") {
        let plugin_id = plugin_id.to_string();
        let request =
            PluginTaskRequest::from_params(&params).map_err(CallbackError::InvalidParams)?;
        match (method, &request.parent_key) {
            ("host.tasks.createChild", None) => {
                return Err(invalid_params("child task create requires parent_key"))
            }
            ("host.tasks.create", Some(_)) => {
                return Err(invalid_params(
                    "host.tasks.create creates a top-level task; use host.tasks.createChild for a child",
                ))
            }
            _ => {}
        }
        return blocking_host_task(move || create_plugin_task(&plugin_id, request)).await;
    }

    if matches!(
        method,
        "host.settings.get" | "host.settings.replace" | "host.settings.patch"
    ) {
        let data_dir = data_dir.to_path_buf();
        let method = method.to_string();
        let path = match method.as_str() {
            "host.settings.get" => settings_path(&params).map_err(CallbackError::InvalidParams)?,
            _ => None,
        };
        let document = match method.as_str() {
            "host.settings.replace" => params.get("settings").cloned().unwrap_or(params),
            "host.settings.patch" => params.get("patch").cloned().unwrap_or(params),
            _ => Value::Null,
        };
        if method != "host.settings.get" && !document.is_object() {
            return Err(invalid_params("plugin settings must be a JSON object"));
        }
        return blocking_host_task(move || match method.as_str() {
            "host.settings.get" => read_plugin_settings(&data_dir).map(|settings| {
                let settings = path
                    .as_deref()
                    .map(|path| plugin_settings_at_path(&settings, path))
                    .unwrap_or(settings);
                serde_json::json!({ "settings": settings })
            }),
            "host.settings.replace" => replace_plugin_settings(&data_dir, document)
                .map(|settings| serde_json::json!({ "settings": settings })),
            "host.settings.patch" => patch_plugin_settings(&data_dir, document)
                .map(|()| serde_json::json!({ "updated": true })),
            _ => unreachable!(),
        })
        .await;
    }
    if method == "host.jira.legacyIssue.matches" {
        let key = params
            .get("key")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| invalid_params("legacy Jira issue lookup requires key"))?
            .to_string();
        let summary = params
            .get("summary")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_params("legacy Jira issue lookup requires summary"))?
            .to_string();
        return blocking_host_task(move || {
            let path = planeai_paths::db_path();
            let conn = Connection::open(path).map_err(|error| error.to_string())?;
            legacy_jira_issue_matches(&conn, &key, &summary)
                .map(|matches| serde_json::json!({ "matches": matches }))
        })
        .await;
    }

    if matches!(method, "host.tasks.read" | "host.task.get") {
        let key = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_params("task read requires key"))?
            .to_string();
        return blocking_host_task(move || {
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
    let write = match method {
        "host.task.create" => TaskWrite::Create(task_create_params(&params)?),
        "host.task.update" => {
            let (key, update) = task_update_params(&params)?;
            TaskWrite::Update(key, update)
        }
        _ => return Err(CallbackError::NotFound),
    };
    let data_dir = data_dir.to_path_buf();
    blocking_host_task(move || {
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
        let task = match write {
            TaskWrite::Create(create) => repo.create(create),
            TaskWrite::Update(key, update) => repo.update(&key, update),
        }
        .map_err(|error| error.to_string())?;
        serde_json::to_value(task).map_err(|error| error.to_string())
    })
    .await
}

enum TaskWrite {
    Create(CreateParams),
    Update(String, UpdateParams),
}

fn task_status_param(params: &Value) -> Result<Option<Status>, CallbackError> {
    params
        .get("status")
        .and_then(Value::as_str)
        .map(|value| Status::parse(value).ok_or_else(|| invalid_params("invalid task status")))
        .transpose()
}

fn task_tags_param(params: &Value) -> Result<Option<Vec<String>>, CallbackError> {
    params
        .get("tags")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| CallbackError::InvalidParams(format!("invalid task tags: {error}")))
}

fn task_create_params(params: &Value) -> Result<CreateParams, CallbackError> {
    Ok(CreateParams {
        key: params
            .get("key")
            .and_then(Value::as_str)
            .map(str::to_string),
        title: params
            .get("title")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_params("task create requires title"))?
            .to_string(),
        description: params
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        status: task_status_param(params)?,
        priority: params.get("priority").and_then(Value::as_i64).unwrap_or(0) as i32,
        tags: task_tags_param(params)?.unwrap_or_default(),
        ..Default::default()
    })
}

fn task_update_params(params: &Value) -> Result<(String, UpdateParams), CallbackError> {
    let key = params
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_params("task update requires key"))?
        .to_string();
    let update = UpdateParams {
        title: params
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string),
        description: params
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string),
        status: task_status_param(params)?,
        priority: params
            .get("priority")
            .and_then(Value::as_i64)
            .map(|value| value as i32),
        tags: task_tags_param(params)?,
        ..Default::default()
    };
    Ok((key, update))
}

/// Whether a plugin starts after its reconciliation: only a broken runtime fails the start;
/// a plugin that refused it still runs, its stale data kept until the next start.
fn startup_reconciliation(
    plugin_id: &str,
    result: Result<(), PluginRpcError>,
) -> Result<(), String> {
    match result {
        Err(error) if error.is_fatal() => Err(error.into()),
        Err(error) => {
            tracing::warn!(plugin_id, %error, "provider session reconciliation failed");
            Ok(())
        }
        Ok(()) => Ok(()),
    }
}

fn transition_linked_plugin_task(session_id: &str, status: Status) -> Result<Value, String> {
    let path = planeai_paths::db_path();
    let conn = Connection::open(&path).map_err(|error| error.to_string())?;
    let session = crate::db::get_session(&conn, session_id)
        .map_err(|error| error.to_string())?
        .ok_or("session not found")?;
    let task_key = session
        .task_key
        .clone()
        .ok_or("session has no linked task")?;
    let project = crate::db::get_project(&conn, session.task_project_id())
        .map_err(|error| error.to_string())?
        .ok_or("session project not found")?;
    if project.hidden {
        return Err("session project is hidden".to_string());
    }
    let repo = SqliteRepository::open(
        path.to_str().ok_or("invalid PlaneAI task database path")?,
        &project.prefix,
    )
    .map_err(|error| error.to_string())?;
    let (task, events) =
        planeai_core::task_lifecycle::move_task_with_lifecycle(&repo, &task_key, status)
            .map_err(|error| error.to_string())?;
    if !events.is_empty() {
        planeai::task_cli::notify_task_lifecycle(&TaskLifecycleBatch::new(
            crate::task_lifecycle::TaskLifecycleOrigin::Ui,
            project.id,
            project.prefix,
            events,
        ));
    }
    let archived_sessions = if status == Status::Done {
        crate::session_ops::archive_sessions_for_task(&conn, &task_key, &None)
    } else {
        Vec::new()
    };
    serde_json::to_value(task)
        .map(|task| serde_json::json!({ "task": task, "archived_sessions": archived_sessions }))
        .map_err(|error| error.to_string())
}

/// A plugin task request's params, checked before anything is written.
#[derive(Debug)]
struct PluginTaskRequest {
    project_path: String,
    parent_key: Option<String>,
    operation_id: String,
    title: String,
    description: String,
}

impl PluginTaskRequest {
    fn from_params(params: &Value) -> Result<Self, String> {
        let text = |names: &[&str]| {
            names
                .iter()
                .find_map(|name| params.get(*name))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
        };
        let required = |names: &[&str], what: &str| {
            text(names).ok_or_else(|| format!("plugin task create requires {what}"))
        };
        Ok(Self {
            project_path: required(&["project_path", "projectPath"], "project_path")?,
            parent_key: text(&["parent_key", "parentKey"]),
            operation_id: required(&["operation_id", "operationId"], "operation_id")?,
            title: required(&["title"], "title")?,
            description: params
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })
    }
}

#[derive(Debug)]
struct InsertedPluginTask {
    task: Value,
    /// Present only when this call created a child task.
    lifecycle: Option<TaskLifecycleBatch>,
}

fn create_plugin_task(plugin_id: &str, request: PluginTaskRequest) -> Result<Value, String> {
    let mut conn = Connection::open(planeai_paths::db_path()).map_err(|error| error.to_string())?;
    let inserted = insert_plugin_task(&mut conn, plugin_id, request)?;
    if let Some(batch) = &inserted.lifecycle {
        planeai::task_cli::notify_task_lifecycle(batch);
    }
    Ok(inserted.task)
}

/// Creates a task at most once per (plugin, operation id), so a retried request returns the
/// task the first attempt created.
fn insert_plugin_task(
    conn: &mut Connection,
    plugin_id: &str,
    request: PluginTaskRequest,
) -> Result<InsertedPluginTask, String> {
    let PluginTaskRequest {
        project_path,
        parent_key,
        operation_id,
        title,
        description,
    } = request;
    let project = crate::db::list_projects(conn)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|project| project.path == project_path && !project.hidden)
        .ok_or("project was not found or is hidden")?;
    let existing = |task_key: String, conn: &Connection| {
        plugin_task_value(conn, &task_key).map(|task| InsertedPluginTask {
            task,
            lifecycle: None,
        })
    };
    let transaction = conn.transaction().map_err(|error| error.to_string())?;
    if let Some(task_key) = transaction
        .query_row(
            "SELECT task_key FROM plugin_task_operations WHERE plugin_id = ?1 AND operation_id = ?2",
            rusqlite::params![plugin_id, operation_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten()
    {
        transaction.commit().map_err(|error| error.to_string())?;
        return existing(task_key, conn);
    }
    let reserved = transaction
        .execute(
            "INSERT OR IGNORE INTO plugin_task_operations (plugin_id, operation_id, task_key) VALUES (?1, ?2, NULL)",
            rusqlite::params![plugin_id, operation_id],
        )
        .map_err(|error| error.to_string())?;
    if reserved == 0 {
        transaction.commit().map_err(|error| error.to_string())?;
        let task_key = conn
            .query_row(
                "SELECT task_key FROM plugin_task_operations WHERE plugin_id = ?1 AND operation_id = ?2",
                rusqlite::params![plugin_id, operation_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(|error| error.to_string())?
            .ok_or("plugin task operation is still in progress")?;
        return existing(task_key, conn);
    }
    if let Some(parent_key) = &parent_key {
        let parent_prefix = transaction
            .query_row(
                "SELECT project_prefix FROM tasks WHERE key = ?1",
                rusqlite::params![parent_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or("parent task was not found")?;
        if parent_prefix != project.prefix {
            return Err("parent task does not belong to the selected project".to_string());
        }
    }
    transaction
        .execute(
            "INSERT OR IGNORE INTO task_projects (prefix, next_seq) VALUES (?1, 1)",
            rusqlite::params![project.prefix],
        )
        .map_err(|error| error.to_string())?;
    let sequence: i64 = transaction
        .query_row(
            "UPDATE task_projects SET next_seq = next_seq + 1 WHERE prefix = ?1 RETURNING next_seq - 1",
            rusqlite::params![project.prefix],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let task_key = format!("{}-{sequence}", project.prefix);
    let first_child = match &parent_key {
        Some(parent_key) => Some(
            !transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM tasks WHERE parent_key = ?1)",
                    rusqlite::params![parent_key],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|error| error.to_string())?,
        ),
        None => None,
    };
    let now = chrono::Utc::now().to_rfc3339();
    transaction
        .execute(
            "INSERT INTO tasks (key, project_prefix, title, description, status, priority, parent_key, base_branch, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'todo', 0, ?5, 'main', ?6, ?6)",
            rusqlite::params![task_key, project.prefix, title, description, parent_key, now],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE plugin_task_operations SET task_key = ?3 WHERE plugin_id = ?1 AND operation_id = ?2",
            rusqlite::params![plugin_id, operation_id, task_key],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    let lifecycle = parent_key
        .zip(first_child)
        .map(|(parent_key, first_child)| {
            TaskLifecycleBatch::new(
                crate::task_lifecycle::TaskLifecycleOrigin::Ui,
                project.id,
                project.prefix,
                vec![crate::task_lifecycle::TaskLifecycleEvent::ChildAssigned {
                    child_key: task_key.clone(),
                    parent_key,
                    is_first_child_assignment: first_child,
                }],
            )
        });
    Ok(InsertedPluginTask {
        task: plugin_task_value(conn, &task_key)?,
        lifecycle,
    })
}

fn plugin_task_value(conn: &Connection, task_key: &str) -> Result<Value, String> {
    let task = conn
        .query_row(
            "SELECT key, title, description, status, priority, parent_key, COALESCE(base_branch, 'main') FROM tasks WHERE key = ?1",
            rusqlite::params![task_key],
            |row| Ok(serde_json::json!({
                "key": row.get::<_, String>(0)?,
                "title": row.get::<_, String>(1)?,
                "description": row.get::<_, String>(2)?,
                "status": row.get::<_, String>(3)?,
                "priority": row.get::<_, i32>(4)?,
                "parent_key": row.get::<_, Option<String>>(5)?,
                "url": Value::Null,
                "base_branch": row.get::<_, String>(6)?,
                "blocked_by": Vec::<String>::new(),
                "tags": Vec::<String>::new(),
            })),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or("idempotent plugin task result was not found")?;
    Ok(serde_json::json!({ "task": task }))
}

fn lifecycle_subscription_is_granted(
    capabilities: &HashSet<PluginHostCapability>,
    subscriptions: &HashSet<String>,
) -> bool {
    capabilities.contains(&PluginHostCapability::TaskEvents)
        && subscriptions.contains("task.lifecycle")
}

fn session_lifecycle_subscription_is_granted(
    capabilities: &HashSet<PluginHostCapability>,
    subscriptions: &HashSet<String>,
) -> bool {
    capabilities.contains(&PluginHostCapability::SessionEvents)
        && subscriptions.contains("session.lifecycle")
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
        let updated = if plugin_id == JIRA_PLUGIN_ID {
            // Jira settings govern cache ownership and connected-site invariants, so only
            // its sidecar may persist them. Generic host persistence would bypass both.
            self.call(plugin_id, "jira.settings.update", settings).await
        } else {
            self.inventory(plugin_id)
                .await?
                .ok_or_else(|| format!("plugin inventory entry not found: {plugin_id}"))?;
            let root = plugin_state_root(&self.app, plugin_id).await?;
            commands::blocking(move || replace_plugin_settings(&root.join("data"), settings)).await
        }?;
        self.wake_background_worker(plugin_id).await;
        Ok(updated)
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

    /// Return the current sidecar-registered session actions. Registrations are
    /// retained by each running runtime process so the frontend can resync after
    /// its event listener and inventory load complete.
    pub async fn session_actions(&self) -> Vec<RegisteredPluginSessionAction> {
        let processes = self
            .processes
            .lock()
            .await
            .iter()
            .map(|(plugin_id, process)| (plugin_id.clone(), process.clone()))
            .collect::<Vec<_>>();
        let mut actions = Vec::new();
        for (plugin_id, process) in processes {
            actions.extend(
                process
                    .session_actions
                    .lock()
                    .await
                    .iter()
                    .cloned()
                    .map(|action| RegisteredPluginSessionAction {
                        plugin_id: plugin_id.clone(),
                        id: action.id,
                        label: action.label,
                        providers: action.providers,
                    }),
            );
        }
        actions
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

    /// Capability-gated PlaneAI data RPC for a sandboxed local-plugin UI.
    /// The Tauri command receives the owner ID from the host frame, never from
    /// the iframe module itself.
    pub async fn host_call(
        &self,
        plugin_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let inventory = self
            .inventory(plugin_id)
            .await?
            .ok_or_else(|| format!("plugin inventory entry not found: {plugin_id}"))?;
        if inventory.source_kind != PluginSourceKind::Local {
            return Err("direct host RPC is available only to local plugins".to_string());
        }
        if inventory.state != PluginRuntimeState::Running {
            return Err(format!("plugin {plugin_id} is not running"));
        }
        let root = plugin_state_root(&self.app, plugin_id).await?;
        let capabilities = inventory
            .capabilities
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        let callback_method = format!("host.{method}");
        let result = execute_host_task(
            plugin_id,
            &capabilities,
            &root.join("data"),
            &callback_method,
            params,
        )
        .await;
        announce_created_task(&self.app, &callback_method, &result);
        result.map_err(|error| error.to_string())
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
        let existing = self.inventory(&manifest.id).await?;
        let was_enabled = match existing.as_ref() {
            Some(inventory) if inventory.source_kind != PluginSourceKind::Local => {
                let error = format!("plugin id is reserved by a bundled plugin: {}", manifest.id);
                if let Err(cleanup_error) =
                    commands::blocking(move || imported.discard_if_newly_published()).await
                {
                    tracing::warn!(%cleanup_error, "failed to discard unreferenced local plugin package");
                }
                return Err(error);
            }
            Some(inventory) if inventory.installed_hash.as_deref() == Some(&hash) => {
                return Ok(inventory.clone());
            }
            Some(inventory) => inventory.enabled,
            None => false,
        };
        if existing
            .as_ref()
            .is_some_and(|inventory| inventory.state != PluginRuntimeState::Disabled)
        {
            self.disable_inner(&manifest.id).await?;
        }
        let replacing = existing.is_some();
        let inventory = self
            .with_db(move |conn| {
                if replacing {
                    replace_local_inventory(
                        conn,
                        &manifest,
                        &backend,
                        &hash,
                        &package_dir,
                        &original_path,
                    )
                } else {
                    insert_local_inventory(
                        conn,
                        &manifest,
                        &backend,
                        &hash,
                        &package_dir,
                        &original_path,
                    )
                }
            })
            .await;
        let inventory = match inventory {
            Ok(inventory) => inventory,
            Err(error) => {
                if let Err(cleanup_error) =
                    commands::blocking(move || imported.discard_if_newly_published()).await
                {
                    tracing::warn!(%cleanup_error, "failed to discard unreferenced local plugin package");
                }
                return Err(error);
            }
        };
        if was_enabled {
            self.ensure_plugin_migration_ready(&inventory.id).await?;
            return self.enable_inner(&inventory.id).await;
        }
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
        let result = self.call_internal(plugin_id, method, params, false).await;
        if result.is_ok() && plugin_id == JIRA_PLUGIN_ID && method == "jira.connect.complete" {
            self.wake_background_worker(plugin_id).await;
        }
        result.map_err(String::from)
    }

    async fn call_internal(
        &self,
        plugin_id: &str,
        method: &str,
        params: Value,
        host_controlled: bool,
    ) -> Result<Value, PluginRpcError> {
        if !host_controlled && is_host_controlled_plugin_method(method) {
            return Err("plugin lifecycle methods are reserved for PlaneAI"
                .to_string()
                .into());
        }
        let process = {
            let _lifecycle = self.lifecycle.lock().await;
            let inventory = self.inventory(plugin_id).await?.ok_or_else(|| {
                PluginRpcError::NotRunning(format!("plugin inventory entry not found: {plugin_id}"))
            })?;
            if inventory.state != PluginRuntimeState::Running {
                return Err(PluginRpcError::NotRunning(format!(
                    "plugin {plugin_id} is not running"
                )));
            }
            self.processes.lock().await.get(plugin_id).cloned()
        };
        let process = process.ok_or_else(|| {
            PluginRpcError::NotRunning(format!("plugin runtime was not available: {plugin_id}"))
        })?;
        let result = process.request(method, params).await;
        let Err(error) = result else {
            return result;
        };
        if !error.is_fatal() {
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
                    Some(error.to_string()),
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

    /// Deliver a committed session lifecycle event in the background. Delivery
    /// is best-effort and requires both manifest capability and handshake opt-in.
    pub fn dispatch_session_lifecycle(
        self: &Arc<Self>,
        transition: crate::commands::sessions::lifecycle::SessionTransition,
    ) {
        if self.shutting_down.load(Ordering::Acquire) {
            tracing::warn!(
                "skipped session lifecycle delivery while plugin runtime is shutting down"
            );
            return;
        }
        let supervisor = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let crate::commands::sessions::lifecycle::SessionTransition {
                event,
                provider_session,
            } = transition;
            // Sessions the GUI ends dispatch this event; ends from the CLI or task
            // completion reach `plugin_providers::reconcile` instead.
            if let Some(reason) = crate::plugin_providers::stop_reason(&event.status) {
                // Every path that ends a session in the GUI dispatches this, so its handoff
                // programs end here, whichever path it was.
                let tabs = supervisor
                    .app
                    .state::<crate::state::TerminalTabsState>()
                    .0
                    .clone();
                tabs.session_ended(&event.session_id).await;
                if let Some(target) = provider_session {
                    let runtime = crate::plugin_providers::AppRuntime::new(&supervisor.app);
                    crate::plugin_providers::stop(&runtime, &target, reason).await;
                }
            }
            let event = match serde_json::to_value(&event) {
                Ok(event) => event,
                Err(error) => {
                    tracing::warn!(%error, "failed to encode session lifecycle event");
                    return;
                }
            };
            let processes = supervisor
                .processes
                .lock()
                .await
                .iter()
                .map(|(plugin_id, process)| (plugin_id.clone(), process.clone()))
                .collect::<Vec<_>>();
            for (plugin_id, process) in processes {
                let subscriptions = process.lifecycle_event_subscriptions.lock().await;
                let subscribed = session_lifecycle_subscription_is_granted(
                    &process.capabilities,
                    &subscriptions,
                );
                drop(subscriptions);
                if !subscribed {
                    continue;
                }
                let supervisor = Arc::clone(&supervisor);
                let event = event.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = supervisor
                        .call_internal(
                            &plugin_id,
                            "plugin.sessionLifecycle",
                            serde_json::json!({ "event": event }),
                            true,
                        )
                        .await
                    {
                        tracing::warn!(plugin_id, %error, "session lifecycle delivery failed");
                    }
                });
            }
        });
    }

    pub fn provider_sessions(&self) -> &Arc<crate::plugin_providers::ProviderSessions> {
        &self.provider_sessions
    }

    /// The live sidecar instance for a plugin, if it is running.
    pub async fn running_instance(&self, plugin_id: &str) -> Option<u64> {
        self.processes
            .lock()
            .await
            .get(plugin_id)
            .map(|process| process.instance)
    }

    /// A provider declared by a running plugin that holds the providers capability.
    pub async fn running_provider(
        &self,
        plugin_id: &str,
        provider_id: &str,
    ) -> Result<PluginProvider, String> {
        let inventory = self
            .inventory(plugin_id)
            .await?
            .ok_or_else(|| format!("Unknown provider: {plugin_id}:{provider_id}"))?;
        if !inventory
            .capabilities
            .contains(&PluginHostCapability::Providers)
        {
            return Err(format!("plugin {plugin_id} does not provide sessions"));
        }
        let provider = inventory
            .providers
            .into_iter()
            .find(|provider| provider.id == provider_id)
            .ok_or_else(|| format!("Unknown provider: {plugin_id}:{provider_id}"))?;
        if inventory.state != PluginRuntimeState::Running {
            return Err(format!(
                "{} is unavailable because plugin {} is not running",
                provider.label, inventory.name
            ));
        }
        Ok(provider)
    }

    /// Host-controlled provider session request; plugin UI cannot reach these methods.
    pub async fn provider_request(
        &self,
        plugin_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, PluginRpcError> {
        debug_assert!(method.starts_with("provider."));
        self.call_internal(plugin_id, method, params, true).await
    }

    pub async fn local_provider_ui_source(
        &self,
        plugin_id: &str,
        provider_id: &str,
    ) -> Result<String, String> {
        self.read_local_bundle(plugin_id, |inventory| {
            inventory
                .providers
                .iter()
                .find(|provider| provider.id == provider_id)
                .map(|provider| provider.entrypoint.clone())
                .ok_or_else(|| format!("plugin {plugin_id} has no provider {provider_id}"))
        })
        .await
    }

    pub async fn local_ui_source(
        &self,
        plugin_id: &str,
        contribution_id: &str,
    ) -> Result<String, String> {
        self.read_local_bundle(plugin_id, |inventory| {
            inventory
                .ui_contributions
                .iter()
                .find(|contribution| contribution.id == contribution_id)
                .map(|contribution| contribution.entrypoint.clone())
                .ok_or_else(|| {
                    format!("plugin {plugin_id} has no UI contribution {contribution_id}")
                })
        })
        .await
    }

    /// Read a UI bundle from a local plugin's imported package.
    async fn read_local_bundle(
        &self,
        plugin_id: &str,
        entrypoint: impl FnOnce(&PluginInventory) -> Result<String, String>,
    ) -> Result<String, String> {
        let inventory = self
            .inventory(plugin_id)
            .await?
            .ok_or_else(|| format!("plugin inventory entry not found: {plugin_id}"))?;
        if inventory.source_kind != PluginSourceKind::Local {
            return Err("builtin plugins use their bundled UI entrypoints".to_string());
        }
        let entrypoint = entrypoint(&inventory)?;
        let package = inventory
            .installed_path
            .ok_or_else(|| format!("local plugin {plugin_id} has no imported package path"))?;
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
        let (migration_blocks_jira, migration_blocks_github) = self
            .with_db(|conn| {
                Ok((
                    crate::jira_migration::blocks_plugin_start(conn),
                    crate::github_migration::blocks_plugin_start(conn),
                ))
            })
            .await
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "failed to read plugin migration fences; refusing protected plugin startup");
                (true, true)
            });
        for plugin in enabled {
            if plugin.id == JIRA_PLUGIN_ID && migration_blocks_jira {
                tracing::info!(
                    "Jira plugin remains disabled until explicit legacy migration completes"
                );
                continue;
            }
            if plugin.id == GITHUB_PLUGIN_ID && migration_blocks_github {
                tracing::info!(
                    "GitHub plugin remains disabled until explicit legacy migration completes"
                );
                continue;
            }
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
            if let Err(error) = self.ensure_plugin_migration_ready(plugin_id).await {
                tracing::warn!(plugin_id, %error, "skipped protected plugin restore after update failure");
                continue;
            }
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
        self.stop_background_worker(plugin_id).await;
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

    async fn ensure_plugin_migration_ready(&self, plugin_id: &str) -> Result<(), String> {
        let blocked = match plugin_id {
            JIRA_PLUGIN_ID => {
                self.with_db(|conn| Ok(crate::jira_migration::blocks_plugin_start(conn)))
                    .await?
            }
            GITHUB_PLUGIN_ID => {
                self.with_db(|conn| Ok(crate::github_migration::blocks_plugin_start(conn)))
                    .await?
            }
            _ => false,
        };
        if !blocked {
            return Ok(());
        }
        Err(match plugin_id {
            JIRA_PLUGIN_ID => "Jira is waiting for explicit legacy migration. Use Migrate and enable Jira plugin in Plugins first.",
            GITHUB_PLUGIN_ID => "GitHub migration is required before enabling the GitHub plugin. Migrate legacy GitHub pull-request mappings first.",
            _ => unreachable!(),
        }.to_string())
    }

    pub async fn enable(&self, plugin_id: &str) -> Result<PluginInventory, String> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("plugin runtime is shutting down".to_string());
        }
        self.ensure_plugin_migration_ready(plugin_id).await?;
        self.enable_inner(plugin_id).await
    }

    /// Reserved for the host migration coordinator. This is intentionally not
    /// exposed through the generic Tauri plugin lifecycle API.
    pub async fn enable_jira_after_migration(&self) -> Result<PluginInventory, String> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("plugin runtime is shutting down".to_string());
        }
        self.enable_inner(JIRA_PLUGIN_ID).await
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
            Err(error) => return self.fail_startup(&id, error).await,
        };

        // Bundled plugins own durable state too; Jira settings and credentials
        // must survive restarts under the same plugin namespace as local plugins.
        let state_path = Some(plugin_state_root(&self.app, &id).await?);
        let process = match spawn_runtime(
            self.app.clone(),
            &binary,
            &log_path,
            state_path.as_deref(),
            &id,
            capabilities.clone(),
            self.provider_sessions.clone(),
        )
        .await
        {
            Ok(process) => Arc::new(process),
            Err(error) => return self.fail_startup(&id, error).await,
        };
        if let Err(error) = self.initialize_runtime(&inventory, &process).await {
            let _ = stop_process(process).await;
            return self.fail_startup(&id, error).await;
        }

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
        self.start_background_worker(&inventory, process.clone())
            .await;
        self.monitor_process(id.clone(), process);
        self.inventory(&id)
            .await?
            .ok_or_else(|| format!("plugin inventory entry not found after startup: {id}"))
    }

    /// Records a runtime that failed to start, returning why.
    async fn fail_startup<T>(&self, plugin_id: &str, error: String) -> Result<T, String> {
        self.update_state(
            plugin_id,
            true,
            PluginRuntimeState::Error,
            Some(error.clone()),
        )
        .await?;
        Err(error)
    }

    /// Handshakes with a spawned runtime and, for a provider plugin, reconciles its sessions,
    /// all before the runtime is published, so no start or resume can precede them.
    async fn initialize_runtime(
        &self,
        inventory: &PluginInventory,
        process: &RuntimeProcess,
    ) -> Result<(), String> {
        let value = process
            .request(
                "plugin.handshake",
                serde_json::json!({
                    "host_api_version": inventory.host_api_version,
                    "host_capabilities": process.capabilities,
                    "host_features": planeai_plugin_contract::provider::HOST_FEATURES,
                }),
            )
            .await?;
        let handshake = serde_json::from_value::<PluginHandshake>(value)
            .map_err(|e| format!("invalid plugin handshake: {e}"))?;
        if handshake.plugin_id != inventory.id
            || handshake.plugin_name != inventory.name
            || handshake.plugin_version != inventory.version
            || handshake.host_api_version != inventory.host_api_version
        {
            return Err(
                "plugin handshake identity or host API version did not match manifest".to_string(),
            );
        }
        {
            let mut subscriptions = process.lifecycle_event_subscriptions.lock().await;
            *subscriptions = handshake.lifecycle_event_subscriptions;
            subscriptions.retain(|subscription| {
                (subscription == "task.lifecycle"
                    && process
                        .capabilities
                        .contains(&PluginHostCapability::TaskEvents))
                    || (subscription == "session.lifecycle"
                        && process
                            .capabilities
                            .contains(&PluginHostCapability::SessionEvents))
            });
        }
        tracing::info!(plugin_id = %handshake.plugin_id, version = %handshake.plugin_version, "plugin runtime handshake completed");
        if process
            .capabilities
            .contains(&PluginHostCapability::Providers)
        {
            let reconciled = self
                .reconcile_provider_sessions(&inventory.id, process)
                .await;
            startup_reconciliation(&inventory.id, reconciled)?;
        }
        Ok(())
    }

    /// Tells a freshly started provider plugin which of its sessions still exist, so it can
    /// drop what it keeps for the others.
    async fn reconcile_provider_sessions(
        &self,
        plugin_id: &str,
        process: &RuntimeProcess,
    ) -> Result<(), PluginRpcError> {
        let owner = plugin_id.to_string();
        let sessions = self
            .with_db(move |conn| {
                crate::db::list_plugin_provider_sessions(conn, &owner)
                    .map_err(|error| error.to_string())
            })
            .await?;
        let sessions = sessions
            .into_iter()
            .map(|(session_id, provider_id, status)| {
                serde_json::json!({
                    "session_id": session_id,
                    "provider_id": provider_id,
                    "status": status,
                })
            })
            .collect::<Vec<_>>();
        process
            .request(
                planeai_plugin_contract::provider::RECONCILE,
                serde_json::json!({ "sessions": sessions }),
            )
            .await
            .map(|_| ())
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
        self.stop_background_worker(plugin_id).await;
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
        self.ensure_plugin_migration_ready(plugin_id).await?;
        self.disable_inner(plugin_id).await?;
        self.enable_inner(plugin_id).await
    }

    async fn wake_background_worker(&self, plugin_id: &str) {
        if let Some(worker) = self.background_workers.lock().await.get(plugin_id) {
            worker.wake.notify_one();
        }
    }

    async fn stop_background_worker(&self, plugin_id: &str) {
        let worker = self.background_workers.lock().await.remove(plugin_id);
        if let Some(worker) = worker {
            tracing::debug!(
                plugin_id,
                generation = worker.generation,
                "cancelling plugin background worker"
            );
            worker.cancel.cancel();
            let _ = worker.task.await;
        }
    }

    async fn start_background_worker(
        &self,
        plugin: &PluginInventory,
        process: Arc<RuntimeProcess>,
    ) {
        let Some(service) = plugin.background_service.clone() else {
            return;
        };
        let plugin_id = plugin.id.clone();
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.stop_background_worker(&plugin_id).await;
        let cancel = CancellationToken::new();
        let worker_cancel = cancel.clone();
        let wake = Arc::new(Notify::new());
        let worker_wake = wake.clone();
        let app = self.app.clone();
        let data_dir = process.data_dir.clone();
        let worker_plugin_id = plugin_id.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = worker_cancel.cancelled() => return,
                    outcome = process.request(&service.method, Value::Null) => match outcome {
                        Ok(totals) => {
                            tracing::info!(plugin_id = %plugin_id, generation, method = %service.method, totals = ?totals, "plugin background service complete");
                            let event = serde_json::json!({
                                "plugin_id": plugin_id,
                                "generation": generation,
                                "method": service.method,
                                "totals": totals,
                            });
                            if let Err(error) = app.emit("plugin-background-service-complete", event.clone()) {
                                tracing::warn!(plugin_id = %plugin_id, %error, "failed to emit plugin service completion event");
                            }
                            if event["totals"]["departed"].as_u64().unwrap_or_default() > 0 {
                                if let Err(error) = app.emit("plugin-background-service-departed", event) {
                                    tracing::warn!(plugin_id = %plugin_id, %error, "failed to emit plugin departure event");
                                }
                            }
                            if let Err(error) = app.emit("plugin-data-changed", plugin_id.clone()) {
                                tracing::warn!(plugin_id = %plugin_id, %error, "failed to emit plugin refresh event");
                            }
                        }
                        Err(error) => tracing::warn!(plugin_id = %plugin_id, generation, method = %service.method, %error, "plugin background service failed"),
                    }
                }
                let interval = match read_plugin_settings(&data_dir).ok().and_then(|settings| {
                    settings
                        .get(&service.interval_setting)
                        .and_then(Value::as_u64)
                }) {
                    Some(value) if value > 0 => value,
                    _ => service.default_interval_ms,
                };
                tokio::select! {
                    _ = worker_cancel.cancelled() => return,
                    _ = worker_wake.notified() => {},
                    _ = tokio::time::sleep(StdDuration::from_millis(interval)) => {}
                }
            }
        });
        self.background_workers.lock().await.insert(
            worker_plugin_id,
            BackgroundWorker {
                generation,
                cancel,
                wake,
                task,
            },
        );
    }

    fn monitor_process(&self, plugin_id: String, process: Arc<RuntimeProcess>) {
        let db = self.db.clone();
        let app = self.app.clone();
        let processes = self.processes.clone();
        let background_workers = self.background_workers.clone();
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
                if let Some(worker) = background_workers.lock().await.remove(&plugin_id) {
                    worker.cancel.cancel();
                    worker.task.abort();
                }
                process.release_provider_sessions();
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

/// Build the PATH handed to plugin backend processes.
///
/// A GUI launch (Spotlight, Finder, Dock) inherits launchd's minimal PATH, which
/// excludes user-local bin directories. Plugin backends routinely shell out to
/// user-installed CLIs (`planeai-plugin-kiro-usage` runs `kiro-cli`), so without
/// this they fail with `No such file or directory (os error 2)`. Applies the same
/// augmentation every other planeai subprocess gets.
fn plugin_runtime_path(extra_path_dirs: &[String]) -> String {
    planeai_core::command::augmented_path(extra_path_dirs)
}

/// Read the user's configured extra PATH directories.
///
/// Falls back to an empty list so a plugin still starts with the conventional
/// directories on its PATH. That fallback is warned about rather than silent: it
/// leaves a plugin unable to find CLIs installed outside the conventional
/// directories, which is the failure this PATH handling exists to prevent.
pub(crate) fn configured_extra_path_dirs(app: &AppHandle) -> Vec<String> {
    let Some(config_state) = app.try_state::<crate::state::ConfigState>() else {
        tracing::warn!("config state unavailable; plugin PATH omits extra_path_dirs");
        return Vec::new();
    };
    // Bound to a local so the lock guard drops before `config_state` does.
    let dirs = match config_state.0.lock() {
        Ok(config) => config.resolved_extra_path_dirs(),
        Err(_) => {
            tracing::warn!("config lock poisoned; plugin PATH omits extra_path_dirs");
            Vec::new()
        }
    };
    dirs
}

fn build_runtime_command(binary: &Path, state_root: Option<&Path>, path_env: &str) -> Command {
    let mut command = Command::new(binary);
    if let Some(state_root) = state_root {
        command
            .env("PLANEAI_PLUGIN_DATA_DIR", state_root.join("data"))
            .env("PLANEAI_PLUGIN_SECRETS_DIR", state_root.join("secrets"));
    }
    command
        .env("PATH", path_env)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    planeai_core::command::no_window_tokio(&mut command);
    command
}

async fn spawn_runtime(
    app: AppHandle,
    binary: &Path,
    log_path: &Path,
    state_root: Option<&Path>,
    plugin_id: &str,
    capabilities: HashSet<PluginHostCapability>,
    provider_sessions: Arc<crate::plugin_providers::ProviderSessions>,
) -> Result<RuntimeProcess, String> {
    let path_env = plugin_runtime_path(&configured_extra_path_dirs(&app));
    let mut command = build_runtime_command(binary, state_root, &path_env);
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
    let (frame_sender, frames) = mpsc::channel(FRAME_QUEUE_CAPACITY);
    let instance = NEXT_RUNTIME_INSTANCE.fetch_add(1, Ordering::Relaxed);
    let provider_sessions = capabilities
        .contains(&PluginHostCapability::Providers)
        .then_some(provider_sessions);
    let notification_sink = provider_sessions.clone().map(|sessions| {
        crate::plugin_providers::NotificationSink::new(app.clone(), plugin_id, instance, sessions)
    });
    let stdout_failure = Arc::new(std::sync::OnceLock::new());
    tokio::spawn(read_stdout_frames(
        BufReader::new(stdout),
        frame_sender,
        plugin_id.to_string(),
        notification_sink,
        stdout_failure.clone(),
    ));
    if let Some(stderr) = child.stderr.take() {
        drain_stderr(stderr, log_path.to_path_buf());
    }
    let data_dir = state_root
        .map(|root| root.join("data"))
        .ok_or("plugin runtime state root was not provided")?;
    Ok(RuntimeProcess {
        app,
        instance,
        child: AsyncMutex::new(child),
        stdin: AsyncMutex::new(stdin),
        frames: AsyncMutex::new(frames),
        request_lock: AsyncMutex::new(()),
        next_request_id: AtomicU64::new(0),
        plugin_id: plugin_id.to_string(),
        data_dir,
        capabilities,
        lifecycle_event_subscriptions: AsyncMutex::new(HashSet::new()),
        session_actions: AsyncMutex::new(Vec::new()),
        provider_sessions,
        stdout_failure,
    })
}

/// Owns sidecar stdout for the process lifetime so notifications are handled as they
/// arrive, not only while a host request is in flight: provider notifications are routed,
/// and any other `host.*` notification, as one from a newer host's contract, is dropped.
/// Every other frame is queued for the request path in arrival order; the first transport
/// error ends the stream and is recorded in `failure`, so the runtime is retired without
/// waiting for a request to see it.
async fn read_stdout_frames(
    mut stdout: BufReader<ChildStdout>,
    frames: mpsc::Sender<Result<String, String>>,
    plugin_id: String,
    notifications: Option<crate::plugin_providers::NotificationSink>,
    failure: Arc<std::sync::OnceLock<String>>,
) {
    loop {
        let frame = read_stdout_frame(&mut stdout).await;
        let handles = |method: &str| {
            notifications
                .as_ref()
                .is_some_and(|sink| sink.handles(method))
        };
        match frame.as_deref().map_or(StdoutFrame::Queue, |frame| {
            classify_stdout_frame(frame, handles)
        }) {
            StdoutFrame::Route(method, params) => {
                if let Some(sink) = &notifications {
                    sink.route(&method, params);
                }
                continue;
            }
            StdoutFrame::Drop(method) => {
                tracing::warn!(%plugin_id, %method, "ignored a host notification this host does not know");
                continue;
            }
            StdoutFrame::Queue => {}
        }
        if let Err(error) = &frame {
            let _ = failure.set(error.clone());
        }
        let failed = frame.is_err();
        if frames.send(frame).await.is_err() || failed {
            return;
        }
    }
}

async fn read_stdout_frame(stdout: &mut BufReader<ChildStdout>) -> Result<String, String> {
    let mut bytes = Vec::new();
    let bytes_read = stdout
        .take(MAX_RPC_FRAME_BYTES + 1)
        .read_until(b'\n', &mut bytes)
        .await
        .map_err(|e| format!("failed to read plugin JSON-RPC response: {e}"))?;
    if bytes_read == 0 {
        return Err(STDOUT_CLOSED_ERROR.to_string());
    }
    validate_plugin_response_frame(&bytes)?;
    String::from_utf8(bytes)
        .map_err(|e| format!("plugin JSON-RPC response was not valid UTF-8: {e}"))
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
    use planeai_plugin_contract::HOST_API_VERSION;

    fn database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        sync_inventory(&conn, &bundled_manifests().unwrap()).unwrap();
        conn
    }

    #[test]
    fn legacy_jira_issue_match_requires_matching_local_and_remote_summary() {
        let conn = database();
        conn.execute_batch("CREATE TABLE tasks (key TEXT PRIMARY KEY, title TEXT NOT NULL);")
            .unwrap();
        crate::jira_migration::migrate_legacy_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO tasks (key, title) VALUES ('DT-8833', 'Legacy Jira summary');
             INSERT INTO jira_issues (issue_key, jira_project, summary, status, last_synced_at)
                 VALUES ('DT-8833', 'DT', 'Legacy Jira summary', 'Accepted', 'now');",
        )
        .unwrap();

        assert!(legacy_jira_issue_matches(&conn, "DT-8833", "Legacy Jira summary").unwrap());
        assert!(!legacy_jira_issue_matches(&conn, "DT-8833", "Different Jira summary").unwrap());
        assert!(!legacy_jira_issue_matches(&conn, "OTHER-1", "Legacy Jira summary").unwrap());
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

        let inventory = get_inventory(&conn, "legacy").unwrap().unwrap();
        assert!(inventory.background_service.is_none());
    }

    fn session_panel_manifest() -> PluginManifest {
        serde_json::from_value(serde_json::json!({
            "schema": "planeai.plugin.v1",
            "id": "panel-test",
            "name": "Panel test",
            "version": "1.0.0",
            "host_api_version": HOST_API_VERSION,
            "source_kind": "local",
            "backend_entrypoints": { crate::plugin_packages::current_platform_key(): "bin/plugin" },
            "ui_contributions": [
                { "id": "details", "label": "Details", "placement": "session.panel", "entrypoint": "ui/details.js" },
                { "id": "dashboard", "label": "Dashboard", "placement": "main-pane", "entrypoint": "ui/dashboard.js" }
            ]
        }))
        .unwrap()
    }

    fn placements(conn: &Connection, plugin_id: &str) -> Vec<PluginUiPlacement> {
        get_inventory(conn, plugin_id)
            .unwrap()
            .unwrap()
            .ui_contributions
            .into_iter()
            .map(|contribution| contribution.placement)
            .collect()
    }

    #[test]
    fn migration_preserves_session_panel_contributions() {
        let conn = database();
        let package = tempfile::TempDir::new().unwrap();
        insert_local_inventory(
            &conn,
            &session_panel_manifest(),
            "bin/plugin",
            "hash",
            package.path(),
            "/source",
        )
        .unwrap();

        migrate(&conn).unwrap();

        assert_eq!(
            placements(&conn, "panel-test"),
            [PluginUiPlacement::SessionPanel, PluginUiPlacement::MainPane]
        );
    }

    #[test]
    fn restores_session_panel_placements_from_the_installed_package_manifest() {
        let conn = database();
        let package = tempfile::TempDir::new().unwrap();
        std::fs::write(
            package.path().join(crate::plugin_packages::MANIFEST_FILE),
            r#"{"schema":"planeai.plugin.v1","id":"panel-test","name":"Panel test","version":"1.0.0","host_api_version":"planeai.plugin-host.v1","source_kind":"local","backend_entrypoints":{},"ui_contributions":[{"id":"details","label":"Details","placement":"session.panel","entrypoint":"ui/details.js"},{"id":"dashboard","label":"Dashboard","placement":"main-pane","entrypoint":"ui/dashboard.js"}]}"#,
        )
        .unwrap();
        insert_local_inventory(
            &conn,
            &session_panel_manifest(),
            "bin/plugin",
            "hash",
            package.path(),
            "/source",
        )
        .unwrap();
        conn.execute(
            "UPDATE plugin_inventory SET ui_contributions = replace(ui_contributions, '\"session.panel\"', '\"main-pane\"') WHERE id = 'panel-test'",
            [],
        )
        .unwrap();
        assert_eq!(
            placements(&conn, "panel-test"),
            [PluginUiPlacement::MainPane, PluginUiPlacement::MainPane]
        );

        assert_eq!(restore_session_panel_placements(&conn).unwrap(), 1);
        assert_eq!(
            placements(&conn, "panel-test"),
            [PluginUiPlacement::SessionPanel, PluginUiPlacement::MainPane]
        );
        assert_eq!(restore_session_panel_placements(&conn).unwrap(), 0);
    }

    #[test]
    fn session_panel_restoration_skips_missing_or_unreadable_package_manifests() {
        let conn = database();
        let package = tempfile::TempDir::new().unwrap();
        insert_local_inventory(
            &conn,
            &session_panel_manifest(),
            "bin/plugin",
            "hash",
            package.path(),
            "/source",
        )
        .unwrap();

        assert_eq!(restore_session_panel_placements(&conn).unwrap(), 0);

        std::fs::write(
            package.path().join(crate::plugin_packages::MANIFEST_FILE),
            "not json",
        )
        .unwrap();
        assert_eq!(restore_session_panel_placements(&conn).unwrap(), 0);
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
        assert_eq!(
            jira.background_service
                .as_ref()
                .map(|service| service.method.as_str()),
            Some("jira.syncNow")
        );
        assert!(jira
            .ui_contributions
            .iter()
            .any(|contribution| contribution.placement == PluginUiPlacement::Interaction));
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
            background_service: None,
            providers: Vec::new(),
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

        let mut titlebar = manifest.clone();
        titlebar.ui_contributions[0].placement = PluginUiPlacement::Titlebar;
        titlebar.ui_contributions[0].order = None;
        assert!(titlebar.validate().is_ok());

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
            background_service: None,
            providers: Vec::new(),
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
    fn replacing_a_local_plugin_updates_its_package_metadata_without_deleting_inventory() {
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
            background_service: None,
            providers: Vec::new(),
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
        let replacement = replace_local_inventory(
            &conn,
            &manifest,
            "bin/plugin",
            "different-hash",
            package.path(),
            "/different/package",
        )
        .unwrap();
        assert_eq!(
            replacement.installed_hash.as_deref(),
            Some("different-hash")
        );
        assert_eq!(
            replacement.original_display_path.as_deref(),
            Some("/different/package")
        );
        assert!(!replacement.enabled);
        assert_eq!(replacement.state, PluginRuntimeState::Disabled);
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
    fn providers_ignore_what_this_host_does_not_know() {
        let provider: PluginProvider = serde_json::from_value(serde_json::json!({
            "id": "chat",
            "label": "Chat",
            "entrypoint": "ui/chat.js",
            "icon": "ui/icon.svg",
            "supports": ["teleport", "handoff"],
        }))
        .unwrap();
        assert_eq!(provider.supports, [ProviderFeature::Handoff]);
    }

    #[test]
    fn jira_sync_and_lifecycle_use_extended_rpc_timeouts() {
        assert_eq!(request_timeout("jira.syncNow"), JIRA_SYNC_RPC_TIMEOUT);
        assert_eq!(
            request_timeout("plugin.taskLifecycle"),
            JIRA_LIFECYCLE_RPC_TIMEOUT
        );
        assert_eq!(request_timeout("jira.status"), RPC_TIMEOUT);
        assert_eq!(
            request_timeout("provider.session.start"),
            Duration::from_secs(30)
        );
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
            background_service: None,
            providers: Vec::new(),
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
            background_service: None,
            providers: Vec::new(),
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
        let session_subscriptions = HashSet::from(["session.lifecycle".to_string()]);
        assert!(session_lifecycle_subscription_is_granted(
            &HashSet::from([PluginHostCapability::SessionEvents]),
            &session_subscriptions
        ));
        assert!(!session_lifecycle_subscription_is_granted(
            &HashSet::new(),
            &session_subscriptions
        ));
        manifest
            .capabilities
            .push(PluginHostCapability::TasksCreate);
        assert!(manifest.validate().is_ok());
        manifest
            .capabilities
            .push(PluginHostCapability::TasksUpdate);
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

    #[test]
    fn a_bad_callback_payload_is_the_plugins_mistake_not_a_broken_runtime() {
        let error = callback_params(
            serde_json::json!({ "actions": "not a list" }),
            "plugin session actions",
            validate_plugin_session_actions,
        )
        .unwrap_err();
        assert!(
            matches!(error, CallbackError::InvalidParams(message) if message.starts_with("invalid plugin session actions"))
        );
        let duplicate = serde_json::json!({ "actions": [
            { "id": "a", "label": "One" },
            { "id": "a", "label": "Two" },
        ] });
        assert!(matches!(
            callback_params(
                duplicate,
                "plugin session actions",
                validate_plugin_session_actions
            ),
            Err(CallbackError::InvalidParams(_))
        ));
    }

    #[test]
    fn only_a_broken_runtime_fails_a_start_over_reconciliation() {
        assert!(startup_reconciliation("chat", Ok(())).is_ok());
        let refused = PluginRpcError::Rpc {
            code: -32601,
            message: "method not found".into(),
        };
        assert!(startup_reconciliation("chat", Err(refused)).is_ok());
        let broken =
            PluginRpcError::Broken("plugin RPC provider.sessions.reconcile timed out".into());
        assert_eq!(
            startup_reconciliation("chat", Err(broken)).unwrap_err(),
            "plugin RPC provider.sessions.reconcile timed out"
        );
    }

    #[tokio::test]
    async fn host_tasks_answer_bad_params_as_invalid_params() {
        let data = tempfile::TempDir::new().unwrap();
        let settings = HashSet::from([PluginHostCapability::Settings]);
        let tasks = HashSet::from([
            PluginHostCapability::TasksCreate,
            PluginHostCapability::TasksUpdate,
            PluginHostCapability::TasksTransition,
        ]);
        let run = |capabilities: &HashSet<PluginHostCapability>,
                   plugin: &'static str,
                   method: &'static str,
                   params: Value| {
            let capabilities = capabilities.clone();
            let data = data.path().to_path_buf();
            async move { execute_host_task(plugin, &capabilities, &data, method, params).await }
        };
        for (capabilities, plugin, method, params) in [
            (
                &settings,
                "local-test",
                "host.settings.replace",
                serde_json::json!({ "settings": 3 }),
            ),
            (
                &settings,
                "local-test",
                "host.settings.patch",
                serde_json::json!({ "patch": [] }),
            ),
            (
                &settings,
                "local-test",
                "host.settings.get",
                serde_json::json!({ "path": [] }),
            ),
            (
                &tasks,
                JIRA_PLUGIN_ID,
                "host.task.create",
                serde_json::json!({}),
            ),
            (
                &tasks,
                JIRA_PLUGIN_ID,
                "host.task.create",
                serde_json::json!({ "title": "t", "tags": 1 }),
            ),
            (
                &tasks,
                JIRA_PLUGIN_ID,
                "host.task.update",
                serde_json::json!({ "status": "x" }),
            ),
            (
                &tasks,
                "local-test",
                "host.sessions.transitionLinkedTask",
                serde_json::json!({ "session_id": "s" }),
            ),
        ] {
            let error = run(capabilities, plugin, method, params.clone())
                .await
                .unwrap_err();
            assert!(
                matches!(error, CallbackError::InvalidParams(_)),
                "{method} {params}: {error:?}"
            );
        }
        assert!(PluginTaskRequest::from_params(
            &serde_json::json!({ "projectPath": "/p", "parentKey": "K-1", "operationId": "o" })
        )
        .unwrap_err()
        .contains("title"));
        let top_level =
            serde_json::json!({ "project_path": "/p", "operation_id": "o", "title": "t" });
        let error = run(
            &tasks,
            "routines",
            "host.tasks.createChild",
            top_level.clone(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            CallbackError::InvalidParams("child task create requires parent_key".into())
        );
        let mut with_parent = top_level;
        with_parent["parent_key"] = "K-1".into();
        let error = run(&tasks, "routines", "host.tasks.create", with_parent)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            CallbackError::InvalidParams(
                "host.tasks.create creates a top-level task; use host.tasks.createChild for a child"
                    .into()
            )
        );
    }

    #[tokio::test]
    async fn top_level_task_create_requires_the_tasks_create_capability() {
        let data = tempfile::TempDir::new().unwrap();
        let error = execute_host_task(
            "routines",
            &HashSet::from([PluginHostCapability::Settings]),
            data.path(),
            "host.tasks.create",
            serde_json::json!({ "project_path": "/p", "operation_id": "o", "title": "t" }),
        )
        .await
        .unwrap_err();
        assert_eq!(error, CallbackError::NotGranted);
    }

    fn task_database() -> (Connection, tempfile::TempDir, crate::db::Project) {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        let checkout = tempfile::TempDir::new().unwrap();
        let path = checkout.path().to_string_lossy().to_string();
        let project = crate::db::create_project(&conn, "Demo", &path).unwrap();
        (conn, checkout, project)
    }

    fn top_level_request(project_path: &str, operation_id: &str) -> PluginTaskRequest {
        PluginTaskRequest::from_params(&serde_json::json!({
            "project_path": project_path,
            "operation_id": operation_id,
            "title": "Weekly retro 2026-10-05",
            "description": "Notes for Monday",
        }))
        .unwrap()
    }

    #[test]
    fn a_top_level_plugin_task_is_a_todo_task_in_the_project() {
        let (mut conn, _checkout, project) = task_database();
        let created = insert_plugin_task(
            &mut conn,
            "routines",
            top_level_request(&project.path, "routine:r1:2026-10-05T13:00:00Z"),
        )
        .unwrap();

        let key = format!("{}-1", project.prefix);
        assert!(created.lifecycle.is_none());
        assert_eq!(
            created.task,
            serde_json::json!({ "task": {
                "key": key,
                "title": "Weekly retro 2026-10-05",
                "description": "Notes for Monday",
                "status": "todo",
                "priority": 0,
                "parent_key": null,
                "url": null,
                "base_branch": "main",
                "blocked_by": [],
                "tags": [],
            } })
        );
        let stored: (String, Option<String>) = conn
            .query_row(
                "SELECT project_prefix, parent_key FROM tasks WHERE key = ?1",
                [&key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, (project.prefix.clone(), None));
    }

    #[test]
    fn a_repeated_operation_id_returns_the_same_top_level_task() {
        let (mut conn, _checkout, project) = task_database();
        let operation = "routine:r1:2026-10-05T13:00:00Z";
        let first = insert_plugin_task(
            &mut conn,
            "routines",
            top_level_request(&project.path, operation),
        )
        .unwrap();
        let second = insert_plugin_task(
            &mut conn,
            "routines",
            top_level_request(&project.path, operation),
        )
        .unwrap();
        let other_plugin = insert_plugin_task(
            &mut conn,
            "other",
            top_level_request(&project.path, operation),
        )
        .unwrap();

        let key = format!("{}-1", project.prefix);
        assert_eq!(first.task["task"]["key"], key.as_str());
        assert_eq!(second.task["task"]["key"], key.as_str());
        assert_eq!(
            other_plugin.task["task"]["key"],
            format!("{}-2", project.prefix).as_str()
        );
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn a_top_level_plugin_task_needs_a_visible_known_project() {
        let (mut conn, _checkout, project) = task_database();
        let unknown = insert_plugin_task(&mut conn, "routines", top_level_request("/nowhere", "a"))
            .unwrap_err();
        assert_eq!(unknown, "project was not found or is hidden");

        crate::db::hide_project(&conn, &project.id).unwrap();
        let hidden =
            insert_plugin_task(&mut conn, "routines", top_level_request(&project.path, "b"))
                .unwrap_err();
        assert_eq!(hidden, "project was not found or is hidden");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn a_child_plugin_task_reports_its_first_child_assignment() {
        let (mut conn, _checkout, project) = task_database();
        let parent =
            insert_plugin_task(&mut conn, "routines", top_level_request(&project.path, "p"))
                .unwrap();
        let parent_key = parent.task["task"]["key"].as_str().unwrap().to_string();
        let mut params = serde_json::json!({
            "project_path": project.path,
            "operation_id": "c",
            "title": "Child",
            "parent_key": parent_key,
        });
        let child = insert_plugin_task(
            &mut conn,
            "routines",
            PluginTaskRequest::from_params(&params).unwrap(),
        )
        .unwrap();
        let child_key = format!("{}-2", project.prefix);
        assert_eq!(child.task["task"]["parent_key"], parent_key.as_str());
        assert_eq!(
            child.lifecycle.unwrap().events,
            vec![crate::task_lifecycle::TaskLifecycleEvent::ChildAssigned {
                child_key,
                parent_key: parent_key.clone(),
                is_first_child_assignment: true,
            }]
        );
        params["operation_id"] = "c2".into();
        let second = insert_plugin_task(
            &mut conn,
            "routines",
            PluginTaskRequest::from_params(&params).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            second.lifecycle.unwrap().events.as_slice(),
            [crate::task_lifecycle::TaskLifecycleEvent::ChildAssigned {
                is_first_child_assignment: false,
                ..
            }]
        ));
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
        assert!(
            execute_host_task(
                "local-test",
                &HashSet::new(),
                data.path(),
                "host.settings.get",
                Value::Null
            )
            .await
            .unwrap_err()
                == CallbackError::NotGranted
        );
    }

    #[tokio::test]
    async fn bounded_settings_callbacks_patch_large_documents_without_returning_them() {
        let data = tempfile::TempDir::new().unwrap();
        let capabilities = HashSet::from([PluginHostCapability::Settings]);
        let legacy_mappings = (0..300)
            .map(|index| {
                (
                    format!("session-{index}"),
                    serde_json::json!({ "url": format!("https://github.com/example/repository/pull/{index}"), "note": "x".repeat(256) }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        replace_plugin_settings(
            data.path(),
            serde_json::json!({
                "github": {
                    "version": 1,
                    "pull_requests": legacy_mappings,
                    "reconciliation": { "status": "idle", "checked": 0 },
                },
                "unrelated": { "retain": true },
            }),
        )
        .unwrap();

        let whole_document = execute_host_task(
            "github",
            &capabilities,
            data.path(),
            "host.settings.get",
            Value::Null,
        )
        .await
        .unwrap();
        let frame =
            encode_host_callback_response(Value::String("whole".into()), Ok(whole_document))
                .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&frame).unwrap()["error"]["message"],
            "host callback response exceeded the frame limit"
        );

        let reconciliation = execute_host_task(
            "github",
            &capabilities,
            data.path(),
            "host.settings.get",
            serde_json::json!({ "path": ["github", "reconciliation"] }),
        )
        .await
        .unwrap();
        let frame = encode_host_callback_response(
            Value::String("reconciliation".into()),
            Ok(reconciliation.clone()),
        )
        .unwrap();
        assert!(host_callback_response_fits(&frame));
        assert_eq!(reconciliation["settings"]["status"], "idle");

        let patched = execute_host_task(
            "github",
            &capabilities,
            data.path(),
            "host.settings.patch",
            serde_json::json!({
                "patch": {
                    "github": {
                        "pull_requests": {
                            "active-session": { "url": "https://github.com/example/repository/pull/999", "state": "open" }
                        }
                    }
                }
            }),
        )
        .await
        .unwrap();
        let frame =
            encode_host_callback_response(Value::String("patch".into()), Ok(patched)).unwrap();
        assert!(host_callback_response_fits(&frame));
        assert_eq!(
            read_plugin_settings(data.path()).unwrap()["unrelated"]["retain"],
            true
        );
        assert_eq!(
            read_plugin_settings(data.path()).unwrap()["github"]["pull_requests"]["active-session"]
                ["state"],
            "open"
        );
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
    fn oversized_callback_id_is_rejected_before_an_oversized_fallback_is_written() {
        let id = Value::String("x".repeat(MAX_RPC_FRAME_BYTES as usize - 100));
        assert!(encode_host_callback_response(
            id,
            Ok(serde_json::json!({ "large": "x".repeat(MAX_RPC_FRAME_BYTES as usize) }))
        )
        .unwrap_err()
        .contains("leaves no room"));
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
            "plugin.sessionLifecycle",
            "$/cancelRequest",
            "provider.session.start",
            "provider.session.send",
        ] {
            assert!(is_host_controlled_plugin_method(method), "{method}");
        }
        assert!(!is_host_controlled_plugin_method("fixture.status"));
    }

    #[test]
    fn local_plugin_session_callback_payloads_are_strict_and_provider_aware() {
        let actions = validate_plugin_session_actions(SessionActionsRequest {
            actions: vec![
                PluginSessionAction {
                    id: "review".into(),
                    label: "Open review".into(),
                    providers: vec!["claude".into()],
                },
                PluginSessionAction {
                    id: "sync".into(),
                    label: "Sync".into(),
                    providers: vec![],
                },
            ],
        })
        .unwrap();
        assert_eq!(actions[0].providers, vec!["claude"]);
        assert!(validate_plugin_session_actions(SessionActionsRequest {
            actions: vec![
                PluginSessionAction {
                    id: "duplicate".into(),
                    label: "One".into(),
                    providers: vec![]
                },
                PluginSessionAction {
                    id: "duplicate".into(),
                    label: "Two".into(),
                    providers: vec![]
                },
            ],
        })
        .is_err());
        assert!(validate_plugin_session_advisory(PluginSessionAdvisory {
            session_id: "session-1".into(),
            message: "Needs attention".into(),
            severity: PluginSessionAdvisorySeverity::Warning,
        })
        .is_ok());
        assert!(validate_plugin_session_completion(PluginSessionCompletion {
            session_id: "session-1".into(),
            message: Some("Integration completed".into()),
        })
        .is_ok());
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
        assert!(!PluginUiPlacement::SessionIndicator.is_sidebar());
    }
}

#[cfg(test)]
mod session_prompt_tests {
    use super::*;

    #[test]
    fn local_plugins_may_request_the_session_prompt_capability() {
        assert!(validate_capabilities(
            PluginSourceKind::Local,
            &[PluginHostCapability::SessionsPrompt],
        )
        .is_ok());
    }

    #[test]
    fn session_prompt_payload_is_bounded_and_delivered_once() {
        let sent = std::sync::Mutex::new(Vec::new());
        let response = dispatch_plugin_session_prompt(
            &HashSet::from([PluginHostCapability::SessionsPrompt]),
            serde_json::json!({ "session_id": "session-123", "text": "Please fix the CI failure." }),
            |session_id, text| {
                sent.lock()
                    .unwrap()
                    .push((session_id.to_string(), text.to_string()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(response, serde_json::json!({ "delivered": true }));
        assert_eq!(
            sent.into_inner().unwrap(),
            vec![(
                "session-123".to_string(),
                "Please fix the CI failure.".to_string()
            )]
        );

        for invalid in [
            serde_json::json!({ "session_id": "", "text": "hello" }),
            serde_json::json!({ "session_id": "session-123", "text": "   " }),
            serde_json::json!({ "session_id": "session-123", "text": "x".repeat(MAX_PLUGIN_SESSION_PROMPT_CHARS + 1) }),
        ] {
            assert!(dispatch_plugin_session_prompt(
                &HashSet::from([PluginHostCapability::SessionsPrompt]),
                invalid,
                |_, _| Ok(())
            )
            .is_err());
        }
        assert!(
            dispatch_plugin_session_prompt(
                &HashSet::new(),
                serde_json::json!({ "session_id": "session-123", "text": "should not send" }),
                |_, _| panic!("ungranted plugins must not deliver a prompt"),
            )
            .unwrap_err()
                == CallbackError::NotGranted
        );
    }
}

#[cfg(test)]
mod runtime_spawn_tests {
    use super::*;

    fn command_env(command: &Command, key: &str) -> Option<String> {
        command
            .as_std()
            .get_envs()
            .find(|(name, _)| *name == std::ffi::OsStr::new(key))
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().into_owned())
    }

    #[test]
    fn plugin_backend_is_spawned_with_an_explicit_path() {
        let command = build_runtime_command(
            Path::new("/opt/planeai/plugins/kiro-usage/backend"),
            None,
            "/custom/bin:/usr/bin",
        );
        assert_eq!(
            command_env(&command, "PATH").as_deref(),
            Some("/custom/bin:/usr/bin"),
            "plugin backends must not inherit the GUI launch PATH"
        );
    }

    #[test]
    fn plugin_backend_keeps_its_state_directories() {
        let state_root = Path::new("/state/kiro-usage");
        let command = build_runtime_command(
            Path::new("/opt/planeai/plugins/kiro-usage/backend"),
            Some(state_root),
            "/usr/bin",
        );
        // Compared as paths, not strings: separators differ across platforms.
        assert_eq!(
            command_env(&command, "PLANEAI_PLUGIN_DATA_DIR").map(PathBuf::from),
            Some(state_root.join("data"))
        );
        assert_eq!(
            command_env(&command, "PLANEAI_PLUGIN_SECRETS_DIR").map(PathBuf::from),
            Some(state_root.join("secrets"))
        );
    }

    /// Reproduces the Spotlight launch failure: launchd hands the app a minimal
    /// PATH, so a plugin backend that shells out to a user-installed CLI fails
    /// with `No such file or directory (os error 2)`. The PATH planeai hands to
    /// plugin backends must resolve that CLI.
    #[cfg(unix)]
    #[test]
    fn plugin_runtime_path_resolves_binaries_a_gui_launch_path_would_miss() {
        let user_bin = tempfile::tempdir().unwrap();
        // A symlink, not a freshly written script: a concurrent test's fork can inherit the
        // script's write handle, making exec fail with "Text file busy".
        std::os::unix::fs::symlink("/bin/echo", user_bin.path().join("planeai-path-probe"))
            .unwrap();

        let extra_path_dirs = vec![user_bin.path().to_string_lossy().into_owned()];
        let gui_launch_path = "/usr/bin:/bin:/usr/sbin:/sbin";

        let spawn_probe = |path: &str| {
            std::process::Command::new("/bin/sh")
                .args(["-c", "planeai-path-probe"])
                .env_clear()
                .env("PATH", path)
                .status()
                .unwrap()
                .success()
        };

        assert!(
            !spawn_probe(gui_launch_path),
            "the bug: a minimal GUI launch PATH cannot resolve user-installed CLIs"
        );
        assert!(
            spawn_probe(&plugin_runtime_path(&extra_path_dirs)),
            "the fix: the plugin runtime PATH must resolve user-installed CLIs"
        );
    }
}
