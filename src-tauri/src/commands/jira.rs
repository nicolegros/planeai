use serde::Serialize;
use tauri::State;
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

pub struct JiraHandle(pub Mutex<Option<JiraState>>);

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

#[tauri::command]
pub async fn jira_connect(
    app: tauri::AppHandle,
    jira: State<'_, JiraHandle>,
    config_state: State<'_, ConfigState>,
) -> Result<(), String> {
    let mut guard = jira.0.lock().await;

    let jira_config = {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        *guard = crate::jira::init_jira(&cfg, app.clone());
        cfg.integrations
            .as_ref()
            .and_then(|i| i.jira.clone())
            .ok_or("jira not configured — add a site URL first")?
    };

    let state = guard.as_mut().ok_or("jira not configured")?;
    state.auth.connect().await.map_err(|e| e.to_string())?;

    let cancel = state.activate(&jira_config, app)?;
    let sync = state.sync.clone().unwrap();
    tokio::spawn(async move { sync.start(cancel).await });

    tracing::info!("jira: connected");
    Ok(())
}

#[tauri::command]
pub async fn jira_disconnect(jira: State<'_, JiraHandle>) -> Result<(), String> {
    let mut guard = jira.0.lock().await;
    let state = guard.as_mut().ok_or("jira not configured")?;
    state.deactivate();
    state.auth.disconnect().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn jira_sync_now(
    app: tauri::AppHandle,
    jira: State<'_, JiraHandle>,
    config_state: State<'_, ConfigState>,
) -> Result<SyncResult, String> {
    let mut guard = jira.0.lock().await;
    let state = guard.as_mut().ok_or("jira not configured")?;

    let jira_config = get_jira_config(&config_state)?;

    let _ = state.activate(&jira_config, app);
    let sync = state.sync.clone().ok_or("jira not connected")?;
    drop(guard);

    sync.sync_now().await.map_err(|e| e.to_string())
}

/// Read-only status check. Does not activate sync or mutate state.
#[tauri::command]
pub async fn jira_status(
    jira: State<'_, JiraHandle>,
    config_state: State<'_, ConfigState>,
) -> Result<JiraStatusResponse, String> {
    let guard = jira.0.lock().await;

    let connected = guard
        .as_ref()
        .map(|s| s.auth.is_connected())
        .unwrap_or(false);

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
        if let Some(state) = guard.as_ref() {
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
            if let Some(state) = guard.as_ref() {
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
