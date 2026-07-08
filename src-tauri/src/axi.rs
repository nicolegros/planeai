//! AXI subcommand implementations — TOON output for agent consumption.

use planeai_tasks::model::{ListFilter, Status, Task, DEFAULT_BASE_BRANCH};
use planeai_tasks::provider::TaskProvider;
use planeai_toon::{field, int_val, render, str_val, Field, Value};

use crate::db;

// ─── Task ────────────────────────────────────────────────────────────────────

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
    use planeai_tasks::model::CreateParams;
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

// ─── Session ─────────────────────────────────────────────────────────────────

pub fn session_read_output(session_id: &str, text: &str) -> (String, i32) {
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let line_count = lines.len();
    let fields = vec![
        field("session_id", str_val(session_id)),
        field("lines", int_val(line_count as i64)),
        field("output", Value::List(lines)),
    ];
    (render(&fields), 0)
}

/// Render session read output with cursor information for incremental polling.
pub fn session_read_cursor_output(
    session_id: &str,
    backend: &str,
    cursor: &str,
    truncated: bool,
    text: &str,
) -> (String, i32) {
    let fields = vec![
        field("session_id", str_val(session_id)),
        field("backend", str_val(backend)),
        field("cursor", str_val(cursor)),
        field("truncated", Value::Bool(truncated)),
        field("text", str_val(text)),
    ];
    (render(&fields), 0)
}

pub fn session_create_output(session: &crate::db::Session) -> (String, i32) {
    let short_id = &session.id[..8];
    let mut session_fields = vec![
        field("id", str_val(&session.id)),
        field("name", str_val(&session.name)),
        field("status", str_val(&session.status)),
        field("branch", str_val(&session.branch)),
        field("backend", str_val(&session.backend)),
    ];
    if let Some(ref provider) = session.provider {
        session_fields.push(field("provider", str_val(provider)));
    }
    if let Some(ref wt) = session.worktree_path {
        session_fields.push(field("worktree_path", str_val(wt)));
    }
    if let Some(ref parent) = session.parent_session_id {
        session_fields.push(field("parent_session_id", str_val(parent)));
    }

    let fields = vec![
        field("session", Value::Object(session_fields)),
        field(
            "help",
            Value::List(vec![
                format!(
                    "Run `planeai-cli axi session prompt {short_id} \"<text>\"` to send a prompt"
                ),
                format!("Run `planeai-cli axi session read {short_id}` to read session output"),
            ]),
        ),
    ];
    (render(&fields), 0)
}

pub fn session_ls(conn: &rusqlite::Connection, archived: bool) -> (String, i32) {
    let sessions = match crate::session_ops::list(conn, archived) {
        Ok(s) => s,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    if sessions.is_empty() {
        let msg = if archived {
            "0 archived sessions"
        } else {
            "0 active sessions"
        };
        return (render(&[field("sessions", str_val(msg))]), 0);
    }

    let rows: Vec<Vec<String>> = sessions
        .iter()
        .map(|s| {
            vec![
                s.id[..8].to_string(),
                s.name.clone(),
                s.status.clone(),
                s.provider.clone().unwrap_or_default(),
                s.branch.clone(),
                s.task_key.clone().unwrap_or_default(),
                s.parent_session_id
                    .as_ref()
                    .map(|id| id[..8].to_string())
                    .unwrap_or_default(),
                s.worktree_path.clone().unwrap_or_default(),
                s.base_branch.clone().unwrap_or_default(),
                if s.auto_approve {
                    "true".into()
                } else {
                    "false".into()
                },
                s.created_at.clone(),
            ]
        })
        .collect();

    let fields = vec![
        field("count", str_val(&format!("{} total", sessions.len()))),
        field(
            "sessions",
            Value::Table {
                columns: vec![
                    "id".into(),
                    "name".into(),
                    "status".into(),
                    "provider".into(),
                    "branch".into(),
                    "task_key".into(),
                    "parent_session_id".into(),
                    "worktree_path".into(),
                    "base_branch".into(),
                    "yolo".into(),
                    "created_at".into(),
                ],
                rows,
            },
        ),
        field(
            "help",
            Value::List(vec![
                "Run `planeai-cli axi session prompt <id> \"<text>\"` to send a prompt".into(),
            ]),
        ),
    ];
    (render(&fields), 0)
}

pub fn session_prompt(
    conn: &rusqlite::Connection,
    id: &str,
    text: &str,
    ops: &dyn crate::session_ops::PromptOps,
) -> (String, i32) {
    match crate::session_ops::send_prompt(conn, id, text, ops) {
        Ok(result) => {
            let fields = vec![field(
                "prompt",
                Value::Object(vec![
                    field("status", str_val("sent")),
                    field("session_id", str_val(&result.session_id)),
                    field("backend", str_val(&result.backend)),
                ]),
            )];
            (render(&fields), 0)
        }
        Err(e) => {
            let help = if e.contains("already in progress") {
                vec!["retry after the current prompt is sent".into()]
            } else {
                vec![]
            };
            (emit_error(&e, &help), 1)
        }
    }
}

// ─── Session Children / Tree ─────────────────────────────────────────────────

pub fn session_children(conn: &rusqlite::Connection, id: &str) -> (String, i32) {
    use planeai_core::services::SessionService;

    let session = match crate::session_ops::resolve_session_by_prefix(conn, id) {
        Ok(s) => s,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    let children = match SessionService::children(conn, &session.id) {
        Ok(c) => c,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    if children.is_empty() {
        let fields = vec![
            field("parent_session_id", str_val(&session.id[..8])),
            field("children", str_val("0 children")),
        ];
        return (render(&fields), 0);
    }

    let rows: Vec<Vec<String>> = children.iter().map(session_tree_row).collect();

    let fields = vec![
        field("parent_session_id", str_val(&session.id[..8])),
        field(
            "children",
            Value::Table {
                columns: session_tree_columns(),
                rows,
            },
        ),
    ];
    (render(&fields), 0)
}

pub fn session_tree(conn: &rusqlite::Connection, id: &str) -> (String, i32) {
    use planeai_core::services::SessionService;

    let session = match crate::session_ops::resolve_session_by_prefix(conn, id) {
        Ok(s) => s,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    let tree = match SessionService::tree(conn, &session.id) {
        Ok(t) => t,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    let root_id = tree
        .first()
        .map(|s| s.id[..8].to_string())
        .unwrap_or_default();

    let rows: Vec<Vec<String>> = tree.iter().map(session_tree_row).collect();

    let fields = vec![
        field(
            "session_tree",
            Value::Object(vec![field("root", str_val(&root_id))]),
        ),
        field(
            "sessions",
            Value::Table {
                columns: session_tree_columns(),
                rows,
            },
        ),
    ];
    (render(&fields), 0)
}

fn session_tree_columns() -> Vec<String> {
    vec![
        "id".into(),
        "parent_session_id".into(),
        "name".into(),
        "status".into(),
        "provider".into(),
        "task_key".into(),
        "backend".into(),
    ]
}

fn session_tree_row(s: &planeai_core::services::SessionRecord) -> Vec<String> {
    vec![
        s.id[..8].to_string(),
        s.parent_session_id
            .as_ref()
            .map(|id| id[..8].to_string())
            .unwrap_or_default(),
        s.name.clone(),
        s.status.clone(),
        s.provider.clone().unwrap_or_default(),
        s.task_key.clone().unwrap_or_default(),
        s.backend.clone(),
    ]
}

// ─── Project ─────────────────────────────────────────────────────────────────

pub fn project_ls(conn: &rusqlite::Connection) -> (String, i32) {
    let projects = db::list_projects(conn).unwrap_or_default();

    if projects.is_empty() {
        return (
            render(&[
                field("projects", str_val("0 projects")),
                field(
                    "help",
                    Value::List(vec!["Create a project in the planeai GUI".into()]),
                ),
            ]),
            0,
        );
    }

    let rows: Vec<Vec<String>> = projects
        .iter()
        .map(|p| {
            vec![
                p.prefix.clone(),
                p.name.clone(),
                p.path.clone(),
                p.status.clone(),
            ]
        })
        .collect();

    let fields = vec![field(
        "projects",
        Value::Table {
            columns: vec![
                "prefix".into(),
                "name".into(),
                "path".into(),
                "status".into(),
            ],
            rows,
        },
    )];
    (render(&fields), 0)
}

// ─── Home ────────────────────────────────────────────────────────────────────

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

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn task_detail_object(task: &Task) -> Value {
    let desc = truncate_desc(&task.description, 500);
    let mut fields = vec![
        field("key", str_val(&task.key)),
        field("title", str_val(&task.title)),
        field("status", str_val(task.status.as_str())),
        field("priority", int_val(task.priority as i64)),
    ];
    if !desc.is_empty() {
        fields.push(field("description", str_val(&desc)));
    }
    if !task.tags.is_empty() {
        fields.push(field("tags", Value::Array(task.tags.clone())));
    }
    if !task.blocked_by.is_empty() {
        fields.push(field("blocked_by", Value::Array(task.blocked_by.clone())));
    }
    if let Some(ref parent) = task.parent_key {
        fields.push(field("parent_key", str_val(parent)));
    }
    fields.push(field("base_branch", str_val(&task.base_branch)));
    fields.push(field("created_at", str_val(&task.created_at.to_rfc3339())));
    Value::Object(fields)
}

fn truncate_desc(s: &str, limit: usize) -> String {
    if s.len() <= limit {
        return s.to_string();
    }
    let end = s
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= limit)
        .last()
        .unwrap_or(0);
    let total = s.len();
    format!("{}... (truncated, {} chars total)", &s[..end], total)
}

fn emit_error(msg: &str, help: &[String]) -> String {
    let mut fields = vec![field("error", str_val(msg))];
    if !help.is_empty() {
        fields.push(field("help", Value::List(help.to_vec())));
    }
    render(&fields)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use planeai_tasks::sqlite::SqliteRepository;

    fn setup_repo(prefix: &str) -> SqliteRepository {
        SqliteRepository::open_in_memory(prefix).unwrap()
    }

    fn add_task(repo: &dyn TaskProvider, title: &str) {
        use planeai_tasks::model::CreateParams;
        repo.create(CreateParams {
            title: title.to_string(),
            ..Default::default()
        })
        .unwrap();
    }

    #[test]
    fn task_ls_shows_toon_table_with_count() {
        let repo = setup_repo("TST");
        add_task(&repo, "Fix auth bug");
        add_task(&repo, "Add pagination");

        let (output, code) = task_ls(&repo, None, &[]);
        assert_eq!(code, 0);
        assert!(output.contains("count: 2 total"), "output was:\n{output}");
        assert!(
            output.contains("tasks[2]{key,title,status,priority,tags,blocked_by}:"),
            "output was:\n{output}"
        );
        assert!(
            output.contains("TST-1,Fix auth bug,todo,0,,"),
            "output was:\n{output}"
        );
        assert!(
            output.contains("TST-2,Add pagination,todo,0,,"),
            "output was:\n{output}"
        );
    }

    #[test]
    fn task_ls_filters_by_status() {
        let repo = setup_repo("TST");
        add_task(&repo, "first");
        add_task(&repo, "second");
        repo.update(
            "TST-1",
            planeai_tasks::model::UpdateParams {
                status: Some(Status::Done),
                ..Default::default()
            },
        )
        .unwrap();

        let (output, code) = task_ls(&repo, Some("todo"), &[]);
        assert_eq!(code, 0);
        assert!(output.contains("count: 1 matching"));
        assert!(output.contains("TST-2"));
        assert!(!output.contains("TST-1"));
    }

    #[test]
    fn task_ls_empty_state() {
        let repo = setup_repo("TST");
        let (output, code) = task_ls(&repo, None, &[]);
        assert_eq!(code, 0);
        assert!(output.contains("tasks: 0 tasks found"));
    }

    #[test]
    fn task_ls_empty_state_with_status_filter() {
        let repo = setup_repo("TST");
        let (output, code) = task_ls(&repo, Some("done"), &[]);
        assert_eq!(code, 0);
        assert!(output.contains("tasks: 0 done tasks found"));
    }

    #[test]
    fn task_show_outputs_full_detail() {
        let repo = setup_repo("TST");
        use planeai_tasks::model::CreateParams;
        // Create a blocker first
        repo.create(CreateParams {
            key: None,
            title: "Blocker task".into(),
            description: "".into(),
            status: None,
            priority: 0,
            tags: vec![],
            blocked_by: vec![],
            parent_key: None,
            base_branch: "main".into(),
        })
        .unwrap();
        repo.create(CreateParams {
            key: None,
            title: "Fix auth bug".into(),
            description: "Need to fix the login flow".into(),
            status: None,
            priority: 2,
            tags: vec!["backend".into()],
            blocked_by: vec!["TST-1".into()],
            parent_key: None,
            base_branch: "main".into(),
        })
        .unwrap();

        let (output, code) = task_show(&repo, "TST-2");
        assert_eq!(code, 0);
        assert!(output.contains("key: TST-2"), "output:\n{output}");
        assert!(output.contains("title: Fix auth bug"), "output:\n{output}");
        assert!(output.contains("status: todo"), "output:\n{output}");
        assert!(output.contains("priority: 2"), "output:\n{output}");
        assert!(
            output.contains("description: Need to fix the login flow"),
            "output:\n{output}"
        );
        assert!(output.contains("tags[1]: backend"), "output:\n{output}");
        assert!(output.contains("blocked_by[1]: TST-1"), "output:\n{output}");
        assert!(output.contains("base_branch: main"), "output:\n{output}");
    }

    #[test]
    fn task_show_truncates_long_description() {
        let repo = setup_repo("TST");
        use planeai_tasks::model::CreateParams;
        let long_desc = "x".repeat(1000);
        repo.create(CreateParams {
            key: None,
            title: "Long task".into(),
            description: long_desc,
            status: None,
            priority: 0,
            tags: vec![],
            blocked_by: vec![],
            parent_key: None,
            base_branch: "main".into(),
        })
        .unwrap();

        let (output, _) = task_show(&repo, "TST-1");
        assert!(
            output.contains("truncated, 1000 chars total"),
            "output:\n{output}"
        );
    }

    #[test]
    fn task_add_echoes_created_task_with_hint() {
        let repo = setup_repo("TST");
        let (output, code) = task_add(
            &repo,
            crate::task_cli::AddParams {
                title: "New feature",
                description: "",
                priority: 1,
                tags: &[],
                blocked_by: &[],
                parent: None,
                base_branch: None,
            },
        );
        assert_eq!(code, 0);
        assert!(output.contains("key: TST-1"), "output:\n{output}");
        assert!(output.contains("title: New feature"), "output:\n{output}");
        assert!(output.contains("status: todo"), "output:\n{output}");
        assert!(output.contains("priority: 1"), "output:\n{output}");
        assert!(
            output.contains("planeai-cli axi task move TST-1 in_progress"),
            "output:\n{output}"
        );
    }

    #[test]
    fn task_move_echoes_updated_task() {
        let repo = setup_repo("TST");
        add_task(&repo, "A task");
        let (output, code) = task_move(&repo, "TST-1", "in_progress");
        assert_eq!(code, 0);
        assert!(output.contains("status: in_progress"), "output:\n{output}");
    }

    #[test]
    fn task_move_idempotent_noop() {
        let repo = setup_repo("TST");
        add_task(&repo, "A task");
        // Move once
        task_move(&repo, "TST-1", "done");
        // Move again to same status
        let (output, code) = task_move(&repo, "TST-1", "done");
        assert_eq!(code, 0);
        assert!(output.contains("no-op"), "output:\n{output}");
        assert!(output.contains("status: done"), "output:\n{output}");
    }

    #[test]
    fn task_show_not_found_returns_error() {
        let repo = setup_repo("TST");
        let (output, code) = task_show(&repo, "TST-999");
        assert_eq!(code, 1);
        assert!(output.contains("error:"), "output:\n{output}");
        assert!(output.contains("help"), "output:\n{output}");
    }

    #[test]
    fn task_move_invalid_status_returns_error() {
        let repo = setup_repo("TST");
        add_task(&repo, "A task");
        let (output, code) = task_move(&repo, "TST-1", "bogus");
        assert_eq!(code, 1);
        assert!(output.contains("error:"), "output:\n{output}");
        assert!(output.contains("invalid status"), "output:\n{output}");
        assert!(output.contains("Valid statuses"), "output:\n{output}");
    }

    #[test]
    fn session_create_outputs_toon_with_session_id() {
        let session = crate::db::Session {
            id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
            project_id: "proj-1".to_string(),
            name: "my-feature".to_string(),
            tmux_name: None,
            branch: "feat/my-feature".to_string(),
            status: "active".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            worktree_path: Some("/tmp/wt/aaaaaaaa".to_string()),
            provider: Some("kiro".to_string()),
            backend: "daemon".to_string(),
            provider_session_id: None,
            tab_count: 1,
            auto_approve: true,
            task_key: None,
            base_branch: Some("main".to_string()),
            pr_url: None,
            pr_state: None,
            attached_once: false,
            parent_session_id: Some("pppppppp-1111-2222-3333-444444444444".to_string()),
        };

        let (output, code) = session_create_output(&session);
        assert_eq!(code, 0);
        assert!(
            output.contains("id: aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
            "output:\n{output}"
        );
        assert!(output.contains("name: my-feature"), "output:\n{output}");
        assert!(output.contains("status: active"), "output:\n{output}");
        assert!(
            output.contains("branch: feat/my-feature"),
            "output:\n{output}"
        );
        assert!(
            output.contains("worktree_path: /tmp/wt/aaaaaaaa"),
            "output:\n{output}"
        );
        assert!(
            output.contains("parent_session_id: pppppppp-1111-2222-3333-444444444444"),
            "output:\n{output}"
        );
        // Should include help hint
        assert!(
            output.contains("planeai-cli axi session prompt"),
            "output:\n{output}"
        );
    }

    #[test]
    fn session_read_outputs_toon_with_text() {
        let text = "line1\nline2\nline3";
        let (output, code) = session_read_output("aaaabbbb", text);
        assert_eq!(code, 0);
        assert!(output.contains("session_id: aaaabbbb"), "output:\n{output}");
        assert!(output.contains("lines: 3"), "output:\n{output}");
        // Lines are emitted as a list (one per line)
        assert!(output.contains("output[3]:"), "output:\n{output}");
        assert!(output.contains("- line1"), "output:\n{output}");
        assert!(output.contains("- line2"), "output:\n{output}");
        assert!(output.contains("- line3"), "output:\n{output}");
    }

    #[test]
    fn session_read_cursor_outputs_toon_with_cursor_fields() {
        let (output, code) = session_read_cursor_output(
            "aaaabbbb",
            "daemon",
            "daemon:1234",
            false,
            "new output here",
        );
        assert_eq!(code, 0);
        assert!(output.contains("session_id: aaaabbbb"), "output:\n{output}");
        assert!(output.contains("backend: daemon"), "output:\n{output}");
        assert!(
            output.contains("cursor: \"daemon:1234\""),
            "output:\n{output}"
        );
        assert!(output.contains("truncated: false"), "output:\n{output}");
        assert!(
            output.contains("text: new output here"),
            "output:\n{output}"
        );
    }

    #[test]
    fn session_read_cursor_truncated_flag() {
        let (output, code) = session_read_cursor_output(
            "bbbbcccc",
            "tmux",
            "tmux:100:9876543210",
            true,
            "all available content",
        );
        assert_eq!(code, 0);
        assert!(output.contains("truncated: true"), "output:\n{output}");
        assert!(output.contains("backend: tmux"), "output:\n{output}");
    }

    // ─── Session children/tree TOON output ───────────────────────────────────

    fn setup_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        conn
    }

    /// Create a tree: root → child1, child2; child1 → grandchild
    fn setup_session_tree(conn: &rusqlite::Connection) -> (String, String, String, String) {
        let project = crate::db::create_project(conn, "test-project", "/tmp/test").unwrap();
        let root_id = "aaaaaaaa-1111-2222-3333-444444444444".to_string();
        let child1_id = "bbbbbbbb-1111-2222-3333-444444444444".to_string();
        let child2_id = "cccccccc-1111-2222-3333-444444444444".to_string();
        let grandchild_id = "dddddddd-1111-2222-3333-444444444444".to_string();

        crate::db::create_session_with_id(
            conn,
            &root_id,
            &project.id,
            "Planner",
            None,
            "main",
            None,
            Some("claude"),
            "daemon",
            true,
            Some("PLA-201"),
            None,
            None,
        )
        .unwrap();

        crate::db::create_session_with_id(
            conn,
            &child1_id,
            &project.id,
            "Worker 1",
            None,
            "main",
            None,
            Some("codex"),
            "daemon",
            true,
            Some("PLA-201"),
            None,
            Some(&root_id),
        )
        .unwrap();

        crate::db::create_session_with_id(
            conn,
            &child2_id,
            &project.id,
            "Reviewer",
            None,
            "main",
            None,
            Some("kiro"),
            "daemon",
            true,
            Some("PLA-201"),
            None,
            Some(&root_id),
        )
        .unwrap();

        crate::db::create_session_with_id(
            conn,
            &grandchild_id,
            &project.id,
            "Sub-worker",
            None,
            "main",
            None,
            Some("codex"),
            "daemon",
            true,
            None,
            None,
            Some(&child1_id),
        )
        .unwrap();

        (root_id, child1_id, child2_id, grandchild_id)
    }

    #[test]
    fn session_children_outputs_toon_table() {
        let conn = setup_db();
        let (root_id, child1_id, child2_id, _) = setup_session_tree(&conn);

        let (output, code) = session_children(&conn, &root_id[..8]);
        assert_eq!(code, 0);
        assert!(
            output.contains("parent_session_id: aaaaaaaa"),
            "output:\n{output}"
        );
        assert!(
            output.contains(
                "children[2]{id,parent_session_id,name,status,provider,task_key,backend}:"
            ),
            "output:\n{output}"
        );
        assert!(output.contains(&child1_id[..8]), "output:\n{output}");
        assert!(output.contains(&child2_id[..8]), "output:\n{output}");
        assert!(output.contains("Worker 1"), "output:\n{output}");
        assert!(output.contains("Reviewer"), "output:\n{output}");
    }

    #[test]
    fn session_children_empty_outputs_message() {
        let conn = setup_db();
        let (_, _, child2_id, _) = setup_session_tree(&conn);

        // child2 has no children
        let (output, code) = session_children(&conn, &child2_id[..8]);
        assert_eq!(code, 0);
        assert!(output.contains("children: 0 children"), "output:\n{output}");
    }

    #[test]
    fn session_tree_outputs_full_tree_toon() {
        let conn = setup_db();
        let (root_id, child1_id, child2_id, grandchild_id) = setup_session_tree(&conn);

        let (output, code) = session_tree(&conn, &root_id[..8]);
        assert_eq!(code, 0);
        assert!(output.contains("session_tree:"), "output:\n{output}");
        assert!(output.contains("root: aaaaaaaa"), "output:\n{output}");
        assert!(
            output.contains(
                "sessions[4]{id,parent_session_id,name,status,provider,task_key,backend}:"
            ),
            "output:\n{output}"
        );
        // BFS order: root, child1, child2, grandchild
        let lines: Vec<&str> = output.lines().collect();
        let session_lines: Vec<&&str> = lines
            .iter()
            .filter(|l| {
                l.trim().starts_with(&root_id[..8])
                    || l.trim().starts_with(&child1_id[..8])
                    || l.trim().starts_with(&child2_id[..8])
                    || l.trim().starts_with(&grandchild_id[..8])
            })
            .collect();
        assert_eq!(
            session_lines.len(),
            4,
            "expected 4 session rows, output:\n{output}"
        );
    }

    #[test]
    fn session_tree_from_child_shows_full_tree() {
        let conn = setup_db();
        let (_, _, _, grandchild_id) = setup_session_tree(&conn);

        // Call from grandchild — should walk up to root
        let (output, code) = session_tree(&conn, &grandchild_id[..8]);
        assert_eq!(code, 0);
        assert!(output.contains("root: aaaaaaaa"), "output:\n{output}");
        assert!(output.contains("sessions[4]"), "output:\n{output}");
    }
}
