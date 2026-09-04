use std::collections::HashSet;
use std::path::{Component, Path};

use anyhow::{anyhow, bail, Result};
use serde_json::{Map, Value};

const HOST_API_VERSION: &str = "planeai.plugin-host.v1";

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
    "tasks.read",
    "tasks.create",
    "task-events",
];
const BACKGROUND_SERVICE_FIELDS: &[&str] = &["method", "interval_setting", "default_interval_ms"];
const UI_PLACEMENTS: &[&str] = &[
    "sidebar.header",
    "sidebar.navigation",
    "sidebar.section",
    "sidebar.footer",
    "preferences",
    "main-pane",
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
    if required_string(object, "host_api_version")? != HOST_API_VERSION {
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
            bail!(
                "local plugins may only request settings, tasks.read, or task-events capabilities"
            );
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
        if placement != "main-pane" && shortcut.is_some() {
            bail!("UI contribution shortcuts are only valid for main-pane contributions");
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
    if matches!(
        key,
        "B" | "D" | "E" | "K" | "N" | "P" | "R" | "S" | "T" | "U" | "W"
    ) {
        bail!("UI contribution shortcut {shortcut} is reserved by PlaneAI");
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
