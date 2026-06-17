use rusqlite::Connection;

use planeai_tasks::model::{CreateParams, ListFilter, Status, UpdateParams};
use planeai_tasks::provider::TaskProvider;
use planeai_tasks::sqlite::derive_prefix;

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
    Ok(derive_prefix(&proj.name))
}

pub fn run_task_add(
    repo: &dyn TaskProvider,
    title: &str,
    description: &str,
    priority: i32,
    tags: &[String],
    blocked_by: &[String],
    parent: Option<&str>,
) -> Result<String, String> {
    tracing::info!(title, priority, "creating task");
    let task = repo
        .create(CreateParams {
            title: title.to_string(),
            description: description.to_string(),
            status: None,
            priority,
            tags: tags.to_vec(),
            blocked_by: blocked_by.to_vec(),
            parent_key: parent.map(|s| s.to_string()),
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

pub fn run_task_edit(
    repo: &dyn TaskProvider,
    key: &str,
    title: Option<&str>,
    description: Option<&str>,
    priority: Option<i32>,
    tags: Option<&[String]>,
    blocked_by: Option<&[String]>,
) -> Result<String, String> {
    tracing::info!(key, "editing task");
    let task = repo
        .update(
            key,
            UpdateParams {
                title: title.map(|s| s.to_string()),
                description: description.map(|s| s.to_string()),
                priority,
                tags: tags.map(|t| t.to_vec()),
                blocked_by: blocked_by.map(|b| b.to_vec()),
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
    let app_dir = crate::paths::app_data_dir();
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
                status TEXT NOT NULL DEFAULT 'active'
            );
            INSERT INTO projects (id, name, path) VALUES ('p1', 'planeai', '/Users/dev/planeai');",
        )
        .unwrap();
        // Open a second in-memory conn for the task repo (since repo takes ownership)
        let repo = SqliteRepository::open_in_memory("PLA").unwrap();
        (conn, repo)
    }

    #[test]
    fn task_add_creates_and_returns_json() {
        let (_conn, repo) = setup();
        let result =
            run_task_add(&repo, "Fix bug", "desc", 1, &["backend".into()], &[], None).unwrap();
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
        run_task_add(&repo, "A task", "", 0, &[], &[], None).unwrap();
        let result = run_task_show(&repo, "PLA-1").unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["key"], "PLA-1");
        assert_eq!(v["title"], "A task");
    }

    #[test]
    fn task_list_returns_all_tasks() {
        let (_conn, repo) = setup();
        run_task_add(&repo, "first", "", 0, &[], &[], None).unwrap();
        run_task_add(&repo, "second", "", 0, &[], &[], None).unwrap();
        let result = run_task_list(&repo, None, &[]).unwrap();
        let v: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn task_list_filters_by_status() {
        let (_conn, repo) = setup();
        run_task_add(&repo, "a", "", 0, &[], &[], None).unwrap();
        run_task_add(&repo, "b", "", 0, &[], &[], None).unwrap();
        run_task_move(&repo, "PLA-1", "done").unwrap();
        let result = run_task_list(&repo, Some("todo"), &[]).unwrap();
        let v: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0]["key"], "PLA-2");
    }

    #[test]
    fn task_move_changes_status() {
        let (_conn, repo) = setup();
        run_task_add(&repo, "task", "", 0, &[], &[], None).unwrap();
        let result = run_task_move(&repo, "PLA-1", "in_progress").unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["status"], "in_progress");
    }

    #[test]
    fn task_move_invalid_status_errors() {
        let (_conn, repo) = setup();
        run_task_add(&repo, "task", "", 0, &[], &[], None).unwrap();
        let result = run_task_move(&repo, "PLA-1", "bogus");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid status"));
    }

    #[test]
    fn task_edit_updates_fields() {
        let (_conn, repo) = setup();
        run_task_add(&repo, "original", "", 0, &[], &[], None).unwrap();
        let result =
            run_task_edit(&repo, "PLA-1", Some("renamed"), None, Some(2), None, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["title"], "renamed");
        assert_eq!(v["priority"], 2);
    }

    #[test]
    fn task_delete_removes_task() {
        let (_conn, repo) = setup();
        run_task_add(&repo, "doomed", "", 0, &[], &[], None).unwrap();
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
}
