use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use planeai_tasks::model::{CreateParams, ListFilter, Status, UpdateParams, DEFAULT_BASE_BRANCH};
use planeai_tasks::provider::TaskProvider;
use planeai_tasks::sqlite::SqliteRepository;

use crate::db;
use crate::plugins::PluginRuntimeHandle;
use crate::state::{ConfigState, DbState, PtyState};
use crate::task_lifecycle::{
    StatusChangeCause, TaskLifecycleBatch, TaskLifecycleEvent, TaskLifecycleOrigin,
};

use crate::commands::pr::poll_pr_for_session;
use crate::commands::sessions::helpers::{fire_task_hook, session_cwd};

/// Task structure returned to the frontend. Matches the original contract + parent_key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskItem {
    pub key: String,
    pub title: String,
    pub status: String,
    pub description: String,
    pub priority: i32,
    pub blocked_by: Vec<String>,
    pub tags: Vec<String>,
    #[serde(default)]
    pub parent_key: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    pub base_branch: String,
}

impl From<planeai_tasks::model::Task> for TaskItem {
    fn from(t: planeai_tasks::model::Task) -> Self {
        Self {
            key: t.key,
            title: t.title,
            status: t.status.as_str().to_string(),
            description: t.description,
            priority: t.priority,
            blocked_by: t.blocked_by,
            tags: t.tags,
            parent_key: t.parent_key,
            url: None,
            base_branch: t.base_branch,
        }
    }
}

fn resolve_project(
    db_state: &State<DbState>,
    repo_path: &str,
) -> Result<planeai_core::services::Project, String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    db::list_projects(&conn)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|project| crate::util::is_project_path_or_descendant(repo_path, &project.path))
        .ok_or_else(|| format!("no project found for path: {repo_path}"))
}

fn resolve_repo(db_state: &State<DbState>, repo_path: &str) -> Result<SqliteRepository, String> {
    let project = resolve_project(db_state, repo_path)?;
    let db_path = planeai_paths::db_path();
    SqliteRepository::open(db_path.to_str().unwrap(), &project.prefix).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_task_details(
    db_state: State<DbState>,
    key: String,
    repo_path: String,
) -> Result<TaskItem, String> {
    tracing::info!(key = %key, "get_task_details");
    let repo = resolve_repo(&db_state, &repo_path)?;
    repo.get(&key)
        .map(TaskItem::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_task_items(
    db_state: State<DbState>,
    repo_path: String,
) -> Result<Vec<TaskItem>, String> {
    let repo = resolve_repo(&db_state, &repo_path)?;
    let filter = ListFilter {
        exclude_status: Some(Status::Done),
        ..Default::default()
    };
    repo.list(filter)
        .map(|tasks| tasks.into_iter().map(TaskItem::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_all_task_items(
    db_state: State<DbState>,
    repo_path: String,
) -> Result<Vec<TaskItem>, String> {
    let repo = resolve_repo(&db_state, &repo_path)?;
    repo.list(ListFilter::default())
        .map(|tasks| tasks.into_iter().map(TaskItem::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn create_task_item(
    db_state: State<DbState>,
    runtime: State<PluginRuntimeHandle>,
    repo_path: String,
    title: String,
    description: String,
    priority: i32,
    tags: Vec<String>,
    blocked_by: Vec<String>,
    parent_key: Option<String>,
    base_branch: Option<String>,
) -> Result<TaskItem, String> {
    tracing::info!(title = %title, "create_task_item");
    let project = resolve_project(&db_state, &repo_path)?;
    let repo = resolve_repo(&db_state, &repo_path)?;
    let (task, first_child_assignment) = repo
        .create_with_first_child_assignment(CreateParams {
            key: None,
            title,
            description,
            status: None,
            priority,
            tags,
            blocked_by,
            parent_key,
            base_branch: base_branch.unwrap_or_else(|| DEFAULT_BASE_BRANCH.to_string()),
        })
        .map_err(|e| e.to_string())?;

    if let (Some(parent_key), Some(is_first_child_assignment)) =
        (&task.parent_key, first_child_assignment)
    {
        runtime.0.dispatch_task_lifecycle(TaskLifecycleBatch::new(
            TaskLifecycleOrigin::Ui,
            project.id,
            project.prefix,
            vec![TaskLifecycleEvent::ChildAssigned {
                child_key: task.key.clone(),
                parent_key: parent_key.clone(),
                is_first_child_assignment,
            }],
        ));
    }
    Ok(TaskItem::from(task))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn edit_task_item(
    db_state: State<DbState>,
    runtime: State<PluginRuntimeHandle>,
    repo_path: String,
    key: String,
    title: Option<String>,
    description: Option<String>,
    priority: Option<i32>,
    tags: Option<Vec<String>>,
    blocked_by: Option<Vec<String>>,
    // Frontend sends: undefined/absent = don't touch, null = clear, "KEY" = set
    // Tauri deserializes all of these as Option<String> (absent→None, null→None, "KEY"→Some("KEY"))
    // So we use a separate bool flag to distinguish "clear" from "don't touch"
    parent_key: Option<String>,
    clear_parent: Option<bool>,
    base_branch: Option<String>,
) -> Result<TaskItem, String> {
    let project = resolve_project(&db_state, &repo_path)?;
    let repo = resolve_repo(&db_state, &repo_path)?;
    let before = repo.get(&key).map_err(|e| e.to_string())?;
    let resolved_parent_key = if clear_parent.unwrap_or(false) {
        Some(None)
    } else {
        parent_key.map(Some)
    };
    let task = repo
        .update(
            &key,
            UpdateParams {
                title,
                description,
                priority,
                tags,
                blocked_by,
                parent_key: resolved_parent_key,
                base_branch,
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
    if task.parent_key != before.parent_key {
        if let Some(parent_key) = &task.parent_key {
            let child_count = repo
                .list(ListFilter {
                    parent_key: Some(Some(parent_key.clone())),
                    ..Default::default()
                })
                .map_err(|e| e.to_string())?
                .len();
            runtime.0.dispatch_task_lifecycle(TaskLifecycleBatch::new(
                TaskLifecycleOrigin::Ui,
                project.id,
                project.prefix,
                vec![TaskLifecycleEvent::ChildAssigned {
                    child_key: task.key.clone(),
                    parent_key: parent_key.clone(),
                    is_first_child_assignment: child_count == 1,
                }],
            ));
        }
    }
    Ok(TaskItem::from(task))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn move_task_item(
    db_state: State<'_, DbState>,
    config_state: State<'_, ConfigState>,
    pty_state: State<'_, PtyState>,
    runtime: State<'_, PluginRuntimeHandle>,
    app: AppHandle,
    key: String,
    status: String,
    repo_path: String,
) -> Result<(), String> {
    tracing::info!(key = %key, status = %status, "move_task_item");
    let s = Status::parse(&status).ok_or_else(|| format!("invalid status: {status}"))?;
    let project = resolve_project(&db_state, &repo_path)?;

    let db = db_state.0.clone();
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?.clone();

    // All I/O (DB writes, subprocess kills) runs off the main thread.
    let (archived_session_ids, events) = super::blocking({
        let key = key.clone();
        let cfg = cfg.clone();
        move || {
            let repo = {
                let conn = db.lock().map_err(|e| e.to_string())?;
                let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
                let project = projects
                    .iter()
                    .find(|p| crate::util::is_project_path_or_descendant(&repo_path, &p.path))
                    .ok_or_else(|| format!("no project found for path: {repo_path}"))?;
                let prefix = project.prefix.clone();
                drop(conn);
                let db_path = planeai_paths::db_path();
                SqliteRepository::open(db_path.to_str().unwrap(), &prefix)
                    .map_err(|e| e.to_string())?
            };

            let previous = repo.get(&key).map_err(|e| e.to_string())?;
            let task = repo
                .update(
                    &key,
                    UpdateParams {
                        status: Some(s),
                        ..Default::default()
                    },
                )
                .map_err(|e| e.to_string())?;
            let mut events = Vec::new();
            if previous.status != task.status {
                events.push(TaskLifecycleEvent::StatusChanged {
                    task_key: task.key.clone(),
                    parent_key: task.parent_key.clone(),
                    previous_status: previous.status.as_str().to_string(),
                    new_status: task.status.as_str().to_string(),
                    cause: StatusChangeCause::Direct,
                });
            }

            let mut session_ids: Vec<String> = Vec::new();

            if s == Status::Done {
                let parent_before = task
                    .parent_key
                    .as_deref()
                    .and_then(|parent_key| repo.get(parent_key).ok());
                if let Some(parent_key) = planeai_tasks::try_auto_complete_parent(&repo, &task) {
                    tracing::info!(parent_key = %parent_key, "auto-completed parent task");
                    if let (Some(before), Ok(parent)) = (parent_before, repo.get(&parent_key)) {
                        events.push(TaskLifecycleEvent::StatusChanged {
                            task_key: parent.key,
                            parent_key: parent.parent_key,
                            previous_status: before.status.as_str().to_string(),
                            new_status: parent.status.as_str().to_string(),
                            cause: StatusChangeCause::AutomaticParentCompletion,
                        });
                    }
                }

                // Archive sessions linked to this task
                let conn = db.lock().map_err(|e| e.to_string())?;
                session_ids = planeai_core::services::SessionService::list_by_task_key(&conn, &key)
                    .unwrap_or_default()
                    .iter()
                    .map(|s| s.id.clone())
                    .collect();
                crate::session_ops::archive_sessions_for_task(&conn, &key, &Some(cfg));
            }

            Ok((session_ids, events))
        }
    })
    .await?;

    if !events.is_empty() {
        runtime.0.dispatch_task_lifecycle(TaskLifecycleBatch::new(
            TaskLifecycleOrigin::Ui,
            project.id,
            project.prefix,
            events,
        ));
    }

    // Detach PTYs (PtyManager is !Send, must stay on main thread)
    for id in &archived_session_ids {
        pty_state.0.detach(id);
    }

    if !archived_session_ids.is_empty() {
        let _ = app.emit("sessions-changed", ());
    }

    Ok(())
}

#[tauri::command]
pub async fn fire_task_notify_hook(
    session_id: String,
    db_state: State<'_, DbState>,
    config_state: State<'_, ConfigState>,
) -> Result<(), String> {
    let db = db_state.0.clone();
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?.clone();
    super::blocking(move || {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let session = db::get_session(&conn, &session_id)
            .map_err(|e| e.to_string())?
            .ok_or("session not found")?;
        if session.task_key.is_some() {
            if let Some(cwd) = session_cwd(&conn, &session) {
                fire_task_hook(&cfg, &session, "on_notify", &cwd, &conn);
            }
        }
        poll_pr_for_session(&conn, &cfg, &session)?;
        Ok(())
    })
    .await
}
