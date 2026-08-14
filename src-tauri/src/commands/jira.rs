use serde::Serialize;
use tauri::{Emitter, State};
use tokio::sync::Mutex;

use planeai_jira::config::JiraConfig;
use planeai_jira::SyncResult;
use planeai_tasks::model::{CreateParams, ListFilter, Status, DEFAULT_BASE_BRANCH};
use planeai_tasks::provider::TaskProvider;
use planeai_tasks::sqlite::SqliteRepository;

use crate::commands::tasks::TaskItem;
use crate::jira::JiraState;
use crate::state::{ConfigState, DbState};
use crate::{db, jira};

pub struct JiraHandle(pub(crate) std::sync::Arc<Mutex<JiraSlot>>);

impl Clone for JiraHandle {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub(crate) struct JiraSlot {
    pub(crate) state: Option<JiraState>,
    /// Reserves connect construction and OAuth so concurrent connects are rejected before any I/O.
    pub(crate) connecting: bool,
    /// True once this state's auth signal has been wired back to runtime deactivation.
    pub(crate) listener_attached: bool,
}

impl JiraHandle {
    pub fn new(state: Option<JiraState>) -> Self {
        // `state` has not escaped yet, so its listener can be attached without acquiring the
        // async slot mutex. States constructed by `jira_connect` use the awaited method below.
        let auth = state.as_ref().map(|state| state.auth.clone());
        let handle = Self(std::sync::Arc::new(Mutex::new(JiraSlot {
            state,
            connecting: false,
            listener_attached: auth.is_some(),
        })));
        if let Some(auth) = auth {
            handle.attach_runtime_deactivation_listener(auth);
        }
        handle
    }

    /// Auth clears happen inside the Jira library (invalid refresh grants as well as explicit
    /// disconnects). Subscribe the app state to that signal so stale sync/writeback/cancellation
    /// state cannot survive into a reconnect.
    ///
    /// This waits for the slot lock instead of skipping registration under contention. Callers
    /// must await it before starting OAuth, but registration itself occurs after the lock is
    /// released so no runtime mutex is held while touching the auth object.
    pub(crate) async fn install_runtime_deactivation_listener(&self) {
        let auth = {
            let mut slot = self.0.lock().await;
            if slot.listener_attached {
                return;
            }
            let Some(auth) = slot.state.as_ref().map(|state| state.auth.clone()) else {
                return;
            };
            slot.listener_attached = true;
            auth
        };
        self.attach_runtime_deactivation_listener(auth);
    }

    pub(crate) async fn confirm_connect_reservation(
        &self,
        auth: &std::sync::Arc<planeai_jira::auth::JiraAuth>,
    ) -> Result<(), String> {
        let slot = self.0.lock().await;
        let valid = slot.connecting
            && slot
                .state
                .as_ref()
                .is_some_and(|state| std::sync::Arc::ptr_eq(&state.auth, auth))
            && auth.is_connection_active();
        valid
            .then_some(())
            .ok_or_else(|| "Jira authorization was cancelled before it started".to_string())
    }

    fn attach_runtime_deactivation_listener(
        &self,
        auth: std::sync::Arc<planeai_jira::auth::JiraAuth>,
    ) {
        let slot = std::sync::Arc::downgrade(&self.0);
        auth.set_connection_state_listener(std::sync::Arc::new(move || {
            let Some(slot) = slot.upgrade() else {
                return;
            };
            tauri::async_runtime::spawn(async move {
                let mut slot = slot.lock().await;
                if let Some(state) = slot.state.as_mut() {
                    if !state.auth.is_connection_active() {
                        state.deactivate();
                    }
                }
                slot.connecting = false;
            });
        }));
    }
}

#[derive(Serialize)]
pub struct JiraStatusResponse {
    pub connected: bool,
    pub site: Option<String>,
}

fn get_jira_config(config_state: &ConfigState) -> Result<JiraConfig, String> {
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    cfg.integrations
        .as_ref()
        .and_then(|i| i.jira.clone())
        .ok_or_else(|| "jira not configured".to_string())
}

async fn prepare_activation(
    inputs: crate::jira::JiraActivationInputs,
    jira_config: JiraConfig,
    app: tauri::AppHandle,
) -> Result<crate::jira::PreparedJiraActivation, String> {
    crate::commands::blocking(move || JiraState::prepare_activation(inputs, jira_config, app)).await
}

/// Reserve Jira connection work and reuse a disconnected auth object only for its original site.
/// A `JiraAuth` owns both the OAuth site identity and the persisted cloud ID, so reusing it after
/// the site changes could authorize the new settings against the previous Jira Cloud.
fn reserve_jira_connect(
    slot: &mut JiraSlot,
    jira_config: &JiraConfig,
) -> Result<Option<std::sync::Arc<planeai_jira::auth::JiraAuth>>, String> {
    if slot.connecting {
        return Err("Jira authorization is already in progress".to_string());
    }
    if let Some(state) = slot.state.as_mut() {
        // An invalid grant clears auth from the library asynchronously. Remove its cancelled
        // runtime objects before reserving the reconnect, so activation can succeed later.
        if state.auth.is_connection_active() {
            return Err("Jira is already connected or authorization is in progress".to_string());
        }
        state.deactivate();
        if state.auth.site() != jira_config.site {
            // Drop the old auth together with its runtime. Construction below creates an auth
            // with the current site before any OAuth work begins.
            slot.state = None;
            slot.listener_attached = false;
        }
    }
    slot.connecting = true;
    Ok(slot.state.as_ref().map(|state| state.auth.clone()))
}

#[tauri::command]
pub async fn jira_connect(
    app: tauri::AppHandle,
    jira: State<'_, JiraHandle>,
    config_state: State<'_, ConfigState>,
) -> Result<(), String> {
    let jira_config = get_jira_config(&config_state)?;
    let existing_auth = {
        let mut slot = jira.0.lock().await;
        reserve_jira_connect(&mut slot, &jira_config)?
    };

    // JiraState construction probes token storage and opens/migrates SQLite. It must never run
    // on macOS's main Tauri IPC thread, and the handle reservation above prevents duplicates.
    let auth = match existing_auth {
        Some(auth) => auth,
        None => {
            let config = jira_config.clone();
            let construction_app = app.clone();
            let state = crate::commands::blocking(move || {
                crate::jira::construct_jira_state(config, construction_app)
            })
            .await;
            let mut slot = jira.0.lock().await;
            let state = match state {
                Ok(state) if slot.connecting && slot.state.is_none() => state,
                Ok(_) => {
                    return Err("Jira authorization was cancelled before it started".to_string())
                }
                Err(error) => {
                    slot.connecting = false;
                    return Err(error);
                }
            };
            let auth = state.auth.clone();
            slot.state = Some(state);
            auth
        }
    };
    jira.install_runtime_deactivation_listener().await;
    // Listener registration awaits the Jira slot. A concurrent disconnect can clear the
    // reservation in that interval; never begin OAuth unless this exact state still owns it.
    jira.confirm_connect_reservation(&auth).await?;

    // Do not hold JiraHandle across browser, callback, token storage, or network awaits:
    // disconnect can invalidate this authorization attempt immediately.
    let connect_result = auth.connect().await.map_err(|e| e.to_string());
    {
        let mut slot = jira.0.lock().await;
        let Some(state) = slot.state.as_ref() else {
            slot.connecting = false;
            return Err("jira not configured".to_string());
        };
        if !std::sync::Arc::ptr_eq(&state.auth, &auth) {
            slot.connecting = false;
            return Err("Jira authorization was replaced before it completed".to_string());
        }
        slot.connecting = false;
    }
    connect_result?;

    let inputs = {
        let mut slot = jira.0.lock().await;
        let state = slot.state.as_mut().ok_or("jira not configured")?;
        if !std::sync::Arc::ptr_eq(&state.auth, &auth) || !state.auth.is_connection_active() {
            return Err("Jira authorization was disconnected before it completed".to_string());
        }
        if state.activating || state.sync.is_some() || state.cancel.is_some() {
            return Err("jira sync is already initialized".to_string());
        }
        state.activating = true;
        state.activation_inputs()
    };
    let prepared = prepare_activation(inputs, jira_config, app.clone()).await;

    let (sync, cancel) = {
        let mut slot = jira.0.lock().await;
        let state = slot.state.as_mut().ok_or("jira not configured")?;
        if !std::sync::Arc::ptr_eq(&state.auth, &auth) {
            return Err("Jira authorization was replaced before activation completed".to_string());
        }
        state.activating = false;
        let prepared = prepared?;
        let cancel = state.install_activation(prepared)?;
        let sync = state
            .sync
            .clone()
            .ok_or("jira sync not initialized after activate")?;
        (sync, cancel)
    };
    tokio::spawn(async move { sync.start(cancel).await });

    if let Err(error) = app.emit("jira-connection-state-changed", ()) {
        tracing::warn!(error = %error, "failed to emit Jira connection state change");
    }
    tracing::info!("jira: connected");
    Ok(())
}

#[tauri::command]
pub async fn jira_disconnect(jira: State<'_, JiraHandle>) -> Result<(), String> {
    let auth = {
        let mut slot = jira.0.lock().await;
        // A disconnect during state construction invalidates the reservation before OAuth begins.
        slot.connecting = false;
        let Some(state) = slot.state.as_mut() else {
            return Ok(());
        };
        state.deactivate();
        state.auth.clone()
    };
    auth.disconnect().await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn run_manual_sync(
    sync: std::sync::Arc<planeai_jira::JiraSync>,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<SyncResult, String> {
    // `JiraSync` performs synchronous SQLite/task-provider work between network awaits. Poll the
    // manual run from the blocking pool so Tauri's macOS IPC thread never executes that work.
    // The installed token remains the same one `disconnect` cancels, including while waiting for
    // the shared run lock or an in-flight request.
    crate::commands::blocking(move || {
        tauri::async_runtime::block_on(async move {
            sync.sync_now_with_cancellation(&cancel)
                .await
                .map_err(|e| e.to_string())
        })
    })
    .await
}

enum ManualSyncRuntime {
    Active {
        auth: std::sync::Arc<planeai_jira::auth::JiraAuth>,
        sync: std::sync::Arc<planeai_jira::JiraSync>,
        cancel: tokio_util::sync::CancellationToken,
    },
    Activate {
        auth: std::sync::Arc<planeai_jira::auth::JiraAuth>,
        inputs: crate::jira::JiraActivationInputs,
        restart_background: bool,
    },
}

/// Select a runtime for manual sync without I/O. When settings changed since the runtime was
/// created, this cancels and discards that runtime so the caller can prepare one from the current
/// config outside the Jira slot lock.
fn select_manual_sync_runtime(
    state: &mut JiraState,
    jira_config: &JiraConfig,
) -> Result<ManualSyncRuntime, String> {
    if state.activating {
        return Err("jira sync activation is already in progress".to_string());
    }

    match (state.sync.clone(), state.cancel.clone()) {
        (Some(sync), Some(cancel)) if state.sync_matches_config(jira_config) => {
            Ok(ManualSyncRuntime::Active {
                auth: state.auth.clone(),
                sync,
                cancel,
            })
        }
        (Some(_), Some(_)) => {
            // An auth instance is tied to its original site and cloud ID. Rebuilding only the
            // sync for a different site would silently send the new JQL to the old site.
            if state
                .sync_config
                .as_ref()
                .is_some_and(|active_config| active_config.site != jira_config.site)
            {
                return Err("Jira site changed; disconnect and reconnect to Jira".to_string());
            }
            // Cancel the timer/manual run and discard its listener/writeback before creating a
            // replacement. `install_activation` re-registers the auth cancellation token.
            state.deactivate();
            if !state.auth.is_connection_active() {
                return Err("Jira is disconnected; reconnect to Jira".to_string());
            }
            state.activating = true;
            Ok(ManualSyncRuntime::Activate {
                auth: state.auth.clone(),
                inputs: state.activation_inputs(),
                restart_background: true,
            })
        }
        (None, None) => {
            if !state.auth.is_connection_active() {
                return Err("Jira is disconnected; reconnect to Jira".to_string());
            }
            state.activating = true;
            Ok(ManualSyncRuntime::Activate {
                auth: state.auth.clone(),
                inputs: state.activation_inputs(),
                restart_background: false,
            })
        }
        _ => Err("jira sync state is missing its cancellation token".to_string()),
    }
}

#[tauri::command]
pub async fn jira_sync_now(
    app: tauri::AppHandle,
    jira: State<'_, JiraHandle>,
    config_state: State<'_, ConfigState>,
) -> Result<SyncResult, String> {
    let jira_config = get_jira_config(&config_state)?;
    let selection = {
        let mut slot = jira.0.lock().await;
        if slot.connecting {
            return Err("Jira authorization is already in progress".to_string());
        }
        let state = slot.state.as_mut().ok_or("jira not configured")?;
        select_manual_sync_runtime(state, &jira_config)?
    };

    let (auth, active_sync, activation_inputs, restart_background) = match selection {
        ManualSyncRuntime::Active { auth, sync, cancel } => {
            (auth, Some((sync, cancel)), None, false)
        }
        ManualSyncRuntime::Activate {
            auth,
            inputs,
            restart_background,
        } => (auth, None, Some(inputs), restart_background),
    };
    let (sync, cancel) = match active_sync {
        Some(active) => active,
        None => {
            let prepared = prepare_activation(
                activation_inputs.expect("activation inputs reserved with activation"),
                jira_config,
                app,
            )
            .await;
            let mut slot = jira.0.lock().await;
            let state = slot.state.as_mut().ok_or("jira not configured")?;
            if !std::sync::Arc::ptr_eq(&state.auth, &auth) {
                return Err(
                    "Jira authorization was replaced before activation completed".to_string(),
                );
            }
            state.activating = false;
            let cancel = state.install_activation(prepared?)?;
            let sync = state
                .sync
                .clone()
                .ok_or("jira sync not initialized after activate")?;
            (sync, cancel)
        }
    };

    let result = run_manual_sync(sync.clone(), cancel.clone()).await;
    // A stale active runtime had a periodic loop before replacement. Restore that lifecycle only
    // after the user-requested run has had the first turn with the new configuration.
    if restart_background && !cancel.is_cancelled() {
        tokio::spawn(async move { sync.start(cancel).await });
    }
    result
}

/// Read-only status check. Does not activate sync or mutate state.
#[tauri::command]
pub async fn jira_status(
    jira: State<'_, JiraHandle>,
    config_state: State<'_, ConfigState>,
) -> Result<JiraStatusResponse, String> {
    let auth = {
        let guard = jira.0.lock().await;
        guard.state.as_ref().map(|state| state.auth.clone())
    };
    let connected = match auth {
        Some(auth) => crate::commands::blocking(move || Ok(auth.is_connected())).await?,
        None => false,
    };

    let site = config_state.0.lock().ok().and_then(|cfg| {
        cfg.integrations
            .as_ref()?
            .jira
            .as_ref()
            .map(|j| j.site.clone())
    });

    Ok(JiraStatusResponse { connected, site })
}

/// Mark a Jira-synced task as done. Resolves the task provider internally
/// so the frontend doesn't need to know repo_path.
#[tauri::command]
pub async fn mark_jira_task_done(
    key: String,
    config_state: State<'_, ConfigState>,
    jira: State<'_, JiraHandle>,
) -> Result<(), String> {
    use planeai_tasks::model::UpdateParams;

    let jira_config = get_jira_config(&config_state)?;
    let repo = crate::jira::open_task_provider(&jira_config)?;
    repo.update(
        &key,
        UpdateParams {
            status: Some(Status::Done),
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;

    // Fire writeback
    if let Ok(guard) = jira.0.try_lock() {
        if let Some(state) = guard.state.as_ref() {
            if let Ok(cfg) = config_state.0.lock() {
                state.try_writeback(&key, Status::Done, &cfg);
            }
        } else {
            tracing::warn!(key = %key, "mark_jira_task_done: jira state not initialized, skipping writeback");
        }
    } else {
        tracing::warn!(key = %key, "mark_jira_task_done: could not acquire jira lock, skipping writeback");
    }

    Ok(())
}

/// Assign a Jira task to a project by creating a child task in the project's task store.
/// Fires on_start writeback when it's the first child created for this Jira parent.
#[tauri::command]
pub async fn assign_jira_task(
    jira_task_key: String,
    project_id: String,
    db_state: State<'_, DbState>,
    config_state: State<'_, ConfigState>,
    jira: State<'_, JiraHandle>,
) -> Result<TaskItem, String> {
    // 1. Get the Jira task to inherit title/description
    let jira_config = get_jira_config(&config_state)?;
    let jira_repo = jira::open_task_provider(&jira_config)?;
    let parent_task = jira_repo.get(&jira_task_key).map_err(|e| e.to_string())?;

    // 2. Resolve the target project's prefix
    let project = {
        let conn = db_state.0.lock().map_err(|e| e.to_string())?;
        db::get_project(&conn, &project_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("project not found: {project_id}"))?
    };
    let db_path = planeai_paths::db_path();
    let project_repo = SqliteRepository::open(db_path.to_str().unwrap(), &project.prefix)
        .map_err(|e| e.to_string())?;

    // 3. Check if this parent already has children (for on_start logic)
    let existing_children = project_repo
        .list(ListFilter {
            parent_key: Some(Some(jira_task_key.clone())),
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;
    let is_first_child = existing_children.is_empty();

    // 4. Create child task in the project's repo
    let child = project_repo
        .create(CreateParams {
            key: None,
            title: parent_task.title,
            description: parent_task.description,
            parent_key: Some(jira_task_key.clone()),
            base_branch: DEFAULT_BASE_BRANCH.to_string(),
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;

    // 5. Fire on_start writeback if this is the first child
    if is_first_child {
        if let Ok(guard) = jira.0.try_lock() {
            if let Some(state) = guard.state.as_ref() {
                if let Ok(cfg) = config_state.0.lock() {
                    state.try_writeback(&jira_task_key, Status::InProgress, &cfg);
                }
            }
        } else {
            tracing::warn!(key = %jira_task_key, "assign_jira_task: could not acquire jira lock, skipping writeback");
        }
    }

    Ok(TaskItem::from(child))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    use planeai_jira::auth::JiraAuth;
    use planeai_jira::client::JiraClient;
    use planeai_jira::repository::JiraRepository;
    use planeai_jira::{JiraSync, JiraWriteback};
    use rusqlite::Connection;
    use tokio_util::sync::CancellationToken;

    fn active_auth() -> Arc<JiraAuth> {
        let token_dir = tempfile::tempdir().unwrap().keep();
        std::fs::write(token_dir.join("refresh_token"), "refresh").unwrap();
        std::fs::write(token_dir.join("cloud_id"), "cloud").unwrap();
        std::fs::write(token_dir.join("connection_cleared"), "false").unwrap();
        Arc::new(JiraAuth::new("https://test.atlassian.net", token_dir))
    }

    fn disconnected_state(site: &str) -> JiraState {
        let token_dir = tempfile::tempdir().unwrap().keep();
        JiraState {
            sync: None,
            writeback: None,
            auth: Arc::new(JiraAuth::new(site, token_dir)),
            repo: Arc::new(JiraRepository::new(Connection::open_in_memory().unwrap()).unwrap()),
            cancel: None,
            sync_config: None,
            activating: false,
        }
    }

    #[tokio::test]
    async fn disconnect_during_listener_installation_invalidates_connect_reservation() {
        let auth = active_auth();
        let state = JiraState {
            sync: None,
            writeback: None,
            auth: auth.clone(),
            repo: Arc::new(JiraRepository::new(Connection::open_in_memory().unwrap()).unwrap()),
            cancel: None,
            sync_config: None,
            activating: false,
        };
        let handle = JiraHandle(std::sync::Arc::new(Mutex::new(JiraSlot {
            state: Some(state),
            connecting: true,
            listener_attached: false,
        })));

        // Force listener installation to await the handle mutex, as jira_connect does.
        let slot = handle.0.lock().await;
        let installing_handle = handle.clone();
        let installing = tokio::spawn(async move {
            installing_handle
                .install_runtime_deactivation_listener()
                .await;
        });
        tokio::task::yield_now().await;
        assert!(!installing.is_finished());

        // A disconnect clears the reservation while installation was waiting.
        drop(slot);
        {
            let mut slot = handle.0.lock().await;
            slot.connecting = false;
        }
        installing.await.unwrap();

        assert!(handle.confirm_connect_reservation(&auth).await.is_err());
    }

    #[test]
    fn disconnected_site_change_replaces_auth_before_oauth_but_same_site_reuses_it() {
        let old_site = "https://old.atlassian.net";
        let new_site = "https://new.atlassian.net";
        let old_state = disconnected_state(old_site);
        let old_auth = old_state.auth.clone();
        let mut changed_site_slot = JiraSlot {
            state: Some(old_state),
            connecting: false,
            listener_attached: true,
        };

        let replaced = reserve_jira_connect(
            &mut changed_site_slot,
            &JiraConfig {
                site: new_site.to_string(),
                sync_interval_ms: 60_000,
                sources: HashMap::new(),
            },
        )
        .unwrap();

        assert!(
            replaced.is_none(),
            "a new state must be constructed before OAuth"
        );
        assert!(changed_site_slot.state.is_none());
        assert!(!changed_site_slot.listener_attached);
        assert!(changed_site_slot.connecting);
        assert_eq!(old_auth.site(), old_site);

        let same_site_state = disconnected_state(old_site);
        let same_site_auth = same_site_state.auth.clone();
        let mut same_site_slot = JiraSlot {
            state: Some(same_site_state),
            connecting: false,
            listener_attached: true,
        };
        let reused = reserve_jira_connect(
            &mut same_site_slot,
            &JiraConfig {
                site: old_site.to_string(),
                sync_interval_ms: 60_000,
                sources: HashMap::new(),
            },
        )
        .unwrap()
        .expect("same-site reconnect should retain the existing auth");

        assert!(Arc::ptr_eq(&reused, &same_site_auth));
        assert!(same_site_slot.state.is_some());
        assert!(same_site_slot.listener_attached);
    }

    #[tokio::test]
    async fn changed_config_then_manual_sync_cancels_and_replaces_stale_runtime() {
        let auth = active_auth();
        let repo = Arc::new(JiraRepository::new(Connection::open_in_memory().unwrap()).unwrap());
        let old_config = JiraConfig {
            site: "https://test.atlassian.net".to_string(),
            sync_interval_ms: 60_000,
            sources: HashMap::new(),
        };
        let current_config = JiraConfig {
            sync_interval_ms: 30_000,
            ..old_config.clone()
        };
        let client = Arc::new(JiraClient::new(auth.clone(), "cloud".to_string()));
        let stale_sync = Arc::new(JiraSync::new(
            client.clone(),
            repo.clone(),
            Arc::new(planeai_tasks::sqlite::SqliteRepository::open_in_memory("TST").unwrap()),
            old_config.clone(),
        ));
        let stale_cancel = CancellationToken::new();
        let mut state = JiraState {
            sync: Some(stale_sync),
            writeback: Some(Arc::new(JiraWriteback::new(client))),
            auth,
            repo,
            cancel: Some(stale_cancel.clone()),
            sync_config: Some(old_config),
            activating: false,
        };

        let selection = select_manual_sync_runtime(&mut state, &current_config).unwrap();

        assert!(matches!(
            selection,
            ManualSyncRuntime::Activate {
                restart_background: true,
                ..
            }
        ));
        assert!(stale_cancel.is_cancelled());
        assert!(state.sync.is_none());
        assert!(state.writeback.is_none());
        assert!(state.cancel.is_none());
        assert!(state.sync_config.is_none());
        assert!(state.activating);
    }
}

#[cfg(test)]
mod manual_sync_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    use planeai_jira::auth::JiraAuth;
    use planeai_jira::client::JiraClient;
    use planeai_jira::config::JiraConfig;
    use planeai_jira::repository::JiraRepository;
    use planeai_jira::JiraSync;
    use rusqlite::Connection;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn manual_sync_handoff_honors_disconnect_cancellation() {
        let token_dir = tempfile::tempdir().unwrap();
        std::fs::write(token_dir.path().join("refresh_token"), "refresh").unwrap();
        std::fs::write(token_dir.path().join("cloud_id"), "cloud").unwrap();
        std::fs::write(token_dir.path().join("connection_cleared"), "false").unwrap();
        let auth = Arc::new(JiraAuth::new(
            "https://test.atlassian.net",
            token_dir.path().to_path_buf(),
        ));
        let sync = Arc::new(JiraSync::new(
            Arc::new(JiraClient::new(auth, "cloud".to_string())),
            Arc::new(JiraRepository::new(Connection::open_in_memory().unwrap()).unwrap()),
            Arc::new(planeai_tasks::sqlite::SqliteRepository::open_in_memory("TST").unwrap()),
            JiraConfig {
                site: "https://test.atlassian.net".to_string(),
                sync_interval_ms: 60_000,
                sources: HashMap::new(),
            },
        ));
        let cancel = CancellationToken::new();
        cancel.cancel();

        assert!(run_manual_sync(sync, cancel).await.is_err());
    }
}
