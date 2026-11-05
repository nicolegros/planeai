//! PlaneAI's MCP server: the tools agents in sessions call natively, and their HTTP
//! transport (ADR-0016). Session tokens live in `planeai_core::mcp`.

pub mod plugin_tool;
pub mod server;
pub mod tools;

use planeai_core::mcp::McpLaunch;

/// The MCP server a launch outside the app's own launch paths connects its agent to: `url`,
/// with the read and write tools of the plugins the app's inventory shows running, which
/// are the plugin tools its registry serves (ADR-0017). An unreadable inventory approves
/// no plugin tool.
pub fn launch_from_inventory(conn: &rusqlite::Connection, url: String) -> McpLaunch {
    let approved_plugin_tools = plugin_tool::running_plugin_tools(conn)
        .and_then(|plugins| {
            plugins
                .iter()
                .map(|(plugin_id, tools)| plugin_tool::approved_names(plugin_id, tools))
                .collect::<Result<Vec<_>, _>>()
        })
        .inspect_err(|error| tracing::warn!(%error, "plugin tools left to ask"))
        .unwrap_or_default()
        .concat();
    McpLaunch {
        url,
        approved_plugin_tools,
    }
}

#[cfg(test)]
mod tests;
