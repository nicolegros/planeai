use std::future::Future;
use std::sync::Arc;

use serde_json::Value;
use tauri::{AppHandle, Manager};

use super::registry::ProviderSessions;
use super::routing::SessionRuntimeContext;
use super::sink::ProviderStatus;
use crate::db;
use crate::plugin_rpc::PluginRpcError;
use crate::plugins::{PluginProvider, PluginRuntimeSupervisor};
use planeai_core::mcp::McpLaunch;
use planeai_plugin_contract::provider::ProviderMcp;

/// Mint the token a provider's agent connects with, as a PTY launch does.
pub(super) fn provider_mcp(
    conn: &rusqlite::Connection,
    launch: Option<&McpLaunch>,
    session_id: &str,
) -> Result<Option<ProviderMcp>, String> {
    Ok(
        planeai_core::mcp::endpoint(conn, launch, session_id)?.map(|endpoint| {
            let [(_, url), (_, token)] = endpoint.env();
            ProviderMcp {
                url: url.to_string(),
                token: token.to_string(),
            }
        }),
    )
}

/// What session routing needs from the app: the plugin runtimes, the session store and
/// where status goes. `AppRuntime` provides them for real; tests drive routing through a fake.
pub trait ProviderRuntime: Sync {
    type Status: ProviderStatus;
    fn sessions(&self) -> &ProviderSessions;
    fn status(&self) -> &Self::Status;
    /// The provider, when its plugin is running and declares it.
    fn running_provider(
        &self,
        plugin_id: &str,
        provider_id: &str,
    ) -> impl Future<Output = Result<PluginProvider, String>> + Send;
    fn running_instance(&self, plugin_id: &str) -> impl Future<Output = Option<u64>> + Send;
    fn request(
        &self,
        plugin_id: &str,
        method: &str,
        params: Value,
    ) -> impl Future<Output = Result<Value, PluginRpcError>> + Send;
    /// The session's runtime context; an error when it is unknown or not active.
    fn session_context(
        &self,
        session_id: &str,
    ) -> impl Future<Output = Result<SessionRuntimeContext, String>> + Send;
    /// A fresh MCP token for the session's agent, which revokes the previous one; `None`
    /// while the server is not listening.
    fn mcp_endpoint(
        &self,
        session_id: &str,
    ) -> impl Future<Output = Result<Option<ProviderMcp>, String>> + Send;
    /// Each session's stored status, in the order asked, `None` when it has no row.
    fn session_statuses(
        &self,
        session_ids: Vec<String>,
    ) -> impl Future<Output = Result<Vec<(String, Option<String>)>, String>> + Send;
}

/// The app's plugin supervisor and database.
pub struct AppRuntime {
    app: AppHandle,
    supervisor: Arc<PluginRuntimeSupervisor>,
}

impl AppRuntime {
    pub fn new(app: &AppHandle) -> Self {
        Self {
            app: app.clone(),
            supervisor: app.state::<crate::plugins::PluginRuntimeHandle>().0.clone(),
        }
    }
}

impl ProviderRuntime for AppRuntime {
    type Status = AppHandle;

    fn sessions(&self) -> &ProviderSessions {
        self.supervisor.provider_sessions()
    }

    fn status(&self) -> &AppHandle {
        &self.app
    }

    async fn running_provider(
        &self,
        plugin_id: &str,
        provider_id: &str,
    ) -> Result<PluginProvider, String> {
        self.supervisor
            .running_provider(plugin_id, provider_id)
            .await
    }

    async fn running_instance(&self, plugin_id: &str) -> Option<u64> {
        self.supervisor.running_instance(plugin_id).await
    }

    async fn request(
        &self,
        plugin_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, PluginRpcError> {
        self.supervisor
            .provider_request(plugin_id, method, params)
            .await
    }

    async fn session_context(&self, session_id: &str) -> Result<SessionRuntimeContext, String> {
        let db = self.app.state::<crate::state::DbState>().0.clone();
        let extra_path_dirs = crate::plugins::configured_extra_path_dirs(&self.app);
        let session_id = session_id.to_string();
        crate::commands::blocking(move || {
            let conn = db.lock().map_err(|error| error.to_string())?;
            let session = db::get_session(&conn, &session_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("session not found: {session_id}"))?;
            if session.status != "active" {
                return Err(format!(
                    "session is not active (status: {})",
                    session.status
                ));
            }
            SessionRuntimeContext::for_session(&conn, &session, extra_path_dirs)
        })
        .await
    }

    async fn mcp_endpoint(&self, session_id: &str) -> Result<Option<ProviderMcp>, String> {
        let launch = self.app.state::<crate::state::McpServerState>().0.launch();
        let db = self.app.state::<crate::state::DbState>().0.clone();
        let session_id = session_id.to_string();
        crate::commands::blocking(move || {
            let conn = db.lock().map_err(|error| error.to_string())?;
            provider_mcp(&conn, launch.as_ref(), &session_id)
        })
        .await
    }

    async fn session_statuses(
        &self,
        session_ids: Vec<String>,
    ) -> Result<Vec<(String, Option<String>)>, String> {
        let db = self.app.state::<crate::state::DbState>().0.clone();
        crate::commands::blocking(move || {
            let conn = db.lock().map_err(|error| error.to_string())?;
            session_ids
                .into_iter()
                .map(|session_id| {
                    let status = db::get_session(&conn, &session_id)
                        .map_err(|error| error.to_string())?
                        .map(|session| session.status);
                    Ok((session_id, status))
                })
                .collect()
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_provider_token_resolves_to_its_session_and_none_is_minted_while_not_listening() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "demo", "/tmp/demo").unwrap();
        let session_id = "aaaaaaaa-1111-2222-3333-444444444444";
        db::create_session_with_id(
            &conn,
            session_id,
            &project.id,
            "Chat",
            None,
            "main",
            None,
            Some("fixture:echo"),
            "plugin",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(provider_mcp(&conn, None, session_id), Ok(None));

        let launch = McpLaunch {
            url: "http://127.0.0.1:1/mcp".into(),
            approved_plugin_tools: vec!["fixture_echo".into()],
        };
        let mcp = provider_mcp(&conn, Some(&launch), session_id)
            .unwrap()
            .unwrap();
        assert_eq!(mcp.url, launch.url);
        let caller = planeai_core::mcp::resolve(&conn, &mcp.token)
            .unwrap()
            .unwrap();
        assert_eq!(caller.session_id, session_id);
        assert_eq!(caller.approved_plugin_tools, ["fixture_echo"]);
    }
}
