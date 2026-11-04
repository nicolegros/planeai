//! A plugin's declared MCP tool as its manifest and the plugin inventory store it, with the
//! naming and effect rules the registry and every launch share (ADR-0017).

use std::time::Duration;

use planeai_core::mcp::Effect;
use planeai_plugin_contract::{ToolAnnotations, ToolEffect};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginTool {
    /// Unique within the plugin; agents call it as `<plugin id>_<name>`.
    pub name: String,
    pub description: String,
    pub input_schema: Map<String, Value>,
    #[serde(default)]
    pub annotations: ToolAnnotations,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl PluginTool {
    pub fn served_name(&self, plugin_id: &str) -> Result<String, String> {
        planeai_core::mcp::plugin_tool_name(plugin_id, &self.name)
    }

    pub fn effect(&self, plugin_id: &str) -> Result<Effect, String> {
        let effect = self
            .annotations
            .effect()
            .map_err(|error| format!("plugin {plugin_id} tool {} {error}", self.name))?;
        Ok(match effect {
            ToolEffect::Read => Effect::Read,
            ToolEffect::Write { idempotent } => Effect::Write { idempotent },
            ToolEffect::Delete => Effect::Delete,
        })
    }

    pub fn timeout(&self) -> Duration {
        planeai_plugin_contract::tool_timeout(self.timeout_ms)
    }
}

/// The served names of a plugin's tools that agents may run without asking.
pub fn approved_names(plugin_id: &str, tools: &[PluginTool]) -> Result<Vec<String>, String> {
    let mut approved = Vec::new();
    for tool in tools {
        if tool.effect(plugin_id)?.pre_approved() {
            approved.push(tool.served_name(plugin_id)?);
        }
    }
    Ok(approved)
}

/// The declared tools of the plugins the inventory shows running, by plugin id.
pub fn running_plugin_tools(
    conn: &rusqlite::Connection,
) -> Result<Vec<(String, Vec<PluginTool>)>, String> {
    let mut statement = conn
        .prepare(
            "SELECT id, tools FROM plugin_inventory WHERE runtime_state = 'running' ORDER BY id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    rows.into_iter()
        .map(|(plugin_id, tools)| {
            let tools = serde_json::from_str(&tools)
                .map_err(|error| format!("plugin {plugin_id} tools: {error}"))?;
            Ok((plugin_id, tools))
        })
        .collect()
}
