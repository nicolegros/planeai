//! AXI task subcommands — ls, show, add, move.

use planeai_tasks::model::{ListFilter, Status};
use planeai_tasks::provider::TaskProvider;
use planeai_toon::{field, render, str_val, Value};

use super::helpers::{emit_error, task_detail_object};

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
