//! PlaneAI's MCP server: the tools agents in sessions call natively (ADR-0016).

use planeai_core::mcp::McpEndpoint;

use crate::config::Config;

/// Mint the token the session's next agent process carries, `None` while the server is off.
pub fn endpoint(
    conn: &rusqlite::Connection,
    cfg: &Config,
    session_id: &str,
) -> Result<Option<McpEndpoint>, String> {
    cfg.mcp_url()
        .map(|url| planeai_core::mcp::issue(conn, session_id, &url))
        .transpose()
        .map_err(|error| format!("failed to mint the session's MCP token: {error}"))
}
