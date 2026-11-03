//! AXI task subcommands — ls, show, add, move.

use std::path::Path;

use planeai_core::task_lifecycle::TaskLifecycleOrigin;
use planeai_tasks::model::{ListFilter, Status};
use planeai_tasks::provider::TaskProvider;
use planeai_tasks::sqlite::SqliteRepository;
use planeai_toon::{field, render, str_val, Value};

use super::helpers::{emit_error, task_detail_object};
use crate::task_cli::{LifecycleSink, TaskLifecycleContext};

/// One project's tasks, with the context their lifecycle changes are reported in.
pub struct TaskScope {
    pub repo: SqliteRepository,
    pub lifecycle: TaskLifecycleContext,
}

impl TaskScope {
    /// Open the database, migrating its task tables first, as a process outside the app must.
    pub fn open(
        db_path: &Path,
        project: &crate::db::Project,
        origin: TaskLifecycleOrigin,
        sink: LifecycleSink,
    ) -> Result<Self, String> {
        let db_path = db_path.to_str().ok_or("database path is not UTF-8")?;
        let repo = SqliteRepository::open(db_path, &project.prefix).map_err(|e| e.to_string())?;
        Ok(Self::new(repo, project, origin, sink))
    }

    /// Use a connection to the app's database, which it migrated at startup.
    pub fn on_migrated(
        conn: rusqlite::Connection,
        project: &crate::db::Project,
        origin: TaskLifecycleOrigin,
        sink: LifecycleSink,
    ) -> Self {
        Self::new(
            SqliteRepository::on_migrated(conn, &project.prefix),
            project,
            origin,
            sink,
        )
    }

    fn new(
        repo: SqliteRepository,
        project: &crate::db::Project,
        origin: TaskLifecycleOrigin,
        sink: LifecycleSink,
    ) -> Self {
        Self {
            repo,
            lifecycle: TaskLifecycleContext {
                origin,
                project_id: project.id.clone(),
                project_prefix: project.prefix.clone(),
                sink,
            },
        }
    }
}

pub fn task_ls(repo: &dyn TaskProvider, status: Option<&str>, tags: &[String]) -> (String, i32) {
    let filter = ListFilter {
        status: status.and_then(Status::parse),
        tags: tags.to_vec(),
        ..Default::default()
    };
    let tasks = match repo.list(filter) {
        Ok(t) => t,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    if tasks.is_empty() {
        let msg = match status {
            Some(s) => format!("0 {} tasks found", s),
            None => "0 tasks found".to_string(),
        };
        let fields = vec![
            field("tasks", str_val(&msg)),
            field(
                "help",
                Value::List(vec![
                    "Run `planeai-cli axi task add \"<title>\"` to create a task".into(),
                ]),
            ),
        ];
        return (render(&fields), 0);
    }

    let total = tasks.len();
    let rows: Vec<Vec<String>> = tasks
        .iter()
        .map(|t| {
            vec![
                t.key.clone(),
                t.title.clone(),
                t.status.as_str().to_string(),
                t.priority.to_string(),
                if t.tags.is_empty() {
                    String::new()
                } else {
                    t.tags.join(";")
                },
                if t.blocked_by.is_empty() {
                    String::new()
                } else {
                    t.blocked_by.join(";")
                },
            ]
        })
        .collect();

    let count_msg = if status.is_some() {
        format!("{} matching", rows.len())
    } else {
        format!("{} total", total)
    };

    let fields = vec![
        field("count", str_val(&count_msg)),
        field(
            "tasks",
            Value::Table {
                columns: vec![
                    "key".into(),
                    "title".into(),
                    "status".into(),
                    "priority".into(),
                    "tags".into(),
                    "blocked_by".into(),
                ],
                rows,
            },
        ),
        field(
            "help",
            Value::List(vec![
                "Run `planeai-cli axi task show <key>` for task details".into(),
                "Run `planeai-cli axi task move <key> <status>` to change status".into(),
            ]),
        ),
    ];
    (render(&fields), 0)
}

pub fn task_show(repo: &dyn TaskProvider, key: &str) -> (String, i32) {
    let task = match repo.get(key) {
        Ok(t) => t,
        Err(e) => {
            return (
                emit_error(
                    &e.to_string(),
                    &["Run `planeai-cli axi task ls` to see available tasks".to_string()],
                ),
                1,
            )
        }
    };
    (render(&[field("task", task_detail_object(&task))]), 0)
}

pub fn task_add(repo: &dyn TaskProvider, params: crate::task_cli::AddParams) -> (String, i32) {
    use planeai_tasks::model::{CreateParams, DEFAULT_BASE_BRANCH};
    let task = match repo.create(CreateParams {
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
    }) {
        Ok(t) => t,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    let help = vec![format!(
        "Run `planeai-cli axi task move {} in_progress` to start working on it",
        task.key
    )];
    let fields = vec![
        field("task", task_detail_object(&task)),
        field("help", Value::List(help)),
    ];
    (render(&fields), 0)
}

pub fn task_move(repo: &dyn TaskProvider, key: &str, status: &str) -> (String, i32) {
    let s = match Status::parse(status) {
        Some(s) => s,
        None => {
            return (
                emit_error(
                    &format!("invalid status: {status}"),
                    &["Valid statuses: todo, in_progress, in_review, done".into()],
                ),
                1,
            )
        }
    };

    // Check for idempotent no-op
    let existing = match repo.get(key) {
        Ok(t) => t,
        Err(e) => {
            return (
                emit_error(
                    &e.to_string(),
                    &["Run `planeai-cli axi task ls` to see available tasks".to_string()],
                ),
                1,
            )
        }
    };

    if existing.status == s {
        let mut fields = vec![field("task", task_detail_object(&existing))];
        fields.push(field(
            "note",
            str_val("already in requested status (no-op)"),
        ));
        return (render(&fields), 0);
    }

    let task = match repo.update(
        key,
        planeai_tasks::model::UpdateParams {
            status: Some(s),
            ..Default::default()
        },
    ) {
        Ok(t) => t,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };
    (render(&[field("task", task_detail_object(&task))]), 0)
}

/// AXI adapter for the canonical lifecycle-producing task creation path.
pub fn task_add_with_lifecycle(
    repo: &dyn TaskProvider,
    params: crate::task_cli::AddParams,
    context: &crate::task_cli::TaskLifecycleContext,
) -> (String, i32) {
    match crate::task_cli::run_task_add_with_lifecycle(repo, params, context)
        .and_then(|json| serde_json::from_str(&json).map_err(|error| error.to_string()))
    {
        Ok(task) => {
            let fields = vec![
                field("task", task_detail_object(&task)),
                field(
                    "help",
                    Value::List(vec![format!(
                        "Run `planeai-cli axi task move {} in_progress` to start working on it",
                        task.key
                    )]),
                ),
            ];
            (render(&fields), 0)
        }
        Err(error) => (emit_error(&error, &[]), 1),
    }
}

/// AXI adapter for the canonical lifecycle-producing status move path.
pub fn task_move_with_lifecycle(
    repo: &dyn TaskProvider,
    key: &str,
    status: &str,
    context: &crate::task_cli::TaskLifecycleContext,
) -> (String, i32) {
    task_detail_result(crate::task_cli::run_task_move_with_lifecycle(
        repo, key, status, context,
    ))
}

/// AXI adapter for `task snooze`; `prefix` scopes the write to `repo`'s project.
pub fn task_snooze(
    conn: &rusqlite::Connection,
    repo: &dyn TaskProvider,
    prefix: &str,
    key: &str,
    until: &str,
) -> (String, i32) {
    task_detail_result(crate::task_cli::run_task_snooze(
        conn, repo, prefix, key, until,
    ))
}

pub fn task_wake(
    conn: &rusqlite::Connection,
    repo: &dyn TaskProvider,
    prefix: &str,
    key: &str,
) -> (String, i32) {
    task_detail_result(crate::task_cli::run_task_wake(conn, repo, prefix, key))
}

fn task_detail_result(json: Result<String, String>) -> (String, i32) {
    match json.and_then(|json| serde_json::from_str(&json).map_err(|error| error.to_string())) {
        Ok(task) => (render(&[field("task", task_detail_object(&task))]), 0),
        Err(error) => (emit_error(&error, &[]), 1),
    }
}

/// Field edits and a status move for one task; `None` leaves a field as it is.
#[derive(Default)]
pub struct TaskUpdate<'a> {
    pub key: &'a str,
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub priority: Option<i32>,
    pub tags: Option<&'a [String]>,
    pub blocked_by: Option<&'a [String]>,
    /// `Some(None)` clears the parent.
    pub parent: Option<Option<&'a str>>,
    pub base_branch: Option<&'a str>,
    pub status: Option<&'a str>,
}

/// Apply the edits, then the move, each through its lifecycle-producing path. Everything
/// that can be refused is checked first, so a refused update writes nothing.
pub fn task_update(scope: &TaskScope, update: TaskUpdate) -> (String, i32) {
    if let Some(status) = update.status {
        if Status::parse(status).is_none() {
            return (
                emit_error(
                    &format!("invalid status: {status}"),
                    &["Valid statuses: todo, in_progress, in_review, done".into()],
                ),
                1,
            );
        }
    }
    let edits = update.title.is_some()
        || update.description.is_some()
        || update.priority.is_some()
        || update.tags.is_some()
        || update.blocked_by.is_some()
        || update.parent.is_some()
        || update.base_branch.is_some();
    if !edits && update.status.is_none() {
        return (emit_error("nothing to update", &[]), 1);
    }
    if let Err(error) = scope.repo.get(update.key) {
        return (emit_error(&error.to_string(), &[]), 1);
    }
    if let Some(Some(parent)) = update.parent {
        if let Err(error) = check_parent(&scope.repo, update.key, parent) {
            return (emit_error(&error, &[]), 1);
        }
    }
    for blocker in update.blocked_by.unwrap_or_default() {
        if scope.repo.get(blocker).is_err() {
            return (
                emit_error(&format!("blocking task not found: {blocker}"), &[]),
                1,
            );
        }
    }
    if edits {
        if let Err(error) = crate::task_cli::run_task_edit_with_lifecycle(
            &scope.repo,
            crate::task_cli::EditParams {
                key: update.key,
                title: update.title,
                description: update.description,
                priority: update.priority,
                tags: update.tags,
                blocked_by: update.blocked_by,
                parent: update.parent,
                base_branch: update.base_branch,
            },
            &scope.lifecycle,
        ) {
            return (emit_error(&error, &[]), 1);
        }
    }
    if let Some(status) = update.status {
        if let Err(error) = crate::task_cli::run_task_move_with_lifecycle(
            &scope.repo,
            update.key,
            status,
            &scope.lifecycle,
        ) {
            return (emit_error(&error, &[]), 1);
        }
    }
    match scope.repo.get(update.key) {
        Ok(task) => (render(&[field("task", task_detail_object(&task))]), 0),
        Err(error) => (emit_error(&error.to_string(), &[]), 1),
    }
}

/// `parent` exists and is neither `key` nor one of its subtasks.
fn check_parent(repo: &dyn TaskProvider, key: &str, parent: &str) -> Result<(), String> {
    if parent == key {
        return Err("a task cannot be its own parent".into());
    }
    let mut ancestor = repo
        .get(parent)
        .map_err(|_| format!("parent task not found: {parent}"))?;
    let mut seen = std::collections::HashSet::new();
    while let Some(next) = ancestor.parent_key.clone() {
        if next == key {
            return Err(format!(
                "{parent} is a subtask of {key}, so it cannot be its parent"
            ));
        }
        if !seen.insert(next.clone()) {
            break;
        }
        match repo.get(&next) {
            Ok(task) => ancestor = task,
            Err(_) => break,
        }
    }
    Ok(())
}

/// Delete a task and the task workspace it owned.
pub fn task_delete(scope: &TaskScope, key: &str) -> (String, i32) {
    // A mistyped key must not reach the workspace teardown.
    if let Err(error) = scope.repo.get(key) {
        return (emit_error(&error.to_string(), &[]), 1);
    }
    match crate::task_cli::run_task_delete_in_project(
        &scope.repo,
        key,
        Some(&scope.lifecycle.project_id),
    ) {
        Ok(_) => (render(&[field("deleted", str_val(key))]), 0),
        Err(error) => (emit_error(&error, &[]), 1),
    }
}
