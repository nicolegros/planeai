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
