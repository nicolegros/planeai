//! AXI home subcommand — overview of project, tasks, and sessions.

use planeai_tasks::model::{ListFilter, Status, Task};
use planeai_tasks::provider::TaskProvider;
use planeai_toon::{field, render, str_val, Field, Value};

use crate::db;

pub fn home(conn: &rusqlite::Connection, cwd: &str, bin_path: &str) -> (String, i32) {
    let projects = db::list_projects(conn).unwrap_or_default();

    let project = projects.iter().find(|p| cwd.starts_with(&p.path));

    let mut fields: Vec<Field> = vec![
        field("bin", str_val(bin_path)),
        field(
            "description",
            str_val("Orchestrate parallel AI coding agents with persistent sessions"),
        ),
    ];

    let project = match project {
        Some(p) => p,
        None => {
            // No project resolved — show project list
            if projects.is_empty() {
                fields.push(field("projects", str_val("0 projects")));
            } else {
                let rows: Vec<Vec<String>> = projects
                    .iter()
                    .map(|p| vec![p.prefix.clone(), p.name.clone(), p.path.clone()])
                    .collect();
                fields.push(field(
                    "projects",
                    Value::Table {
                        columns: vec!["prefix".into(), "name".into(), "path".into()],
                        rows,
                    },
                ));
            }
            fields.push(field(
                "help",
                Value::List(vec![
                    "cd into a project directory to see tasks and sessions".into(),
                    "Run `planeai-cli axi project ls` for all projects".into(),
                ]),
            ));
            return (render(&fields), 0);
        }
    };

    fields.push(field(
        "project",
        Value::Object(vec![
            field("name", str_val(&project.name)),
            field("path", str_val(&project.path)),
            field("prefix", str_val(&project.prefix)),
        ]),
    ));

    // Tasks (non-done, up to 20)
    let db_path = planeai_paths::db_path();
    if let Ok(repo) = planeai_tasks::sqlite::SqliteRepository::open(
        db_path.to_str().unwrap_or_default(),
        &project.prefix,
    ) {
        let filter = ListFilter {
            exclude_status: Some(Status::Done),
            ..Default::default()
        };
        if let Ok(tasks) = repo.list(filter) {
            let capped: Vec<&Task> = tasks.iter().take(20).collect();
            if capped.is_empty() {
                fields.push(field("tasks", str_val("0 open tasks")));
            } else {
                let rows: Vec<Vec<String>> = capped
                    .iter()
                    .map(|t| {
                        vec![
                            t.key.clone(),
                            t.title.clone(),
                            t.status.as_str().to_string(),
                            t.priority.to_string(),
                        ]
                    })
                    .collect();
                fields.push(field(
                    "tasks",
                    Value::Table {
                        columns: vec![
                            "key".into(),
                            "title".into(),
                            "status".into(),
                            "priority".into(),
                        ],
                        rows,
                    },
                ));
            }
        }
    }

    // Sessions (active only)
    if let Ok(sessions) = crate::session_ops::list(conn, false) {
        let project_sessions: Vec<_> = sessions
            .iter()
            .filter(|s| s.project_id == project.id)
            .collect();
        if project_sessions.is_empty() {
            fields.push(field("sessions", str_val("0 active sessions")));
        } else {
            let rows: Vec<Vec<String>> = project_sessions
                .iter()
                .map(|s| {
                    vec![
                        s.id[..8].to_string(),
                        s.name.clone(),
                        s.status.clone(),
                        s.provider.clone().unwrap_or_default(),
                        s.branch.clone(),
                    ]
                })
                .collect();
            fields.push(field(
                "sessions",
                Value::Table {
                    columns: vec![
                        "id".into(),
                        "name".into(),
                        "status".into(),
                        "provider".into(),
                        "branch".into(),
                    ],
                    rows,
                },
            ));
        }
    }

    fields.push(field(
        "help",
        Value::List(vec![
            "Run `planeai-cli axi task ls` for full task list".into(),
            "Run `planeai-cli axi task show <key>` for task details".into(),
            "Run `planeai-cli axi session ls` for all sessions".into(),
        ]),
    ));

    (render(&fields), 0)
}
