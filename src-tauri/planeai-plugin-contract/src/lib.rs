use std::collections::HashSet;
use std::path::{Component, Path};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// What a declared provider can do beyond running sessions.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFeature {
    /// Honors PlaneAI's auto-approve.
    AutoApprove,
    /// Can continue a session in the agent's own terminal UI.
    Handoff,
}

/// The features this host knows, in declared order without duplicates. Features from newer
/// hosts are skipped, so a plugin declaring one still loads here.
pub fn known_provider_features(values: &[Value]) -> Vec<ProviderFeature> {
    let mut features = Vec::new();
    for feature in values
        .iter()
        .filter_map(|value| ProviderFeature::deserialize(value).ok())
    {
        if !features.contains(&feature) {
            features.push(feature);
        }
    }
    features
}

/// Serde adapter reading `supports` with [`known_provider_features`].
pub fn deserialize_provider_features<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<ProviderFeature>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values = Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(known_provider_features(&values))
}

pub mod provider;

pub const HOST_API_VERSION: &str = "planeai.plugin-host.v1";
pub const RECIPIENT_HOST_API_VERSION: &str = "planeai.plugin-host.v2";
/// Unstable until plugin-provided session runtimes ship; see ADR-0014.
pub const PROVIDER_HOST_API_VERSION: &str = "planeai.plugin-host.v3";

/// Plugin-created tasks that start sessions (`sessions.start`, `host.tasks.create`,
/// `host.sessions.providers`) and the `dialog` placement.
pub const TASK_SESSION_HOST_API_VERSION: &str = "planeai.plugin-host.v4";

/// A `session.meta` slot's tooltip and click-through to the plugin's session panel; on older
/// host APIs the slot stays visual-only. The frontend enforces it in `src/lib/plugin-navigation.ts`.
pub const SESSION_META_HOST_API_VERSION: &str = "planeai.plugin-host.v5";

/// MCP tools a plugin contributes to agents in sessions (`tools[]`, the `tools` capability
/// and `tool.call`); see ADR-0017.
pub const TOOLS_HOST_API_VERSION: &str = "planeai.plugin-host.v6";

/// Every host API this host serves, oldest first; each includes the ones before it.
const HOST_API_VERSIONS: [&str; 6] = [
    HOST_API_VERSION,
    RECIPIENT_HOST_API_VERSION,
    PROVIDER_HOST_API_VERSION,
    TASK_SESSION_HOST_API_VERSION,
    SESSION_META_HOST_API_VERSION,
    TOOLS_HOST_API_VERSION,
];

/// The host-to-plugin request that runs one of the plugin's tools.
pub const TOOL_CALL: &str = "tool.call";
pub const DEFAULT_TOOL_TIMEOUT_MS: u64 = 30_000;
pub const MAX_TOOL_TIMEOUT_MS: u64 = 120_000;

/// How long the host waits for a tool declaring `timeoutMs`.
pub fn tool_timeout(timeout_ms: Option<u64>) -> std::time::Duration {
    std::time::Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_TOOL_TIMEOUT_MS))
}

/// `tool.call` params: the tool by its manifest name, the agent's arguments, and the session
/// the agent runs in.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ToolCallParams {
    pub tool: String,
    pub arguments: Map<String, Value>,
    pub caller: ToolCaller,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolCaller {
    pub session_id: String,
    pub project: ToolProject,
    /// The session's task, `null` when it has none.
    pub task: Option<ToolTask>,
    /// The session runs without permission prompts.
    pub auto_approve: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolProject {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolTask {
    pub key: String,
    /// The project owning the task, which may differ from the session's.
    pub project: ToolProject,
}

/// A `tool.call` result, what the agent reads.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolCallResult {
    pub text: String,
    #[serde(default)]
    pub is_error: bool,
}

/// What a tool call does, from its MCP annotations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolEffect {
    Read,
    Write { idempotent: bool },
    Delete,
}

/// A declared tool's MCP annotation hints, absent ones taking the MCP defaults.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolAnnotations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

impl ToolAnnotations {
    /// MCP's defaults make a tool that is not read-only destructive unless it says otherwise.
    pub fn effect(&self) -> std::result::Result<ToolEffect, String> {
        match (self.read_only_hint, self.destructive_hint) {
            (Some(true), Some(true)) => Err("cannot be both read-only and destructive".into()),
            (Some(true), _) => Ok(ToolEffect::Read),
            (_, Some(false)) => Ok(ToolEffect::Write {
                idempotent: self.idempotent_hint.unwrap_or(false),
            }),
            _ => Ok(ToolEffect::Delete),
        }
    }

    pub fn open_world(&self) -> bool {
        self.open_world_hint.unwrap_or(true)
    }
}

pub fn supports_host_api_version(version: &str) -> bool {
    HOST_API_VERSIONS.contains(&version)
}

/// Whether a manifest's host API includes the features `required` introduced.
fn includes_host_api(object: &Map<String, Value>, required: &str) -> bool {
    let position = |version| HOST_API_VERSIONS.iter().position(|known| *known == version);
    let declared = object.get("host_api_version").and_then(Value::as_str);
    declared.and_then(position) >= position(required)
}

fn require_host_api(object: &Map<String, Value>, feature: &str, required: &str) -> Result<()> {
    if !includes_host_api(object, required) {
        bail!("{feature} requires host API {required}");
    }
    Ok(())
}

const MANIFEST_FIELDS: &[&str] = &[
    "schema",
    "id",
    "name",
    "version",
    "host_api_version",
    "source_kind",
    "backend_entrypoint",
    "backend_entrypoints",
    "ui_contributions",
    "capabilities",
    "ui_entrypoint",
    "background_service",
    "providers",
    "tools",
    "downloads",
];
/// The `backend_entrypoints` and `downloads` keys PlaneAI resolves for the running host.
pub const PLATFORM_KEYS: &[&str] = &[
    "macos-arm64",
    "macos-x64",
    "linux-arm64",
    "linux-x64",
    "windows-arm64",
    "windows-x64",
];
const TOOL_FIELDS: &[&str] = &[
    "name",
    "description",
    "inputSchema",
    "annotations",
    "timeoutMs",
];
const TOOL_ANNOTATION_FIELDS: &[&str] = &[
    "readOnlyHint",
    "destructiveHint",
    "idempotentHint",
    "openWorldHint",
];
const UI_CONTRIBUTION_FIELDS: &[&str] = &[
    "id",
    "label",
    "placement",
    "entrypoint",
    "order",
    "shortcut",
];
const LOCAL_CAPABILITIES: &[&str] = &[
    "settings",
    "projects.read",
    "sessions.read",
    "sessions.repository-context",
    "sessions.prompt",
    "session-events",
    "sessions.actions",
    "sessions.advisories",
    "sessions.complete",
    "sessions.start",
    "tasks.read",
    "tasks.create",
    "tasks.transition",
    "task-events",
    "providers",
    "tools",
];
const BACKGROUND_SERVICE_FIELDS: &[&str] = &["method", "interval_setting", "default_interval_ms"];
const UI_PLACEMENTS: &[&str] = &[
    "sidebar.header",
    "sidebar.navigation",
    "sidebar.section",
    "sidebar.footer",
    "preferences",
    "main-pane",
    "dialog",
    "session.panel",
    "session.indicator",
    "session.meta",
    "titlebar",
    "interaction",
];

pub fn validate_local_manifest(manifest: &Value, platform: &str) -> Result<String> {
    let object = manifest
        .as_object()
        .ok_or_else(|| anyhow!("plugin manifest must be a JSON object"))?;
    reject_unknown_fields(object, MANIFEST_FIELDS, "plugin manifest")?;
    if required_string(object, "schema")? != "planeai.plugin.v1" {
        bail!("unsupported plugin manifest schema");
    }
    let id = required_string(object, "id")?;
    validate_id(id, "plugin")?;
    for field in ["name", "version"] {
        required_string(object, field)?;
    }
    if !supports_host_api_version(required_string(object, "host_api_version")?) {
        bail!("plugin manifest requires an unsupported host API version");
    }
    if required_string(object, "source_kind")? != "local" {
        bail!("plugin test only accepts local plugin packages (source_kind must be \"local\")");
    }
    optional_string(object, "backend_entrypoint")?;
    if object.contains_key("ui_entrypoint") {
        optional_string(object, "ui_entrypoint")?;
        bail!("local plugin manifests must use ui_contributions instead of legacy ui_entrypoint");
    }
    let entrypoints = object
        .get("backend_entrypoints")
        .and_then(Value::as_object)
        .filter(|values| !values.is_empty())
        .ok_or_else(|| anyhow!("local plugin backend_entrypoints is required"))?;
    for (entrypoint_platform, entrypoint) in entrypoints {
        let entrypoint = entrypoint.as_str().ok_or_else(|| {
            anyhow!("plugin manifest backend entrypoint for {entrypoint_platform} must be a string")
        })?;
        validate_relative_path(entrypoint, "plugin backend entrypoint")?;
    }
    let entrypoint = entrypoints
        .get(platform)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("plugin manifest has no backend entrypoint for {platform}"))?;
    validate_capabilities(object)?;
    validate_background_service(object)?;
    validate_ui_contributions(object, id)?;
    validate_providers(object, id)?;
    validate_tools(object, id)?;
    validate_downloads(object)?;
    Ok(entrypoint.to_owned())
}

/// `downloads` maps a platform key to the https URL of that platform's package archive.
fn validate_downloads(object: &Map<String, Value>) -> Result<()> {
    let Some(value) = object.get("downloads") else {
        return Ok(());
    };
    let downloads = value.as_object().ok_or_else(|| {
        anyhow!("plugin manifest downloads must be an object of platform keys to https URLs")
    })?;
    for (platform, url) in downloads {
        if !PLATFORM_KEYS.contains(&platform.as_str()) {
            bail!("plugin manifest downloads declares an unknown platform: {platform}");
        }
        match url.as_str() {
            Some(url) if url.starts_with("https://") && url.len() > "https://".len() => {}
            Some(url) => {
                bail!("plugin manifest download for {platform} must be an https URL: {url}")
            }
            None => bail!("plugin manifest download for {platform} must be an https URL"),
        }
    }
    Ok(())
}

fn validate_id(id: &str, subject: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!("{subject} id must contain lowercase letters, digits, or hyphens");
    }
    Ok(())
}

fn optional_string<'a>(object: &'a Map<String, Value>, field: &str) -> Result<Option<&'a str>> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => bail!("plugin manifest {field} must be a string"),
    }
}

fn validate_capabilities(object: &Map<String, Value>) -> Result<()> {
    let capabilities = match object.get("capabilities") {
        None => return Ok(()),
        Some(value) => value
            .as_array()
            .ok_or_else(|| anyhow!("plugin manifest capabilities must be an array"))?,
    };
    let mut seen = HashSet::new();
    for capability in capabilities {
        let capability = capability
            .as_str()
            .ok_or_else(|| anyhow!("plugin manifest capabilities must contain strings"))?;
        if !LOCAL_CAPABILITIES.contains(&capability) {
            bail!("local plugins may only request documented local plugin capabilities");
        }
        if !seen.insert(capability) {
            bail!("plugin manifest declares duplicate capabilities");
        }
        if capability == "sessions.start" {
            require_host_api(
                object,
                "the sessions.start capability",
                TASK_SESSION_HOST_API_VERSION,
            )?;
        }
        if capability == "tools" {
            require_host_api(object, "the tools capability", TOOLS_HOST_API_VERSION)?;
        }
    }
    Ok(())
}

fn validate_tools(object: &Map<String, Value>, plugin_id: &str) -> Result<()> {
    let declares_capability = object
        .get("capabilities")
        .and_then(Value::as_array)
        .is_some_and(|capabilities| capabilities.iter().any(|value| value == "tools"));
    let tools = match object.get("tools") {
        None | Some(Value::Null) => &[][..],
        Some(value) => value
            .as_array()
            .ok_or_else(|| anyhow!("plugin manifest tools must be an array"))?,
    };
    match (declares_capability, tools.is_empty()) {
        (true, true) => bail!("the tools capability requires at least one declared tool"),
        (false, false) => bail!("declared tools require the tools capability"),
        _ => {}
    }
    let mut names = HashSet::new();
    for tool in tools {
        let tool = tool
            .as_object()
            .ok_or_else(|| anyhow!("tool must be an object"))?;
        let name = tool.get("name").and_then(Value::as_str).unwrap_or_default();
        if !is_tool_name(name) {
            bail!("tool name must start with a lowercase letter and contain only lowercase letters, digits, or underscores");
        }
        reject_unknown_fields(tool, TOOL_FIELDS, &format!("tool {name}"))?;
        if !names.insert(name) {
            bail!("plugin {plugin_id} defines duplicate tool {name}");
        }
        if tool
            .get("description")
            .and_then(Value::as_str)
            .is_none_or(|description| description.trim().is_empty())
        {
            bail!("tool {name} description must be a nonempty string");
        }
        if tool
            .get("inputSchema")
            .and_then(|schema| schema.get("type"))
            .and_then(Value::as_str)
            != Some("object")
        {
            bail!("tool {name} inputSchema must be a JSON Schema object with \"type\": \"object\"");
        }
        match tool.get("timeoutMs") {
            None => {}
            Some(Value::Number(value))
                if value
                    .as_u64()
                    .is_some_and(|ms| (1..=MAX_TOOL_TIMEOUT_MS).contains(&ms)) => {}
            Some(_) => {
                bail!("tool {name} timeoutMs must be an integer from 1 to {MAX_TOOL_TIMEOUT_MS}")
            }
        }
        if let Some(annotations) = tool.get("annotations") {
            let subject = format!("tool {name} annotations");
            let fields = annotations
                .as_object()
                .ok_or_else(|| anyhow!("{subject} must be an object"))?;
            reject_unknown_fields(fields, TOOL_ANNOTATION_FIELDS, &subject)?;
            if let Some((field, _)) = fields.iter().find(|(_, value)| !value.is_boolean()) {
                bail!("{subject} {field} must be a boolean");
            }
            ToolAnnotations::deserialize(annotations)
                .map_err(|error| anyhow!("{subject}: {error}"))?
                .effect()
                .map_err(|error| anyhow!("tool {name} {error}"))?;
        }
    }
    Ok(())
}

/// A tool's name within its plugin; the host serves it as `<plugin id>_<name>`.
pub fn is_tool_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn validate_background_service(object: &Map<String, Value>) -> Result<()> {
    let Some(value) = object.get("background_service") else {
        return Ok(());
    };
    if value.is_null() {
        return Ok(());
    }
    let service = value
        .as_object()
        .ok_or_else(|| anyhow!("plugin manifest background_service must be an object"))?;
    reject_unknown_fields(service, BACKGROUND_SERVICE_FIELDS, "background service")?;
    required_string(service, "method")?;
    required_string(service, "interval_setting")?;
    match service.get("default_interval_ms") {
        Some(Value::Number(value)) if value.as_u64().is_some_and(|value| value > 0) => Ok(()),
        _ => bail!("background service default_interval_ms must be a positive integer"),
    }
}

fn validate_ui_contributions(object: &Map<String, Value>, plugin_id: &str) -> Result<()> {
    let contributions = match object.get("ui_contributions") {
        None => return Ok(()),
        Some(value) => value
            .as_array()
            .ok_or_else(|| anyhow!("plugin manifest ui_contributions must be an array"))?,
    };
    let mut ids = HashSet::new();
    let mut shortcuts = HashSet::new();
    for contribution in contributions {
        let contribution = contribution
            .as_object()
            .ok_or_else(|| anyhow!("UI contribution must be an object"))?;
        reject_unknown_fields(contribution, UI_CONTRIBUTION_FIELDS, "UI contribution")?;
        let id = required_string(contribution, "id")?;
        validate_id(id, "UI contribution")?;
        if !ids.insert(id) {
            bail!("plugin {plugin_id} defines duplicate UI contribution {id}");
        }
        required_string(contribution, "label")?;
        let entrypoint = required_string(contribution, "entrypoint")?;
        validate_relative_path(entrypoint, "UI contribution entrypoint")?;
        let placement = required_string(contribution, "placement")?;
        if !UI_PLACEMENTS.contains(&placement) {
            bail!("UI contribution placement is unsupported: {placement}");
        }
        if placement == "dialog" {
            require_host_api(
                object,
                "the dialog placement",
                TASK_SESSION_HOST_API_VERSION,
            )?;
        }
        let has_order = match contribution.get("order") {
            None | Some(Value::Null) => false,
            Some(Value::Number(value))
                if value
                    .as_i64()
                    .is_some_and(|value| i32::try_from(value).is_ok()) =>
            {
                true
            }
            Some(_) => bail!("UI contribution order must be a 32-bit integer"),
        };
        let shortcut = match contribution.get("shortcut") {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) => Some(value.as_str()),
            Some(_) => bail!("UI contribution shortcut must be a string"),
        };
        if !matches!(placement, "main-pane" | "dialog" | "session.panel") && shortcut.is_some() {
            bail!("UI contribution shortcuts are only valid for main-pane, dialog, or session-panel contributions");
        }
        if !placement.starts_with("sidebar.") && has_order {
            bail!("UI contribution order is only valid for sidebar contributions");
        }
        if let Some(shortcut) = shortcut {
            validate_shortcut(shortcut)?;
            if !shortcuts.insert(shortcut) {
                bail!("plugin {plugin_id} defines duplicate UI contribution shortcut {shortcut}");
            }
        }
    }
    Ok(())
}

fn validate_providers(object: &Map<String, Value>, plugin_id: &str) -> Result<()> {
    let declares_capability = object
        .get("capabilities")
        .and_then(Value::as_array)
        .is_some_and(|capabilities| capabilities.iter().any(|value| value == "providers"));
    let providers = match object.get("providers") {
        None | Some(Value::Null) => {
            if declares_capability {
                bail!("the providers capability requires at least one declared provider");
            }
            return Ok(());
        }
        Some(value) => value
            .as_array()
            .ok_or_else(|| anyhow!("plugin manifest providers must be an array"))?,
    };
    if providers.is_empty() {
        bail!("plugin manifest providers must not be empty when present");
    }
    if !declares_capability {
        bail!("declared providers require the providers capability");
    }
    if !includes_host_api(object, PROVIDER_HOST_API_VERSION) {
        bail!("declared providers require host API {PROVIDER_HOST_API_VERSION}");
    }
    let mut ids = HashSet::new();
    for provider in providers {
        let provider = provider
            .as_object()
            .ok_or_else(|| anyhow!("provider must be an object"))?;
        // Unknown fields are left for newer hosts.
        let id = required_string(provider, "id")?;
        validate_id(id, "provider")?;
        if !ids.insert(id) {
            bail!("plugin {plugin_id} defines duplicate provider {id}");
        }
        required_string(provider, "label")?;
        validate_relative_path(
            required_string(provider, "entrypoint")?,
            "provider entrypoint",
        )?;
        let features = match provider.get("supports") {
            None | Some(Value::Null) => continue,
            Some(value) => value
                .as_array()
                .ok_or_else(|| anyhow!("provider supports must be an array"))?,
        };
        if features.iter().any(|feature| !feature.is_string()) {
            bail!("provider supports must be an array of strings");
        }
        let mut seen = HashSet::new();
        if !features.iter().all(|feature| seen.insert(feature.as_str())) {
            bail!("provider declares duplicate supported features");
        }
    }
    Ok(())
}

fn validate_relative_path(entrypoint: &str, subject: &str) -> Result<()> {
    let path = Path::new(entrypoint);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || entrypoint.starts_with('\\')
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("{subject} must be a safe package-relative path: {entrypoint}");
    }
    Ok(())
}

fn validate_shortcut(shortcut: &str) -> Result<()> {
    let parts = shortcut.split('+').collect::<Vec<_>>();
    let key = parts.last().copied().unwrap_or_default();
    let modifiers = &parts[1..parts.len().saturating_sub(1)];
    if parts.len() < 2
        || parts.first() != Some(&"Mod")
        || key.len() != 1
        || !key.as_bytes()[0].is_ascii_uppercase()
        || modifiers
            .iter()
            .any(|modifier| !matches!(*modifier, "Shift" | "Alt"))
        || modifiers.windows(2).any(|pair| pair[0] == pair[1])
    {
        bail!("UI contribution shortcut must use Mod+[Shift+][Alt+]A-Z syntax");
    }
    let canonical = match modifiers {
        [] => format!("Mod+{key}"),
        ["Shift"] => format!("Mod+Shift+{key}"),
        ["Alt"] => format!("Mod+Alt+{key}"),
        ["Shift", "Alt"] => format!("Mod+Shift+Alt+{key}"),
        _ => bail!("UI contribution shortcut modifiers must be ordered Shift then Alt"),
    };
    if canonical != shortcut {
        bail!("UI contribution shortcut modifiers must be ordered Shift then Alt");
    }
    Ok(())
}

fn reject_unknown_fields(
    object: &Map<String, Value>,
    allowed: &[&str],
    subject: &str,
) -> Result<()> {
    if let Some(field) = object
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        bail!("{subject} contains undocumented field: {field}");
    }
    Ok(())
}

fn required_string<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("plugin manifest {field} must be a nonempty string"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest() -> Value {
        json!({
            "schema": "planeai.plugin.v1",
            "id": "integration",
            "name": "Integration",
            "version": "0.1.0",
            "host_api_version": SESSION_META_HOST_API_VERSION,
            "source_kind": "local",
            "backend_entrypoints": { "macos-arm64": "bin/plugin" },
            "capabilities": [
                "sessions.repository-context",
                "sessions.prompt",
                "session-events",
                "sessions.actions",
                "sessions.advisories",
                "sessions.complete",
                "sessions.start",
                "tasks.transition"
            ],
            "ui_contributions": [{
                "id": "panel",
                "label": "Integration",
                "placement": "session.panel",
                "entrypoint": "ui/entry.js"
            }]
        })
    }

    #[test]
    fn accepts_session_panel_and_repository_context_capability() {
        assert_eq!(
            validate_local_manifest(&manifest(), "macos-arm64").unwrap(),
            "bin/plugin"
        );
    }

    #[test]
    fn accepts_compact_titlebar_contributions() {
        let mut manifest = manifest();
        manifest["ui_contributions"][0]["placement"] = json!("titlebar");
        assert_eq!(
            validate_local_manifest(&manifest, "macos-arm64").unwrap(),
            "bin/plugin"
        );
    }

    #[test]
    fn accepts_visual_only_session_slots() {
        for placement in ["session.indicator", "session.meta"] {
            let mut manifest = manifest();
            manifest["ui_contributions"][0]["placement"] = json!(placement);
            assert_eq!(
                validate_local_manifest(&manifest, "macos-arm64").unwrap(),
                "bin/plugin",
                "{placement}"
            );
        }
    }

    #[test]
    fn accepts_https_downloads_for_known_platforms() {
        let mut manifest = manifest();
        manifest["downloads"] = json!({
            "macos-arm64": "https://github.com/acme/planeai-plugin-acme/releases/latest/download/planeai-plugin-acme-macos-arm64.tar.gz",
            "windows-x64": "https://cdn.example.com/acme-windows-x64.zip"
        });
        assert_eq!(
            validate_local_manifest(&manifest, "macos-arm64").unwrap(),
            "bin/plugin"
        );
    }

    #[test]
    fn rejects_downloads_for_unknown_platforms_or_non_https_urls() {
        let mut unknown_platform = manifest();
        unknown_platform["downloads"] = json!({ "macos-universal": "https://example.com/a.zip" });
        assert_eq!(
            validate_local_manifest(&unknown_platform, "macos-arm64")
                .unwrap_err()
                .to_string(),
            "plugin manifest downloads declares an unknown platform: macos-universal"
        );

        let mut plain_http = manifest();
        plain_http["downloads"] = json!({ "macos-arm64": "http://example.com/a.tar.gz" });
        assert_eq!(
            validate_local_manifest(&plain_http, "macos-arm64")
                .unwrap_err()
                .to_string(),
            "plugin manifest download for macos-arm64 must be an https URL: http://example.com/a.tar.gz"
        );

        let mut not_an_object = manifest();
        not_an_object["downloads"] = json!(["https://example.com/a.tar.gz"]);
        assert_eq!(
            validate_local_manifest(&not_an_object, "macos-arm64")
                .unwrap_err()
                .to_string(),
            "plugin manifest downloads must be an object of platform keys to https URLs"
        );
    }

    #[test]
    fn session_meta_stays_valid_on_the_host_api_it_shipped_with() {
        let mut manifest = manifest();
        manifest["ui_contributions"][0]["placement"] = json!("session.meta");
        manifest["host_api_version"] = json!(TASK_SESSION_HOST_API_VERSION);
        assert!(validate_local_manifest(&manifest, "macos-arm64").is_ok());
    }

    #[test]
    fn session_slots_reject_shortcuts_and_order() {
        for placement in ["session.indicator", "session.meta"] {
            let mut shortcut_manifest = manifest();
            shortcut_manifest["ui_contributions"][0]["placement"] = json!(placement);
            shortcut_manifest["ui_contributions"][0]["shortcut"] = json!("Mod+Shift+P");
            assert!(validate_local_manifest(&shortcut_manifest, "macos-arm64")
                .unwrap_err()
                .to_string()
                .contains("shortcuts are only valid"));

            let mut ordered_manifest = manifest();
            ordered_manifest["ui_contributions"][0]["placement"] = json!(placement);
            ordered_manifest["ui_contributions"][0]["order"] = json!(1);
            assert!(validate_local_manifest(&ordered_manifest, "macos-arm64")
                .unwrap_err()
                .to_string()
                .contains("order is only valid"));
        }
    }

    fn provider_manifest() -> Value {
        let mut manifest = manifest();
        manifest["host_api_version"] = json!(PROVIDER_HOST_API_VERSION);
        manifest["capabilities"] = json!(["providers"]);
        manifest["providers"] = json!([{
            "id": "chat",
            "label": "Chat",
            "entrypoint": "ui/chat.js",
            "supports": ["auto_approve", "handoff"]
        }]);
        manifest
    }

    fn provider_error(manifest: Value) -> String {
        validate_local_manifest(&manifest, "macos-arm64")
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn accepts_providers_on_host_api_v3() {
        assert_eq!(
            validate_local_manifest(&provider_manifest(), "macos-arm64").unwrap(),
            "bin/plugin"
        );
    }

    #[test]
    fn providers_require_host_api_v3() {
        let mut manifest = provider_manifest();
        manifest["host_api_version"] = json!("planeai.plugin-host.v2");
        assert!(provider_error(manifest).contains("require host API"));
    }

    #[test]
    fn task_session_features_require_host_api_v4() {
        let older = |change: fn(&mut Value)| {
            let mut manifest = manifest();
            change(&mut manifest);
            manifest["host_api_version"] = json!("planeai.plugin-host.v3");
            validate_local_manifest(&manifest, "macos-arm64").map_err(|error| error.to_string())
        };

        assert_eq!(
            older(|_| {}),
            Err("the sessions.start capability requires host API planeai.plugin-host.v4".into())
        );
        assert_eq!(
            older(|manifest| {
                manifest["capabilities"] = json!(["settings"]);
                manifest["ui_contributions"][0]["placement"] = json!("dialog");
            }),
            Err("the dialog placement requires host API planeai.plugin-host.v4".into())
        );
        assert_eq!(
            older(|manifest| manifest["capabilities"] = json!(["settings"])),
            Ok("bin/plugin".into())
        );
    }

    #[test]
    fn providers_stay_available_on_later_host_apis() {
        let mut manifest = provider_manifest();
        manifest["host_api_version"] = json!(TASK_SESSION_HOST_API_VERSION);
        assert_eq!(
            validate_local_manifest(&manifest, "macos-arm64").unwrap(),
            "bin/plugin"
        );
    }

    #[test]
    fn providers_and_capability_must_be_declared_together() {
        let mut without_capability = provider_manifest();
        without_capability["capabilities"] = json!([]);
        assert!(provider_error(without_capability).contains("require the providers capability"));

        let mut without_providers = provider_manifest();
        without_providers
            .as_object_mut()
            .unwrap()
            .remove("providers");
        assert!(provider_error(without_providers).contains("at least one declared provider"));
    }

    #[test]
    fn providers_leave_room_for_newer_hosts() {
        let mut newer = provider_manifest();
        newer["providers"][0]["icon"] = json!("ui/icon.svg");
        newer["providers"][0]["supports"] = json!(["handoff", "teleport"]);
        assert!(validate_local_manifest(&newer, "macos-arm64").is_ok());
        assert_eq!(
            known_provider_features(&[json!("teleport"), json!("handoff"), json!("handoff")]),
            vec![ProviderFeature::Handoff]
        );
    }

    #[test]
    fn providers_are_validated() {
        let mut duplicate = provider_manifest();
        duplicate["providers"] = json!([
            { "id": "chat", "label": "Chat", "entrypoint": "ui/chat.js" },
            { "id": "chat", "label": "Chat 2", "entrypoint": "ui/chat.js" }
        ]);
        assert!(provider_error(duplicate).contains("duplicate provider"));

        let mut unsafe_path = provider_manifest();
        unsafe_path["providers"][0]["entrypoint"] = json!("../chat.js");
        assert!(provider_error(unsafe_path).contains("safe package-relative path"));

        let mut not_a_name = provider_manifest();
        not_a_name["providers"][0]["supports"] = json!([1]);
        assert!(provider_error(not_a_name).contains("array of strings"));

        let mut duplicate_feature = provider_manifest();
        duplicate_feature["providers"][0]["supports"] = json!(["handoff", "handoff"]);
        assert!(provider_error(duplicate_feature).contains("duplicate supported features"));

        let mut empty = provider_manifest();
        empty["providers"] = json!([]);
        assert!(provider_error(empty).contains("must not be empty"));
    }

    fn tools_manifest() -> Value {
        let mut manifest = manifest();
        manifest["host_api_version"] = json!(TOOLS_HOST_API_VERSION);
        manifest["capabilities"] = json!(["tools"]);
        manifest["tools"] = json!([
            {
                "name": "issue_show",
                "description": "Show an issue.",
                "inputSchema": { "type": "object", "properties": { "key": { "type": "string" } } },
                "annotations": { "readOnlyHint": true },
                "timeoutMs": 10000
            },
            {
                "name": "issue_close",
                "description": "Close an issue.",
                "inputSchema": { "type": "object" }
            }
        ]);
        manifest
    }

    fn tools_error(change: impl FnOnce(&mut Value)) -> String {
        let mut manifest = tools_manifest();
        change(&mut manifest);
        validate_local_manifest(&manifest, "macos-arm64")
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn accepts_tools_on_host_api_v6() {
        assert_eq!(
            validate_local_manifest(&tools_manifest(), "macos-arm64").unwrap(),
            "bin/plugin"
        );
    }

    #[test]
    fn tools_require_host_api_v6_and_the_tools_capability() {
        assert_eq!(
            tools_error(
                |manifest| manifest["host_api_version"] = json!(SESSION_META_HOST_API_VERSION)
            ),
            "the tools capability requires host API planeai.plugin-host.v6"
        );
        assert_eq!(
            tools_error(|manifest| manifest["capabilities"] = json!([])),
            "declared tools require the tools capability"
        );
        assert_eq!(
            tools_error(|manifest| {
                manifest.as_object_mut().unwrap().remove("tools");
            }),
            "the tools capability requires at least one declared tool"
        );
        assert_eq!(
            tools_error(|manifest| manifest["tools"] = json!([])),
            "the tools capability requires at least one declared tool"
        );
    }

    #[test]
    fn tool_names_are_lowercase_words_unique_within_the_plugin() {
        for name in ["", "Show", "1show", "show-issue", "show issue", "_show"] {
            assert_eq!(
                tools_error(|manifest| manifest["tools"][0]["name"] = json!(name)),
                "tool name must start with a lowercase letter and contain only lowercase letters, digits, or underscores",
                "{name:?}"
            );
        }
        assert_eq!(
            tools_error(|manifest| manifest["tools"][1]["name"] = json!("issue_show")),
            "plugin integration defines duplicate tool issue_show"
        );
    }

    #[test]
    fn tools_are_validated() {
        assert_eq!(
            tools_error(|manifest| manifest["tools"][0]["description"] = json!(" ")),
            "tool issue_show description must be a nonempty string"
        );
        assert_eq!(
            tools_error(
                |manifest| manifest["tools"][0]["inputSchema"] = json!({ "type": "string" })
            ),
            "tool issue_show inputSchema must be a JSON Schema object with \"type\": \"object\""
        );
        assert_eq!(
            tools_error(|manifest| {
                manifest["tools"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("inputSchema");
            }),
            "tool issue_show inputSchema must be a JSON Schema object with \"type\": \"object\""
        );
        for timeout in [json!(0), json!(120_001), json!("10"), json!(1.5)] {
            assert_eq!(
                tools_error(|manifest| manifest["tools"][0]["timeoutMs"] = timeout.clone()),
                "tool issue_show timeoutMs must be an integer from 1 to 120000",
                "{timeout}"
            );
        }
        assert_eq!(
            tools_error(|manifest| manifest["tools"][0]["outputSchema"] = json!({})),
            "tool issue_show contains undocumented field: outputSchema"
        );
        assert_eq!(
            tools_error(|manifest| manifest["tools"][0]["annotations"] = json!({ "title": "x" })),
            "tool issue_show annotations contains undocumented field: title"
        );
        assert_eq!(
            tools_error(
                |manifest| manifest["tools"][0]["annotations"] = json!({ "readOnlyHint": "yes" })
            ),
            "tool issue_show annotations readOnlyHint must be a boolean"
        );
        assert_eq!(
            tools_error(|manifest| {
                manifest["tools"][0]["annotations"] =
                    json!({ "readOnlyHint": true, "destructiveHint": true })
            }),
            "tool issue_show cannot be both read-only and destructive"
        );
    }

    #[test]
    fn tool_call_messages_keep_their_wire_shape() {
        let params = ToolCallParams {
            tool: "show".into(),
            arguments: json!({ "key": "ISS-1" }).as_object().unwrap().clone(),
            caller: ToolCaller {
                session_id: "s1".into(),
                project: ToolProject {
                    id: "p1".into(),
                    name: "demo".into(),
                },
                task: None,
                auto_approve: false,
            },
        };
        assert_eq!(
            serde_json::to_value(&params).unwrap(),
            json!({
                "tool": "show",
                "arguments": { "key": "ISS-1" },
                "caller": {
                    "session_id": "s1",
                    "project": { "id": "p1", "name": "demo" },
                    "task": null,
                    "auto_approve": false
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<ToolCallResult>(json!({ "text": "ok" })).unwrap(),
            ToolCallResult {
                text: "ok".into(),
                is_error: false
            }
        );
        assert!(
            serde_json::from_value::<ToolCallResult>(json!({ "text": "x", "is_error": "no" }))
                .is_err()
        );
        assert_eq!(tool_timeout(None), std::time::Duration::from_secs(30));
        assert_eq!(
            tool_timeout(Some(250)),
            std::time::Duration::from_millis(250)
        );
    }

    #[test]
    fn tool_effects_follow_the_mcp_annotation_defaults() {
        let effect = |annotations: Value| {
            ToolAnnotations::deserialize(&annotations)
                .unwrap()
                .effect()
                .unwrap()
        };
        assert_eq!(effect(json!({ "readOnlyHint": true })), ToolEffect::Read);
        // MCP treats a tool that is not read-only as destructive unless it says otherwise.
        assert_eq!(effect(json!({})), ToolEffect::Delete);
        assert_eq!(
            effect(json!({ "destructiveHint": true })),
            ToolEffect::Delete
        );
        assert_eq!(
            effect(json!({ "destructiveHint": false })),
            ToolEffect::Write { idempotent: false }
        );
        assert_eq!(
            effect(json!({ "destructiveHint": false, "idempotentHint": true })),
            ToolEffect::Write { idempotent: true }
        );
    }

    #[test]
    fn dialog_can_claim_a_global_shortcut() {
        let mut manifest = manifest();
        manifest["ui_contributions"][0]["placement"] = json!("dialog");
        manifest["ui_contributions"][0]["shortcut"] = json!("Mod+Shift+R");
        assert_eq!(
            validate_local_manifest(&manifest, "macos-arm64").unwrap(),
            "bin/plugin"
        );
    }

    #[test]
    fn session_panel_can_claim_a_global_shortcut() {
        let mut manifest = manifest();
        manifest["ui_contributions"][0]["shortcut"] = json!("Mod+Shift+P");
        assert_eq!(
            validate_local_manifest(&manifest, "macos-arm64").unwrap(),
            "bin/plugin"
        );
    }
}
