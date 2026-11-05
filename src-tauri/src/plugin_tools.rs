//! The bridge from the MCP registry to plugin sidecars (ADR-0017): a declared tool as a
//! registry entry, the session `tool.call` describes, and what the agent reads back.

use std::time::Duration;

use planeai::mcp::plugin_tool::PluginTool;
use planeai::mcp::tools::{ToolDef, ToolFuture, ToolOutput};
use planeai_core::mcp::{Effect, McpCaller};
use planeai_plugin_contract::{ToolCallResult, ToolCaller, ToolProject, ToolTask};
use rusqlite::Connection;
use serde_json::{Map, Value};

use crate::plugin_rpc::PluginRpcError;

/// The registry entry serving a plugin's declared tool under its served name, with its
/// effect from [`PluginTool::effect`].
pub(crate) fn plugin_tool_def(
    plugin_id: &str,
    tool: &PluginTool,
    effect: Effect,
    run: impl Fn(McpCaller, Map<String, Value>) -> ToolFuture + Send + Sync + 'static,
) -> Result<ToolDef, String> {
    Ok(ToolDef::plugin(
        tool.served_name(plugin_id)?,
        tool.description.clone(),
        tool.input_schema.clone(),
        effect,
        tool.annotations.open_world(),
        run,
    ))
}

/// A call the plugin received may have changed its data, even when it failed, as with
/// PlaneAI's task tools; a read-only one did not.
pub(crate) fn refreshes_views(effect: Effect, result: &Result<Value, PluginRpcError>) -> bool {
    effect != Effect::Read
        && !matches!(
            result,
            Err(PluginRpcError::NotSent(_) | PluginRpcError::NotRunning(_))
        )
}

/// The calling session as `tool.call` describes it to the plugin.
pub(crate) fn tool_caller(conn: &Connection, caller: &McpCaller) -> Result<ToolCaller, String> {
    let project = |id: &str| -> Result<ToolProject, String> {
        let project = crate::db::get_project(conn, id)
            .map_err(|error| error.to_string())?
            .ok_or("the project of this session no longer exists")?;
        Ok(ToolProject {
            id: project.id,
            name: project.name,
        })
    };
    Ok(ToolCaller {
        session_id: caller.session_id.clone(),
        project: project(&caller.project_id)?,
        task: match &caller.task {
            Some(task) => Some(ToolTask {
                key: task.key.clone(),
                project: project(&task.project_id)?,
            }),
            None => None,
        },
        auto_approve: caller.auto_approve,
    })
}

/// What the agent reads back from a `tool.call`.
pub(crate) fn tool_output(
    plugin_id: &str,
    timeout: Duration,
    result: Result<Value, PluginRpcError>,
) -> ToolOutput {
    let failure = |message: String| ToolOutput::error(&format!("plugin {plugin_id}: {message}"));
    match result {
        Ok(value) => match serde_json::from_value::<ToolCallResult>(value) {
            Ok(ToolCallResult { text, is_error }) => ToolOutput { text, is_error },
            Err(error) => failure(format!("invalid tool.call result: {error}")),
        },
        Err(PluginRpcError::Rpc { code: -32800, .. }) => failure(format!(
            "no answer within {} ms; the call was cancelled",
            timeout.as_millis()
        )),
        Err(PluginRpcError::Rpc { message, .. }) => failure(message),
        Err(error) => failure(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_rpc::timed_out_request_error;

    #[test]
    fn a_declared_tool_is_served_under_its_plugin_with_its_effect() {
        let tool = |annotations: Value| -> PluginTool {
            serde_json::from_value(serde_json::json!({
                "name": "show",
                "description": "Show an issue.",
                "inputSchema": { "type": "object" },
                "annotations": annotations,
            }))
            .unwrap()
        };
        let served = |annotations: Value| {
            let tool = tool(annotations);
            let effect = tool.effect("issues").unwrap();
            let def = plugin_tool_def("issues", &tool, effect, |_, _| {
                Box::pin(async { planeai::mcp::tools::ToolOutput::error("unused") })
            })
            .unwrap()
            .tool();
            let hints = def.annotations.unwrap();
            (
                def.name.to_string(),
                hints.read_only_hint.unwrap(),
                hints.destructive_hint.unwrap(),
                hints.idempotent_hint.unwrap(),
                hints.open_world_hint.unwrap(),
            )
        };
        assert_eq!(
            served(serde_json::json!({ "readOnlyHint": true, "openWorldHint": false })),
            ("issues_show".into(), true, false, true, false)
        );
        assert_eq!(
            served(serde_json::json!({ "destructiveHint": false })),
            ("issues_show".into(), false, false, false, true)
        );
        assert_eq!(
            served(serde_json::json!({})),
            ("issues_show".into(), false, true, true, true)
        );
        assert_eq!(
            tool(serde_json::json!({})).timeout(),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn calls_that_reached_the_plugin_refresh_its_views_unless_read_only() {
        let answered = Ok(serde_json::json!({ "text": "ok" }));
        let failed = Err(PluginRpcError::Rpc {
            code: -32000,
            message: "GitHub is down".into(),
        });
        for effect in [Effect::Write { idempotent: true }, Effect::Delete] {
            assert!(refreshes_views(effect, &answered));
            assert!(
                refreshes_views(effect, &failed),
                "a failed call may have written"
            );
            assert!(refreshes_views(
                effect,
                &Err(timed_out_request_error("tool.call"))
            ));
            assert!(!refreshes_views(
                effect,
                &Err(PluginRpcError::NotRunning("stopped".into()))
            ));
            assert!(!refreshes_views(
                effect,
                &Err(PluginRpcError::NotSent("too large".into()))
            ));
        }
        assert!(!refreshes_views(Effect::Read, &answered));
    }

    #[test]
    fn tool_call_describes_the_calling_session_with_project_names() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        let project = crate::db::create_project(&conn, "Demo", "/tmp/demo").unwrap();
        let other = crate::db::create_project(&conn, "other", "/tmp/other").unwrap();
        let mut caller = planeai_core::mcp::McpCaller {
            session_id: "aaaaaaaa-1111-2222-3333-444444444444".into(),
            project_id: project.id.clone(),
            task: Some(planeai_core::mcp::CallerTask {
                key: "OTH-3".into(),
                project_id: other.id.clone(),
            }),
            auto_approve: true,
            approved_plugin_tools: Vec::new(),
        };
        assert_eq!(
            serde_json::to_value(tool_caller(&conn, &caller).unwrap()).unwrap(),
            serde_json::json!({
                "session_id": "aaaaaaaa-1111-2222-3333-444444444444",
                "project": { "id": project.id, "name": project.name },
                "task": { "key": "OTH-3", "project": { "id": other.id, "name": "other" } },
                "auto_approve": true,
            })
        );
        caller.task = None;
        assert_eq!(tool_caller(&conn, &caller).unwrap().task, None);
    }

    #[test]
    fn every_tool_call_outcome_reaches_the_agent() {
        use planeai::mcp::tools::ToolOutput;
        let timeout = Duration::from_millis(2500);
        let output = |result| tool_output("issues", timeout, result);
        assert_eq!(
            output(Ok(serde_json::json!({ "text": "issue: ISS-1" }))),
            ToolOutput {
                text: "issue: ISS-1".into(),
                is_error: false
            }
        );
        assert_eq!(
            output(Ok(
                serde_json::json!({ "text": "no such issue", "is_error": true })
            )),
            ToolOutput {
                text: "no such issue".into(),
                is_error: true
            }
        );
        for (result, message) in [
            (
                Ok(serde_json::json!({ "content": [] })),
                "plugin issues: invalid tool.call result: missing field `text`",
            ),
            (
                Err(PluginRpcError::Rpc {
                    code: -32800,
                    message: "request cancelled".into(),
                }),
                "plugin issues: no answer within 2500 ms; the call was cancelled",
            ),
            (
                Err(timed_out_request_error("tool.call")),
                "plugin issues: plugin RPC tool.call timed out",
            ),
            (
                Err(PluginRpcError::NotRunning(
                    "plugin issues is not running".into(),
                )),
                "plugin issues: plugin issues is not running",
            ),
            (
                Err(PluginRpcError::Rpc {
                    code: -32602,
                    message: "key is required".into(),
                }),
                "plugin issues: key is required",
            ),
        ] {
            assert_eq!(output(result), ToolOutput::error(message), "{message}");
        }
    }
}
