use serde::{Deserialize, Serialize};
use tauri::State;

use planeai_tasks::model::{CreateParams, ListFilter, Status, UpdateParams, DEFAULT_BASE_BRANCH};
use planeai_tasks::provider::TaskProvider;
use planeai_tasks::sqlite::SqliteRepository;

use crate::db;
use crate::state::{ConfigState, DbState};

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

fn resolve_repo(db_state: &State<DbState>, repo_path: &str) -> Result<SqliteRepository, String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
    let project = projects
        .iter()
        .find(|p| p.path == repo_path || repo_path.starts_with(&p.path))
        .ok_or_else(|| format!("no project found for path: {repo_path}"))?;
    let prefix = project.prefix.clone();
    drop(conn);
    let db_path = planeai_paths::db_path();
    SqliteRepository::open(db_path.to_str().unwrap(), &prefix).map_err(|e| e.to_string())
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
    repo_path: String,
    title: String,
    description: String,
    priority: i32,
    tags: Vec<String>,
    blocked_by: Vec<String>,
    base_branch: Option<String>,
) -> Result<TaskItem, String> {
    tracing::info!(title = %title, "create_task_item");
    let repo = resolve_repo(&db_state, &repo_path)?;
    repo.create(CreateParams {
        title,
        description,
        priority,
        tags,
        blocked_by,
        parent_key: None,
        base_branch: base_branch.unwrap_or_else(|| DEFAULT_BASE_BRANCH.to_string()),
    })
    .map(TaskItem::from)
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn edit_task_item(
    db_state: State<DbState>,
    repo_path: String,
    key: String,
    title: Option<String>,
    description: Option<String>,
    priority: Option<i32>,
    tags: Option<Vec<String>>,
    blocked_by: Option<Vec<String>>,
    parent_key: Option<Option<String>>,
    base_branch: Option<String>,
) -> Result<TaskItem, String> {
    let repo = resolve_repo(&db_state, &repo_path)?;
    repo.update(
        &key,
        UpdateParams {
            title,
            description,
            priority,
            tags,
            blocked_by,
            parent_key,
            base_branch,
            ..Default::default()
        },
    )
    .map(TaskItem::from)
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn move_task_item(
    db_state: State<DbState>,
    key: String,
    status: String,
    repo_path: String,
) -> Result<(), String> {
    tracing::info!(key = %key, status = %status, "move_task_item");
    let s = Status::parse(&status).ok_or_else(|| format!("invalid status: {status}"))?;
    let repo = resolve_repo(&db_state, &repo_path)?;
    repo.update(
        &key,
        UpdateParams {
            status: Some(s),
            ..Default::default()
        },
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn fire_task_notify_hook(
    session_id: String,
    db_state: State<DbState>,
    config_state: State<ConfigState>,
) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    if session.task_key.is_some() {
        if let Some(cwd) = session_cwd(&conn, &session) {
            fire_task_hook(&cfg, &session, "on_notify", &cwd, &conn);
        }
    }
    poll_pr_for_session(&conn, &cfg, &session)?;
    Ok(())
}
