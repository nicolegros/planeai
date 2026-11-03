//! PlaneAI's MCP server: the tools agents in sessions call natively, and their HTTP
//! transport (ADR-0016). Session tokens live in `planeai_core::mcp`.

pub mod tools;

#[cfg(test)]
mod tests;
