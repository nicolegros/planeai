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

pub fn supports_host_api_version(version: &str) -> bool {
    matches!(
        version,
        HOST_API_VERSION | RECIPIENT_HOST_API_VERSION | PROVIDER_HOST_API_VERSION
    )
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
    "tasks.read",
    "tasks.create",
    "tasks.transition",
    "task-events",
    "providers",
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
    Ok(entrypoint.to_owned())
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
    }
    Ok(())
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
    if object.get("host_api_version").and_then(Value::as_str) != Some(PROVIDER_HOST_API_VERSION) {
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
            "host_api_version": "planeai.plugin-host.v1",
            "source_kind": "local",
            "backend_entrypoints": { "macos-arm64": "bin/plugin" },
            "capabilities": [
                "sessions.repository-context",
                "sessions.prompt",
                "session-events",
                "sessions.actions",
                "sessions.advisories",
                "sessions.complete",
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
    fn accepts_visual_only_session_indicators() {
        let mut manifest = manifest();
        manifest["ui_contributions"][0]["placement"] = json!("session.indicator");
        assert_eq!(
            validate_local_manifest(&manifest, "macos-arm64").unwrap(),
            "bin/plugin"
        );
    }

    #[test]
    fn session_indicators_reject_shortcuts_and_order() {
        let mut shortcut_manifest = manifest();
        shortcut_manifest["ui_contributions"][0]["placement"] = json!("session.indicator");
        shortcut_manifest["ui_contributions"][0]["shortcut"] = json!("Mod+Shift+P");
        assert!(validate_local_manifest(&shortcut_manifest, "macos-arm64")
            .unwrap_err()
            .to_string()
            .contains("shortcuts are only valid"));

        let mut ordered_manifest = manifest();
        ordered_manifest["ui_contributions"][0]["placement"] = json!("session.indicator");
        ordered_manifest["ui_contributions"][0]["order"] = json!(1);
        assert!(validate_local_manifest(&ordered_manifest, "macos-arm64")
            .unwrap_err()
            .to_string()
            .contains("order is only valid"));
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
