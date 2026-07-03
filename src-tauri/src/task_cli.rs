use rusqlite::Connection;

use planeai_tasks::model::{CreateParams, ListFilter, Status, UpdateParams, DEFAULT_BASE_BRANCH};
use planeai_tasks::provider::TaskProvider;

use crate::db;

/// Resolve project prefix from --project name or CWD.
pub fn resolve_prefix(
    conn: &Connection,
    project: Option<&str>,
    cwd: &str,
) -> Result<String, String> {
    let projects = db::list_projects(conn).map_err(|e| e.to_string())?;
    let proj = if let Some(name) = project {
        projects
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| format!("unknown project: {name}"))?
    } else {
        projects
            .iter()
            .find(|p| cwd.starts_with(&p.path))
            .ok_or_else(|| "could not resolve project from CWD; use --project".to_string())?
    };
    Ok(proj.prefix.clone())
}

pub struct AddParams<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub priority: i32,
    pub tags: &'a [String],
    pub blocked_by: &'a [String],
    pub parent: Option<&'a str>,
    pub base_branch: Option<&'a str>,
}

pub fn run_task_add(repo: &dyn TaskProvider, params: AddParams) -> Result<String, String> {
    tracing::info!(params.title, params.priority, "creating task");
    let task = repo
        .create(CreateParams {
            key: None,
            title: params.title.to_string(),
            description: params.description.to_string(),
            status: None,
            priority: params.priority,
            tags: params.tags.to_vec(),
            blocked_by: params.blocked_by.to_vec(),
            parent_key: params.parent.map(|s| s.to_string()),
            base_branch: params
                .base_branch
                .unwrap_or(DEFAULT_BASE_BRANCH)
                .to_string(),
        })
        .map_err(|e| e.to_string())?;
    tracing::info!(key = %task.key, "task created");
    serde_json::to_string(&task).map_err(|e| e.to_string())
}

pub fn run_task_show(repo: &dyn TaskProvider, key: &str) -> Result<String, String> {
    let task = repo.get(key).map_err(|e| e.to_string())?;
    serde_json::to_string(&task).map_err(|e| e.to_string())
}

pub fn run_task_list(
    repo: &dyn TaskProvider,
    status: Option<&str>,
    tags: &[String],
) -> Result<String, String> {
    let filter = ListFilter {
        status: status.and_then(Status::parse),
        tags: tags.to_vec(),
        ..Default::default()
    };
    let tasks = repo.list(filter).map_err(|e| e.to_string())?;
    serde_json::to_string(&tasks).map_err(|e| e.to_string())
}

pub fn run_task_move(repo: &dyn TaskProvider, key: &str, status: &str) -> Result<String, String> {
    let s = Status::parse(status).ok_or_else(|| format!("invalid status: {status}"))?;
    tracing::info!(key, status, "moving task");
    let task = repo
        .update(
            key,
            UpdateParams {
                status: Some(s),
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
    serde_json::to_string(&task).map_err(|e| e.to_string())
}

pub struct EditParams<'a> {
    pub key: &'a str,
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub priority: Option<i32>,
    pub tags: Option<&'a [String]>,
    pub blocked_by: Option<&'a [String]>,
    pub parent: Option<Option<&'a str>>,
    pub base_branch: Option<&'a str>,
}

pub fn run_task_edit(repo: &dyn TaskProvider, params: EditParams) -> Result<String, String> {
    tracing::info!(params.key, "editing task");
    let task = repo
        .update(
            params.key,
            UpdateParams {
                title: params.title.map(|s| s.to_string()),
                description: params.description.map(|s| s.to_string()),
                priority: params.priority,
                tags: params.tags.map(|t| t.to_vec()),
                blocked_by: params.blocked_by.map(|b| b.to_vec()),
                parent_key: params.parent.map(|p| p.map(|s| s.to_string())),
                base_branch: params.base_branch.map(|s| s.to_string()),
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
    serde_json::to_string(&task).map_err(|e| e.to_string())
}

pub fn run_task_delete(repo: &dyn TaskProvider, key: &str) -> Result<String, String> {
    tracing::info!(key, "deleting task");
    repo.delete(key).map_err(|e| e.to_string())?;
    Ok(format!("{{\"deleted\":\"{key}\"}}"))
}

/// Send a task_changed event through the notify socket.
pub fn notify_task_changed(key: &str) {
    use crate::ipc::{self, Channel};
    let app_dir = planeai_paths::app_data_dir();
    if ipc::channel_exists(Channel::Notify, &app_dir) {
        if let Ok(mut stream) = ipc::connect(Channel::Notify, &app_dir) {
            use std::io::Write;
            let msg = format!("{{\"event\":\"task_changed\",\"key\":\"{key}\"}}\n");
            let _ = stream.write_all(msg.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use planeai_tasks::sqlite::SqliteRepository;

    fn setup() -> (Connection, SqliteRepository) {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                prefix TEXT NOT NULL DEFAULT ''
            );
            INSERT INTO projects (id, name, path, prefix) VALUES ('p1', 'planeai', '/Users/dev/planeai', 'PLA');",
        )
        .unwrap();
        // Open a second in-memory conn for the task repo (since repo takes ownership)
        let repo = SqliteRepository::open_in_memory("PLA").unwrap();
        (conn, repo)
    }

    fn add(repo: &dyn TaskProvider, title: &str) -> String {
        run_task_add(
            repo,
            AddParams {
                title,
                description: "",
                priority: 0,
                tags: &[],
                blocked_by: &[],
                parent: None,
                base_branch: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn task_add_creates_and_returns_json() {
        let (_conn, repo) = setup();
        let result = run_task_add(
            &repo,
            AddParams {
                title: "Fix bug",
                description: "desc",
                priority: 1,
                tags: &["backend".into()],
                blocked_by: &[],
                parent: None,
                base_branch: None,
            },
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["key"], "PLA-1");
        assert_eq!(v["title"], "Fix bug");
        assert_eq!(v["status"], "todo");
        assert_eq!(v["priority"], 1);
        assert_eq!(v["tags"], serde_json::json!(["backend"]));
    }

    #[test]
    fn task_show_returns_task() {
        let (_conn, repo) = setup();
        add(&repo, "A task");
        let result = run_task_show(&repo, "PLA-1").unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["key"], "PLA-1");
        assert_eq!(v["title"], "A task");
    }

    #[test]
    fn task_list_returns_all_tasks() {
        let (_conn, repo) = setup();
        add(&repo, "first");
        add(&repo, "second");
        let result = run_task_list(&repo, None, &[]).unwrap();
        let v: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn task_list_filters_by_status() {
        let (_conn, repo) = setup();
        add(&repo, "a");
        add(&repo, "b");
        run_task_move(&repo, "PLA-1", "done").unwrap();
        let result = run_task_list(&repo, Some("todo"), &[]).unwrap();
        let v: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0]["key"], "PLA-2");
    }

    #[test]
    fn task_move_changes_status() {
        let (_conn, repo) = setup();
        add(&repo, "task");
        let result = run_task_move(&repo, "PLA-1", "in_progress").unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["status"], "in_progress");
    }

    #[test]
    fn task_move_invalid_status_errors() {
        let (_conn, repo) = setup();
        add(&repo, "task");
        let result = run_task_move(&repo, "PLA-1", "bogus");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid status"));
    }

    #[test]
    fn task_edit_updates_fields() {
        let (_conn, repo) = setup();
        add(&repo, "original");
        let result = run_task_edit(
            &repo,
            EditParams {
                key: "PLA-1",
                title: Some("renamed"),
                description: None,
                priority: Some(2),
                tags: None,
                blocked_by: None,
                parent: None,
                base_branch: None,
            },
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["title"], "renamed");
        assert_eq!(v["priority"], 2);
    }

    #[test]
    fn task_delete_removes_task() {
        let (_conn, repo) = setup();
        add(&repo, "doomed");
        let result = run_task_delete(&repo, "PLA-1").unwrap();
        assert!(result.contains("PLA-1"));
        let show = run_task_show(&repo, "PLA-1");
        assert!(show.is_err());
    }

    #[test]
    fn resolve_prefix_from_project_name() {
        let (conn, _repo) = setup();
        let prefix = resolve_prefix(&conn, Some("planeai"), "/tmp").unwrap();
        assert_eq!(prefix, "PLA");
    }

    #[test]
    fn resolve_prefix_from_cwd() {
        let (conn, _repo) = setup();
        let prefix = resolve_prefix(&conn, None, "/Users/dev/planeai/src").unwrap();
        assert_eq!(prefix, "PLA");
    }

    #[test]
    fn resolve_prefix_fails_without_match() {
        let (conn, _repo) = setup();
        let result = resolve_prefix(&conn, None, "/some/other/path");
        assert!(result.is_err());
    }

    #[test]
    fn task_add_with_base_branch() {
        let (_conn, repo) = setup();
        let result = run_task_add(
            &repo,
            AddParams {
                title: "Feature",
                description: "",
                priority: 0,
                tags: &[],
                blocked_by: &[],
                parent: None,
                base_branch: Some("develop"),
            },
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["base_branch"], "develop");
    }

    #[test]
    fn task_add_without_base_branch_defaults_to_main() {
        let (_conn, repo) = setup();
        let result = add(&repo, "Feature");
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["base_branch"], "main");
    }

    #[test]
    fn task_edit_base_branch() {
        let (_conn, repo) = setup();
        add(&repo, "task");
        let result = run_task_edit(
            &repo,
            EditParams {
                key: "PLA-1",
                title: None,
                description: None,
                priority: None,
                tags: None,
                blocked_by: None,
                parent: None,
                base_branch: Some("release"),
            },
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["base_branch"], "release");
    }

    #[test]
    fn task_edit_parent() {
        let (_conn, repo) = setup();
        add(&repo, "parent task");
        add(&repo, "child task");
        // Set parent
        let result = run_task_edit(
            &repo,
            EditParams {
                key: "PLA-2",
                title: None,
                description: None,
                priority: None,
                tags: None,
                blocked_by: None,
                parent: Some(Some("PLA-1")),
                base_branch: None,
            },
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["parent_key"], "PLA-1");
        // Clear parent
        let result = run_task_edit(
            &repo,
            EditParams {
                key: "PLA-2",
                title: None,
                description: None,
                priority: None,
                tags: None,
                blocked_by: None,
                parent: Some(None),
                base_branch: None,
            },
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["parent_key"].is_null());
    }
}
