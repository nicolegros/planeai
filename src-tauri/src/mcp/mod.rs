//! PlaneAI's MCP server: the tools agents in sessions call natively, and their HTTP
//! transport (ADR-0016). Session tokens live in `planeai_core::mcp`.

pub mod server;
pub mod tools;

use planeai_core::mcp::McpLaunch;
use serde::Deserialize;

/// The MCP server a launch outside the app's own launch paths connects its agent to: `url`,
/// with the read and write tools of the plugins the app's inventory shows running, which
/// are the plugin tools its registry serves (ADR-0017).
pub fn launch_from_inventory(conn: &rusqlite::Connection, url: String) -> McpLaunch {
    let approved_plugin_tools = running_plugin_tools(conn)
        .inspect_err(
            |error| tracing::warn!(%error, "plugin tools left to ask: inventory unreadable"),
        )
        .unwrap_or_default();
    McpLaunch {
        url,
        approved_plugin_tools,
    }
}

#[derive(Deserialize)]
struct DeclaredTool {
    name: String,
    #[serde(default)]
    annotations: planeai_plugin_contract::ToolAnnotations,
}

fn running_plugin_tools(conn: &rusqlite::Connection) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(
            "SELECT id, capabilities, tools FROM plugin_inventory
             WHERE runtime_state = 'running' ORDER BY id",
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
    let mut approved = Vec::new();
    for (plugin_id, capabilities, tools) in rows {
        let capabilities: Vec<String> =
            serde_json::from_str(&capabilities).map_err(|error| error.to_string())?;
        if !capabilities.iter().any(|capability| capability == "tools") {
            continue;
        }
        let tools: Vec<DeclaredTool> =
            serde_json::from_str(&tools).map_err(|error| error.to_string())?;
        approved.extend(tools.into_iter().filter_map(|tool| {
            let effect = tool.annotations.effect().ok()?;
            (effect != planeai_plugin_contract::ToolEffect::Delete)
                .then(|| planeai_core::mcp::plugin_tool_name(&plugin_id, &tool.name).ok())
                .flatten()
        }));
    }
    Ok(approved)
}

#[cfg(test)]
mod tests;
