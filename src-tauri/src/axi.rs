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

// ─── Loop ────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn loop_create(
    conn: &rusqlite::Connection,
    cwd: &str,
    project_flag: Option<&str>,
    task_key: Option<&str>,
    strategy: &str,
    recipe_flag: Option<&str>,
    goal: &str,
    max_rounds: i64,
    start: bool,
) -> (String, i32) {
    use planeai_core::loop_recipe_service::RecipeService;
    use planeai_core::loop_run::LoopStrategy;
    use planeai_core::loop_service::{CreateLoopParams, LoopService};
    use std::collections::BTreeMap;

    // Validate max_rounds
    if max_rounds < 1 {
        return (
            emit_error(
                "--max-rounds must be >= 1",
                &["Use --max-rounds 3 for the default".into()],
            ),
            1,
        );
    }

    // Resolve project
    let project = match resolve_project(conn, project_flag, cwd) {
        Ok(p) => p,
        Err(e) => {
            return (
                emit_error(
                    &e,
                    &["Run `planeai-cli axi project ls` to see projects".into()],
                ),
                1,
            )
        }
    };

    // Validate task key if provided — scoped to the resolved project prefix
    if let Some(key) = task_key {
        match conn
            .prepare("SELECT 1 FROM tasks WHERE key = ?1 AND project_prefix = ?2")
            .and_then(|mut stmt| stmt.exists(rusqlite::params![key, project.prefix]))
        {
            Ok(true) => {}
            Ok(false) => {
                return (
                    emit_error(
                        &format!("task not found: {key}"),
                        &[format!(
                            "Run `planeai-cli axi task ls --project {}` to see tasks",
                            project.prefix
                        )],
                    ),
                    1,
                );
            }
            Err(e) => {
                return (
                    emit_error(
                        &format!("task validation failed: {e}"),
                        &["task database may be unavailable or schema incompatible".into()],
                    ),
                    1,
                );
            }
        }
    }

    // Conflict check: if both --recipe and non-default --strategy are provided, error
    if recipe_flag.is_some() && strategy != "maker-verifier" {
        return (
            emit_error(
                "cannot specify both --recipe and --strategy",
                &["Use --recipe to specify a recipe, or --strategy as an alias (not both)".into()],
            ),
            1,
        );
    }
    let recipe_id = recipe_flag.unwrap_or(strategy);
    let project_root = std::path::Path::new(&project.path);

    // Resolve recipe. If --recipe was explicitly passed, fail loudly on error.
    // If using --strategy (legacy alias), allow graceful fallback.
    let discovered = if recipe_flag.is_some() {
        // Explicit --recipe: must resolve or fail
        match RecipeService::resolve(recipe_id, Some(project_root)) {
            Ok(d) => Some(d),
            Err(e) => {
                return (
                    emit_error(
                        &format!("recipe not found: {}", e),
                        &["Run `planeai-cli axi loop recipe ls` to see available recipes".into()],
                    ),
                    1,
                );
            }
        }
    } else {
        // Legacy --strategy: try to resolve, fall back to non-recipe loop
        RecipeService::resolve(recipe_id, Some(project_root)).ok()
    };

    // Build resolved recipe state
    struct ResolvedRecipe {
        policy_json: Option<serde_json::Value>,
        strategy: String,
        source: &'static str,
        max_rounds: i64,
    }

    let resolved = match discovered {
        Some(ref dr) => {
            let mut inputs = BTreeMap::new();
            inputs.insert("goal".to_string(), goal.to_string());
            if let Some(key) = task_key {
                inputs.insert("task_key".to_string(), key.to_string());
            }
            let snapshot = RecipeService::create_snapshot(dr, inputs);
            let max_r = snapshot.policy.max_rounds as i64;
            let json_val = serde_json::to_value(&snapshot).ok();
            ResolvedRecipe {
                policy_json: json_val,
                strategy: dr.recipe.id.clone(),
                source: dr.source.as_str(),
                max_rounds: max_r,
            }
        }
        None => ResolvedRecipe {
            policy_json: None,
            strategy: recipe_id.to_string(),
            source: "none",
            max_rounds,
        },
    };

    // Block creation if recipe uses a non-manual trigger (not executable in v1)
    if let Some(ref dr) = discovered {
        if !dr.recipe.trigger.is_v1_executable() {
            return (
                emit_error(
                    &format!(
                        "recipe '{}' uses trigger kind '{}' which is not executable in v1",
                        dr.recipe.id, dr.recipe.trigger.kind
                    ),
                    &["Only 'manual' trigger is supported for loop creation".into()],
                ),
                1,
            );
        }
    }

    // Validate recipe before creating a LoopRun (reject invalid recipes)
    if let Some(ref dr) = discovered {
        let validation = RecipeService::validate(&dr.recipe, Some(project_root));
        if !validation.valid {
            return (
                emit_error(
                    &format!("recipe '{}' failed validation", dr.recipe.id),
                    &validation
                        .errors
                        .iter()
                        .map(|e| e.to_string())
                        .collect::<Vec<_>>(),
                ),
                1,
            );
        }
    }

    let parent_session_id = std::env::var("PLANEAI_SESSION_ID").ok();

    let params = CreateLoopParams {
        project_id: project.id.clone(),
        task_key: task_key.map(|s| s.to_string()),
        created_by_session_id: parent_session_id,
        strategy: LoopStrategy::new(&resolved.strategy),
        goal: goal.to_string(),
        max_rounds: resolved.max_rounds,
        policy_json: resolved.policy_json,
        budget_json: None,
    };

    let loop_run = match LoopService::create_loop(conn, params) {
        Ok(r) => r,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    // Append loop_created event
    if let Err(e) =
        LoopService::append_loop_event(conn, &loop_run.id, "loop_created", &serde_json::json!({}))
    {
        return (emit_error(&e.to_string(), &[]), 1);
    }

    // If a recipe was resolved, append recipe_loaded event
    if discovered.is_some() {
        let _ = LoopService::append_loop_event(
            conn,
            &loop_run.id,
            "recipe_loaded",
            &serde_json::json!({"recipe_id": resolved.strategy, "source": resolved.source}),
        );
    }

    // If --start, transition to running and append loop_started event
    if start {
        if let Err(e) = LoopService::update_loop_status(
            conn,
            &loop_run.id,
            planeai_core::loop_run::LoopStatus::Running,
        ) {
            return (emit_error(&e.to_string(), &[]), 1);
        }
        if let Err(e) = LoopService::append_loop_event(
            conn,
            &loop_run.id,
            "loop_started",
            &serde_json::json!({}),
        ) {
            return (emit_error(&e.to_string(), &[]), 1);
        }
    }

    let status_str = if start { "running" } else { "draft" };
    let short_id = &loop_run.id[..8];

    let mut loop_fields = vec![
        field("id", str_val(&loop_run.id)),
        field("status", str_val(status_str)),
        field("recipe_id", str_val(&resolved.strategy)),
        field("recipe_source", str_val(resolved.source)),
        field("trigger", str_val("manual")),
        field("goal", str_val(goal)),
        field("current_round", int_val(0)),
        field("max_rounds", int_val(resolved.max_rounds)),
    ];
    if let Some(key) = task_key {
        loop_fields.push(field("task_key", str_val(key)));
    }
    if let Some(ref dr) = discovered {
        loop_fields.push(field(
            "max_ticks",
            int_val(dr.recipe.policy.max_ticks as i64),
        ));
    }

    let next = if start {
        format!("run `planeai-cli axi loop tick {short_id}` to execute the next recipe step")
    } else {
        format!(
            "run `planeai-cli axi loop tick {short_id}` to start and execute the next recipe step"
        )
    };

    let fields = vec![
        field("loop", Value::Object(loop_fields)),
        field("next_actions", Value::List(vec![next])),
    ];
    (render(&fields), 0)
}

pub fn loop_observe(conn: &rusqlite::Connection, id: &str, limit: usize) -> (String, i32) {
    use planeai_core::loop_run::LoopStatus;
    use planeai_core::loop_service::LoopService;

    let loop_run = match resolve_loop(conn, id) {
        Ok(r) => r,
        Err(e) => return (emit_error(&e, &[]), 1),
    };

    let short_id = &loop_run.id[..8];

    let mut loop_fields = vec![
        field("id", str_val(&loop_run.id)),
        field("status", str_val(loop_run.status.as_str())),
        field("strategy", str_val(loop_run.strategy.as_str())),
        field("goal", str_val(&loop_run.goal)),
        field("current_round", int_val(loop_run.current_round)),
        field("max_rounds", int_val(loop_run.max_rounds)),
    ];
    if let Some(ref key) = loop_run.task_key {
        loop_fields.push(field("task_key", str_val(key)));
    }
    if let Some(ref sid) = loop_run.created_by_session_id {
        loop_fields.push(field("created_by_session_id", str_val(sid)));
    }
    loop_fields.push(field("created_at", str_val(&loop_run.created_at)));
    loop_fields.push(field("updated_at", str_val(&loop_run.updated_at)));

    let mut fields = vec![field("loop", Value::Object(loop_fields))];

    // Loop-owned sessions (use `loop tree` for recursive expansion)
    let loop_sessions = match LoopService::list_loop_sessions(conn, &loop_run.id) {
        Ok(s) => s,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };
    if loop_sessions.is_empty() {
        fields.push(field("sessions", str_val("0 sessions")));
    } else {
        let rows: Vec<Vec<String>> = loop_sessions
            .iter()
            .map(|s| {
                vec![
                    s.session_id[..8].to_string(),
                    s.role.clone(),
                    s.provider.clone().unwrap_or_default(),
                    s.status.clone(),
                    s.round.to_string(),
                ]
            })
            .collect();

        fields.push(field(
            "sessions",
            Value::Table {
                columns: vec![
                    "id".into(),
                    "role".into(),
                    "provider".into(),
                    "status".into(),
                    "round".into(),
                ],
                rows,
            },
        ));
    }

    // Events (recent, capped by limit)
    let events = match LoopService::list_loop_events(conn, &loop_run.id) {
        Ok(e) => e,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };
    let recent: Vec<_> = if events.len() > limit {
        events[events.len() - limit..].to_vec()
    } else {
        events
    };

    if recent.is_empty() {
        fields.push(field("events", str_val("0 events")));
    } else {
        let rows: Vec<Vec<String>> = recent
            .iter()
            .map(|e| vec![e.id.to_string(), e.kind.clone(), e.ts.clone()])
            .collect();
        fields.push(field(
            "events",
            Value::Table {
                columns: vec!["id".into(), "kind".into(), "ts".into()],
                rows,
            },
        ));
    }

    // Next actions (only shown for non-terminal loops)
    let is_terminal = matches!(
        loop_run.status,
        LoopStatus::Cancelled
            | LoopStatus::Failed
            | LoopStatus::CompletedUnreviewed
            | LoopStatus::Approved
            | LoopStatus::Merged
            | LoopStatus::Cleaned
    );

    if is_terminal {
        fields.push(field(
            "next_actions",
            Value::List(vec![format!(
                "Loop is in terminal status '{}'. No further actions available.",
                loop_run.status.as_str()
            )]),
        ));
    } else {
        fields.push(field(
            "next_actions",
            Value::List(vec![
                format!("Run `planeai-cli axi loop tick {short_id}` to dispatch the next step"),
                format!("Run `planeai-cli axi loop stop {short_id}` to cancel the loop"),
            ]),
        ));
    }

    (render(&fields), 0)
}

pub fn loop_tick(conn: &rusqlite::Connection, id: &str) -> (String, i32) {
    use planeai_core::loop_recipe_service::RecipeSnapshot;
    use planeai_core::loop_run::LoopStatus;
    use planeai_core::loop_service::LoopService;

    let mut loop_run = match resolve_loop(conn, id) {
        Ok(r) => r,
        Err(e) => return (emit_error(&e, &[]), 1),
    };

    let is_terminal = matches!(
        loop_run.status,
        LoopStatus::Cancelled
            | LoopStatus::Failed
            | LoopStatus::CompletedUnreviewed
            | LoopStatus::Approved
            | LoopStatus::Merged
            | LoopStatus::Cleaned
    );

    if is_terminal {
        return (
            emit_error(
                &format!(
                    "cannot tick loop {}: already in terminal status '{}'",
                    &loop_run.id[..8],
                    loop_run.status.as_str()
                ),
                &[],
            ),
            1,
        );
    }

    // If draft, transition to running and append loop_started event
    if loop_run.status == LoopStatus::Draft {
        if let Err(e) = LoopService::update_loop_status(conn, &loop_run.id, LoopStatus::Running) {
            return (emit_error(&e.to_string(), &[]), 1);
        }
        if let Err(e) = LoopService::append_loop_event(
            conn,
            &loop_run.id,
            "loop_started",
            &serde_json::json!({}),
        ) {
            return (emit_error(&e.to_string(), &[]), 1);
        }
        loop_run.status = LoopStatus::Running;
    }

    // Check if this loop has a recipe snapshot in policy_json
    if let Some(ref policy_val) = loop_run.policy_json {
        if let Ok(mut snapshot) = serde_json::from_value::<RecipeSnapshot>(policy_val.clone()) {
            // Recipe-aware tick: delegate to recipe_tick runtime
            return crate::recipe_tick::tick_recipe(conn, &loop_run.id, &mut snapshot);
        }
    }

    // Legacy tick (no recipe) — append a tick event
    let payload = serde_json::json!({});
    let event = match LoopService::append_loop_event(conn, &loop_run.id, "tick", &payload) {
        Ok(e) => e,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    let short_id = &loop_run.id[..8];

    let fields = vec![
        field(
            "loop",
            Value::Object(vec![
                field("id", str_val(&loop_run.id)),
                field("status", str_val(loop_run.status.as_str())),
                field("current_round", int_val(loop_run.current_round)),
                field("max_rounds", int_val(loop_run.max_rounds)),
            ]),
        ),
        field(
            "event",
            Value::Object(vec![
                field("id", int_val(event.id)),
                field("kind", str_val("tick")),
                field("ts", str_val(&event.ts)),
            ]),
        ),
        field(
            "next_actions",
            Value::List(vec![
                format!("Run `planeai-cli axi loop observe {short_id}` to inspect state"),
                format!("Run `planeai-cli axi loop stop {short_id}` to cancel the loop"),
            ]),
        ),
    ];
    (render(&fields), 0)
}

pub fn loop_stop(conn: &rusqlite::Connection, id: &str) -> (String, i32) {
    use planeai_core::loop_run::LoopStatus;
    use planeai_core::loop_service::LoopService;

    let loop_run = match resolve_loop(conn, id) {
        Ok(r) => r,
        Err(e) => return (emit_error(&e, &[]), 1),
    };

    let short_id = &loop_run.id[..8];

    // Idempotent: if already in a terminal status, just acknowledge
    let is_terminal = matches!(
        loop_run.status,
        LoopStatus::Cancelled
            | LoopStatus::Failed
            | LoopStatus::CompletedUnreviewed
            | LoopStatus::Approved
            | LoopStatus::Merged
            | LoopStatus::Cleaned
    );

    if is_terminal {
        let fields = vec![
            field(
                "loop",
                Value::Object(vec![
                    field("id", str_val(&loop_run.id)),
                    field("status", str_val(loop_run.status.as_str())),
                ]),
            ),
            field("note", str_val("already in terminal status (no-op)")),
        ];
        return (render(&fields), 0);
    }

    // Transition to cancelled and append event atomically
    if let Err(e) = LoopService::update_loop_status(conn, &loop_run.id, LoopStatus::Cancelled) {
        return (emit_error(&e.to_string(), &[]), 1);
    }
    if let Err(e) =
        LoopService::append_loop_event(conn, &loop_run.id, "loop_cancelled", &serde_json::json!({}))
    {
        return (emit_error(&e.to_string(), &[]), 1);
    }

    let fields = vec![
        field(
            "loop",
            Value::Object(vec![
                field("id", str_val(&loop_run.id)),
                field("status", str_val("cancelled")),
            ]),
        ),
        field(
            "next_actions",
            Value::List(vec![
                format!("Run `planeai-cli axi loop observe {short_id}` to inspect final state"),
                "Clean up any running sessions manually if needed".into(),
            ]),
        ),
    ];
    (render(&fields), 0)
}

pub fn loop_tree(conn: &rusqlite::Connection, id: &str) -> (String, i32) {
    use planeai_core::loop_service::LoopService;
    use planeai_core::services::SessionService;

    let loop_run = match resolve_loop(conn, id) {
        Ok(r) => r,
        Err(e) => return (emit_error(&e, &[]), 1),
    };

    let short_id = &loop_run.id[..8];
    let loop_sessions = match LoopService::list_loop_sessions(conn, &loop_run.id) {
        Ok(s) => s,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    if loop_sessions.is_empty() {
        let fields = vec![
            field("loop_id", str_val(short_id)),
            field("sessions", str_val("0 sessions")),
        ];
        return (render(&fields), 0);
    }

    // For each loop session, get its full tree (recursive children)
    let mut all_records: Vec<planeai_core::services::SessionRecord> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for ls in &loop_sessions {
        if seen.contains(&ls.session_id) {
            continue;
        }
        if let Ok(tree) = SessionService::tree(conn, &ls.session_id) {
            for record in tree {
                if seen.insert(record.id.clone()) {
                    all_records.push(record);
                }
            }
        }
    }

    let rows: Vec<Vec<String>> = all_records
        .iter()
        .map(|s| {
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
        })
        .collect();

    let fields = vec![
        field("loop_id", str_val(short_id)),
        field(
            "sessions",
            Value::Table {
                columns: vec![
                    "id".into(),
                    "parent_session_id".into(),
                    "name".into(),
                    "status".into(),
                    "provider".into(),
                    "task_key".into(),
                    "backend".into(),
                ],
                rows,
            },
        ),
    ];
    (render(&fields), 0)
}

// ─── Loop Handoff ────────────────────────────────────────────────────────────

pub fn loop_handoff_path(
    conn: &rusqlite::Connection,
    loop_id_arg: &str,
    session_arg: &str,
    cwd: &str,
) -> (String, i32) {
    use planeai_core::loop_service::LoopService;

    let loop_run = match resolve_loop(conn, loop_id_arg) {
        Ok(r) => r,
        Err(e) => return (emit_error(&e, &[]), 1),
    };

    // Resolve session by prefix among loop sessions
    let loop_sessions = match LoopService::list_loop_sessions(conn, &loop_run.id) {
        Ok(s) => s,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    let session = match resolve_loop_session(&loop_sessions, session_arg) {
        Ok(s) => s,
        Err(e) => return (emit_error(&e, &[]), 1),
    };

    // Determine the base path (session worktree or project root/cwd)
    let base_path = resolve_handoff_base_path(conn, &session.session_id, cwd);

    let handoff_path = std::path::PathBuf::from(&base_path)
        .join(".planeai")
        .join("loops")
        .join(&loop_run.id)
        .join("sessions")
        .join(&session.session_id)
        .join("handoff.json");

    // Create the parent directory
    if let Some(parent) = handoff_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let exists = handoff_path.exists();
    let short_loop = &loop_run.id[..std::cmp::min(8, loop_run.id.len())];
    let short_session = &session.session_id[..std::cmp::min(8, session.session_id.len())];
    let path_str = handoff_path.to_string_lossy().to_string();

    let fields = vec![
        field(
            "handoff_path",
            Value::Object(vec![
                field("loop_id", str_val(&loop_run.id)),
                field("session_id", str_val(&session.session_id)),
                field("role", str_val(&session.role)),
                field("path", str_val(&path_str)),
                field("exists", Value::Bool(exists)),
            ]),
        ),
        field(
            "next_actions",
            Value::List(vec![
                format!("write a planeai.handoff.v1 JSON file to {path_str}"),
                format!(
                    "run `planeai-cli axi loop handoff record --loop {short_loop} --session {short_session} --path {path_str}`"
                ),
            ]),
        ),
    ];
    (render(&fields), 0)
}

pub fn loop_handoff_record(
    conn: &rusqlite::Connection,
    loop_id_arg: &str,
    session_arg: &str,
    path: &std::path::Path,
    cwd: &str,
) -> (String, i32) {
    use planeai_core::handoff::{parse_handoff, validate_ids, HandoffStatus};
    use planeai_core::loop_run::LoopStatus;
    use planeai_core::loop_service::LoopService;

    let loop_run = match resolve_loop(conn, loop_id_arg) {
        Ok(r) => r,
        Err(e) => return (emit_error(&e, &[]), 1),
    };

    // Resolve session by prefix among loop sessions
    let loop_sessions = match LoopService::list_loop_sessions(conn, &loop_run.id) {
        Ok(s) => s,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    let session = match resolve_loop_session(&loop_sessions, session_arg) {
        Ok(s) => s,
        Err(e) => return (emit_error(&e, &[]), 1),
    };

    // Security: canonicalize and validate path is under the project root or worktree
    let canonical_path = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(e) => {
            return (
                emit_error(
                    &format!("cannot read handoff file: {e}"),
                    &[format!("path: {}", path.display())],
                ),
                1,
            )
        }
    };

    let base_path = resolve_handoff_base_path(conn, &session.session_id, cwd);
    let canonical_base = std::fs::canonicalize(&base_path).unwrap_or_else(|_| base_path.into());

    if !canonical_path.starts_with(&canonical_base) {
        return (
            emit_error(
                "handoff file path is outside the project root",
                &[
                    format!("path: {}", canonical_path.display()),
                    format!("project root: {}", canonical_base.display()),
                ],
            ),
            1,
        );
    }

    // Read the file
    let content = match std::fs::read_to_string(&canonical_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                emit_error(
                    &format!("cannot read handoff file: {e}"),
                    &[format!("path: {}", canonical_path.display())],
                ),
                1,
            )
        }
    };

    // Parse and validate
    let handoff = match parse_handoff(&content) {
        Ok(h) => h,
        Err(errors) => {
            let details: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
            let mut fields = vec![
                field("error", str_val("invalid handoff file")),
                field("path", str_val(&canonical_path.to_string_lossy())),
                field("details", Value::List(details)),
                field(
                    "help",
                    Value::List(vec![format!(
                        "run `planeai-cli axi loop handoff path --loop {} --session {}` for the expected location",
                        &loop_run.id[..std::cmp::min(8, loop_run.id.len())],
                        &session.session_id[..std::cmp::min(8, session.session_id.len())]
                    )]),
                ),
            ];
            // If all errors are just about unknown schema, still include it
            let _ = &mut fields;
            return (render(&fields), 1);
        }
    };

    // Validate IDs match
    if let Err(errors) = validate_ids(&handoff, &loop_run.id, &session.session_id) {
        let details: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        let fields = vec![
            field("error", str_val("invalid handoff file")),
            field("path", str_val(&canonical_path.to_string_lossy())),
            field("details", Value::List(details)),
            field(
                "help",
                Value::List(vec![format!(
                    "ensure loop_id and session_id in the handoff file match the command arguments"
                )]),
            ),
        ];
        return (render(&fields), 1);
    }

    // Determine loop status transition
    let is_active = matches!(
        loop_run.status,
        LoopStatus::Running | LoopStatus::Observing | LoopStatus::Verifying
    );

    let (new_loop_status, state_changed) = if is_active {
        match handoff.status {
            HandoffStatus::Completed => {
                if loop_run.status == LoopStatus::Running {
                    (Some(LoopStatus::Observing), true)
                } else {
                    (None, false)
                }
            }
            HandoffStatus::Blocked => (Some(LoopStatus::Blocked), true),
            HandoffStatus::NeedsHuman => (Some(LoopStatus::NeedsHuman), true),
            HandoffStatus::Failed => (Some(LoopStatus::Failed), true),
        }
    } else {
        (None, false)
    };

    // Atomically record: artifact + event + session status + loop status
    let handoff_json: serde_json::Value = serde_json::to_value(&handoff).unwrap_or_default();
    let session_status = handoff.status.as_str();

    let event_payload = serde_json::json!({
        "session_id": session.session_id,
        "status": session_status,
        "path": canonical_path.to_string_lossy(),
    });

    let result = match LoopService::record_handoff(
        conn,
        planeai_core::loop_service::RecordHandoffParams {
            loop_id: loop_run.id.clone(),
            session_id: session.session_id.clone(),
            artifact_path: Some(canonical_path.to_string_lossy().to_string()),
            content_json: Some(handoff_json),
            handoff_status: session_status.to_string(),
            event_payload,
            new_loop_status: new_loop_status.clone(),
        },
    ) {
        Ok(r) => r,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    let final_loop_status = new_loop_status.unwrap_or(loop_run.status.clone());

    // Build TOON output
    let short_loop = &loop_run.id[..std::cmp::min(8, loop_run.id.len())];

    let mut result_fields = vec![field(
        "handoff_recorded",
        Value::Object(vec![
            field("loop_id", str_val(&loop_run.id)),
            field("session_id", str_val(&session.session_id)),
            field("artifact_id", str_val(&result.artifact_id)),
            field("event_id", int_val(result.event_id)),
            field("schema", str_val(&handoff.schema)),
            field("status", str_val(session_status)),
            field("loop_status", str_val(final_loop_status.as_str())),
            field("session_status", str_val(session_status)),
            field("state_changed", Value::Bool(state_changed)),
            field("path", str_val(&canonical_path.to_string_lossy())),
        ]),
    )];

    // Add risks if present
    if !handoff.risks.is_empty() {
        result_fields.push(field("risks", Value::List(handoff.risks.clone())));
    }

    // Add next_actions guidance
    let next_actions = match handoff.status {
        HandoffStatus::Completed => vec![format!(
            "run verifier gates or `planeai-cli axi loop tick {short_loop}`"
        )],
        HandoffStatus::Blocked => {
            vec!["inspect handoff risks and unblock manually".to_string()]
        }
        HandoffStatus::NeedsHuman => {
            vec!["review handoff and provide human input".to_string()]
        }
        HandoffStatus::Failed => {
            vec!["inspect failure, fix, and re-run or stop loop".to_string()]
        }
    };
    result_fields.push(field("next_actions", Value::List(next_actions)));

    (render(&result_fields), 0)
}

// ─── Loop Helpers ────────────────────────────────────────────────────────────

/// Resolve a session within a loop's sessions by exact or prefix match.
fn resolve_loop_session(
    loop_sessions: &[planeai_core::loop_run::LoopSession],
    session_arg: &str,
) -> Result<planeai_core::loop_run::LoopSession, String> {
    // Exact match first
    if let Some(s) = loop_sessions.iter().find(|s| s.session_id == session_arg) {
        return Ok(s.clone());
    }

    // Prefix match
    let matches: Vec<_> = loop_sessions
        .iter()
        .filter(|s| s.session_id.starts_with(session_arg))
        .collect();

    match matches.len() {
        0 => Err(format!("session not found in this loop: {session_arg}")),
        1 => Ok(matches[0].clone()),
        n => {
            let previews: Vec<String> = matches
                .iter()
                .take(5)
                .map(|s| s.session_id[..std::cmp::min(8, s.session_id.len())].to_string())
                .collect();
            Err(format!(
                "ambiguous session prefix '{session_arg}' matches {n} sessions: {}",
                previews.join(", ")
            ))
        }
    }
}

/// Resolve the base path for handoff file validation.
/// Prefers the session's worktree path, falls back to project path, then CWD.
fn resolve_handoff_base_path(conn: &rusqlite::Connection, session_id: &str, cwd: &str) -> String {
    // Try to get the session's worktree path
    if let Ok(Some(session)) = db::get_session(conn, session_id) {
        if let Some(ref wt) = session.worktree_path {
            if !wt.is_empty() {
                return wt.clone();
            }
        }
    }

    // Fall back to CWD
    cwd.to_string()
}

/// Resolve a project from --project flag or CWD, returning (id, prefix, name, path).
fn resolve_project(
    conn: &rusqlite::Connection,
    project_flag: Option<&str>,
    cwd: &str,
) -> Result<db::Project, String> {
    let projects = db::list_projects(conn).unwrap_or_default();

    if let Some(name) = project_flag {
        projects
            .into_iter()
            .find(|p| p.name == name || p.prefix == name)
            .ok_or_else(|| format!("project not found: {name}"))
    } else {
        projects
            .into_iter()
            .find(|p| cwd.starts_with(&p.path))
            .ok_or_else(|| "could not resolve project from current directory".to_string())
    }
}

/// Resolve a loop by full ID or prefix match.
fn resolve_loop(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<planeai_core::loop_run::LoopRun, String> {
    use planeai_core::loop_service::LoopService;

    // Try exact match first
    match LoopService::get_loop(conn, id) {
        Ok(Some(run)) => return Ok(run),
        Ok(None) => {}
        Err(e) => return Err(e.to_string()),
    }

    // Prefix match: query all loops and find prefix match
    // We need a list of all loops — use a raw query for prefix matching
    let mut stmt = conn
        .prepare("SELECT id FROM loop_runs WHERE id GLOB ?1")
        .map_err(|e| e.to_string())?;
    let escaped_id: String = id
        .chars()
        .flat_map(|c| match c {
            '*' | '?' | '[' | ']' => vec!['[', c, ']'],
            _ => vec![c],
        })
        .collect();
    let prefix_pattern = format!("{escaped_id}*");
    let ids: Vec<String> = stmt
        .query_map(rusqlite::params![prefix_pattern], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    match ids.len() {
        0 => Err(format!("loop not found: {id}")),
        1 => match LoopService::get_loop(conn, &ids[0]) {
            Ok(Some(run)) => Ok(run),
            Ok(None) => Err(format!("loop not found: {id}")),
            Err(e) => Err(e.to_string()),
        },
        n => {
            let previews: Vec<String> = ids.iter().take(5).map(|i| i[..8].to_string()).collect();
            Err(format!(
                "ambiguous loop prefix '{id}' matches {n} loops: {}",
                previews.join(", ")
            ))
        }
    }
}

// ─── Verify ──────────────────────────────────────────────────────────────────

/// Run a verifier gate command and persist the result to the loop.
///
/// This is a thin AXI wrapper around `planeai_core::verifier::run_verifier_gate`.
/// It resolves the loop/session, builds a VerifyGateRequest, calls the primitive,
/// and renders the result as TOON.
pub fn loop_verify(
    conn: &rusqlite::Connection,
    loop_id_arg: &str,
    session_arg: &str,
    name: &str,
    command: &str,
    timeout_ms: u64,
    max_output_bytes: usize,
) -> (String, i32) {
    use planeai_core::loop_service::LoopService;
    use planeai_core::verifier::{self, VerifierLimits, VerifyGateRequest};

    // 1. Resolve loop
    let loop_run = match resolve_loop(conn, loop_id_arg) {
        Ok(r) => r,
        Err(e) => return (emit_error(&e, &[]), 1),
    };

    // 2. Resolve session within the loop
    let loop_sessions = match LoopService::list_loop_sessions(conn, &loop_run.id) {
        Ok(s) => s,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    let session = match resolve_loop_session(&loop_sessions, session_arg) {
        Ok(s) => s,
        Err(e) => return (emit_error(&e, &[]), 1),
    };

    // 3. Resolve project path for artifact root
    let project_path = match db::get_project(conn, &loop_run.project_id) {
        Ok(Some(p)) => p.path,
        Ok(None) => return (emit_error("project not found for this loop", &[]), 1),
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    // 4. Resolve session worktree_path
    let session_worktree_path = db::get_session(conn, &session.session_id)
        .ok()
        .flatten()
        .and_then(|s| s.worktree_path)
        .filter(|wt| !wt.is_empty());

    // 5. Build request and call the structured primitive
    let request = VerifyGateRequest {
        loop_id: loop_run.id.clone(),
        session_id: session.session_id.clone(),
        name: name.to_string(),
        command: command.to_string(),
        project_path,
        session_worktree_path,
        limits: VerifierLimits {
            timeout_ms,
            max_output_bytes,
        },
    };

    match verifier::run_verifier_gate(conn, request) {
        Ok(result) => render_verify_result(&result),
        Err(e) => render_verify_error(&e, &loop_run.id, &session.session_id),
    }
}

fn render_verify_result(result: &planeai_core::verifier::VerifyGateResult) -> (String, i32) {
    use planeai_core::verifier::VerifierStatus;

    let short_loop = &result.loop_id[..std::cmp::min(8, result.loop_id.len())];
    let short_session = &result.session_id[..std::cmp::min(8, result.session_id.len())];

    let mut result_fields = vec![field(
        "verifier",
        Value::Object(vec![
            field("id", str_val(&result.verifier_run_id)),
            field("loop_id", str_val(&result.loop_id)),
            field("session_id", str_val(&result.session_id)),
            field("name", str_val(&result.name)),
            field("status", str_val(result.status.as_str())),
            field(
                "exit_code",
                match result.exit_code {
                    Some(c) => int_val(c as i64),
                    None => Value::Null,
                },
            ),
            field(
                "output_path",
                match &result.output_path {
                    Some(p) => str_val(p),
                    None => Value::Null,
                },
            ),
        ]),
    )];

    let next_actions = match result.status {
        VerifierStatus::Pass => vec![
            format!("run `planeai-cli axi loop observe {short_loop}` to check overall loop state"),
            format!("run `planeai-cli axi loop tick {short_loop}` to advance the loop"),
        ],
        VerifierStatus::Fail => vec![
            format!("inspect output at: {}", result.output_path.as_deref().unwrap_or("(not written)")),
            format!("fix the issue and re-run `planeai-cli axi loop verify --loop-id {short_loop} --session {short_session} --name {} --command \"...\"`", result.name),
        ],
        VerifierStatus::Error => vec![
            format!("check command syntax and working directory"),
            format!("working directory was: {}", result.cwd),
        ],
    };
    result_fields.push(field("next_actions", Value::List(next_actions)));

    let exit = if result.status == VerifierStatus::Pass {
        0
    } else {
        1
    };
    (render(&result_fields), exit)
}

fn render_verify_error(
    err: &planeai_core::verifier::VerifyGateError,
    _loop_id: &str,
    _session_id: &str,
) -> (String, i32) {
    use planeai_core::verifier::VerifyGateError;

    match err {
        VerifyGateError::CwdUnavailable {
            reason,
            session_id: sid,
            loop_id: lid,
        } => {
            let fields = vec![
                field("error", str_val("verifier working directory unavailable")),
                field("loop_id", str_val(lid)),
                field("session_id", str_val(sid)),
                field("details", Value::List(vec![reason.clone()])),
                field(
                    "help",
                    Value::List(vec![
                        "recreate the session worktree or run verification against a valid loop session".to_string(),
                    ]),
                ),
            ];
            (render(&fields), 1)
        }
        _ => (emit_error(&err.to_string(), &[]), 1),
    }
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

// ─── Recipe Commands (delegated to axi_recipe module) ────────────────────────

pub use crate::axi_recipe::{recipe_ls, recipe_show, recipe_validate};

// ─── TOON Helpers ────────────────────────────────────────────────────────────

pub(crate) fn emit_error(msg: &str, help: &[String]) -> String {
    let mut fields = vec![field("error", str_val(msg))];
    if !help.is_empty() {
        fields.push(field("help", Value::List(help.to_vec())));
    }
    render(&fields)
}

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
        planeai_core::loop_service::LoopService::migrate(&conn).unwrap();
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

    // ─── Loop tests ──────────────────────────────────────────────────────────

    fn extract_loop_id(toon_output: &str) -> String {
        toon_output
            .lines()
            .find(|l| l.trim().starts_with("id: "))
            .and_then(|l| l.trim().strip_prefix("id: "))
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn loop_create_outputs_toon_with_loop_id_and_status() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let (output, code) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            None,
            "Implement auth",
            3,
            false,
        );
        assert_eq!(code, 0, "output:\n{output}");
        assert!(output.contains("loop:"), "output:\n{output}");
        assert!(output.contains("status: draft"), "output:\n{output}");
        assert!(
            output.contains("recipe_id: maker-verifier"),
            "output:\n{output}"
        );
        assert!(output.contains("goal: Implement auth"), "output:\n{output}");
        assert!(output.contains("max_rounds: 3"), "output:\n{output}");
        assert!(output.contains("current_round: 0"), "output:\n{output}");
        assert!(output.contains("next_actions[1]:"), "output:\n{output}");
        assert!(
            output.contains("planeai-cli axi loop tick"),
            "output:\n{output}"
        );
        // ID should be a valid UUID prefix (8 hex chars)
        assert!(output.contains("id: "), "output:\n{output}");
    }

    #[test]
    fn loop_create_with_start_outputs_running_status() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let (output, code) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            None,
            "Build feature",
            5,
            true,
        );
        assert_eq!(code, 0, "output:\n{output}");
        assert!(output.contains("status: running"), "output:\n{output}");
        // Recipe's policy overrides CLI max_rounds
        assert!(output.contains("max_rounds: 3"), "output:\n{output}");
    }

    #[test]
    fn loop_create_with_session_id_env_stores_parent() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        // Set the env var for this test
        std::env::set_var("PLANEAI_SESSION_ID", "parent-session-1234");
        let (output, code) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            None,
            "Goal",
            3,
            false,
        );
        std::env::remove_var("PLANEAI_SESSION_ID");

        assert_eq!(code, 0, "output:\n{output}");
        // Verify via observe that created_by_session_id is stored
        // Extract loop ID from the output
        let id_line = output
            .lines()
            .find(|l| l.trim().starts_with("id: "))
            .unwrap();
        let loop_id = id_line.trim().strip_prefix("id: ").unwrap();

        let (obs_output, obs_code) = loop_observe(&conn, loop_id, 20);
        assert_eq!(obs_code, 0, "observe output:\n{obs_output}");
        assert!(
            obs_output.contains("created_by_session_id: parent-session-1234"),
            "observe output:\n{obs_output}"
        );
    }

    #[test]
    fn loop_create_validates_task_key() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        // Insert a task directly so it exists in the same DB
        conn.execute(
            "INSERT OR IGNORE INTO task_projects (prefix) VALUES ('MYA')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (key, project_prefix, title, status, created_at, updated_at)
             VALUES ('MYA-1', 'MYA', 'Real task', 'todo', '2026-01-01', '2026-01-01')",
            [],
        )
        .unwrap();

        // Valid task key works
        let (output, code) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            Some("MYA-1"),
            "maker-verifier",
            None,
            "Goal",
            3,
            false,
        );
        assert_eq!(code, 0, "output:\n{output}");
        assert!(output.contains("task_key: MYA-1"), "output:\n{output}");

        // Invalid task key fails
        let (output, code) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            Some("NONEXIST-999"),
            "maker-verifier",
            None,
            "Goal",
            3,
            false,
        );
        assert_eq!(code, 1, "output:\n{output}");
        assert!(output.contains("task not found"), "output:\n{output}");
    }

    #[test]
    fn loop_create_rejects_invalid_max_rounds() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let (output, code) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            None,
            "Goal",
            0,
            false,
        );
        assert_eq!(code, 1, "output:\n{output}");
        assert!(
            output.contains("--max-rounds must be >= 1"),
            "output:\n{output}"
        );
    }

    #[test]
    fn loop_observe_returns_status_and_events() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        // Create a loop
        let (create_output, _) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            None,
            "Build auth",
            3,
            false,
        );
        let loop_id = extract_loop_id(&create_output);

        // Observe it
        let (output, code) = loop_observe(&conn, &loop_id, 20);
        assert_eq!(code, 0, "output:\n{output}");
        assert!(output.contains("loop:"), "output:\n{output}");
        assert!(output.contains("status: draft"), "output:\n{output}");
        assert!(
            output.contains("strategy: maker-verifier"),
            "output:\n{output}"
        );
        assert!(output.contains("goal: Build auth"), "output:\n{output}");
        assert!(output.contains("sessions: 0 sessions"), "output:\n{output}");
        // loop_created event should exist from the create call
        assert!(
            output.contains("loop_created"),
            "expected loop_created event, output:\n{output}"
        );
        assert!(output.contains("next_actions"), "output:\n{output}");
    }

    #[test]
    fn loop_tick_appends_event_and_returns_state() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        // Create a draft loop with a non-recipe strategy (legacy path)
        let (create_output, _) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "legacy-strategy",
            None,
            "Goal",
            3,
            false,
        );
        let loop_id = extract_loop_id(&create_output);

        // Tick should transition draft → running
        let (output, code) = loop_tick(&conn, &loop_id);
        assert_eq!(code, 0, "output:\n{output}");
        assert!(output.contains("loop:"), "output:\n{output}");
        assert!(output.contains("status: running"), "output:\n{output}");
        assert!(output.contains("event:"), "output:\n{output}");
        assert!(output.contains("kind: tick"), "output:\n{output}");
        assert!(output.contains("next_actions"), "output:\n{output}");

        // Verify loop_started event was appended via observe
        let (obs_output, _) = loop_observe(&conn, &loop_id, 20);
        assert!(
            obs_output.contains("loop_started"),
            "expected loop_started event, output:\n{obs_output}"
        );
    }

    #[test]
    fn loop_stop_cancels_running_loop() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let (create_output, _) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            None,
            "Goal",
            3,
            true,
        );
        let loop_id = extract_loop_id(&create_output);

        let (output, code) = loop_stop(&conn, &loop_id);
        assert_eq!(code, 0, "output:\n{output}");
        assert!(output.contains("status: cancelled"), "output:\n{output}");
        assert!(output.contains("next_actions"), "output:\n{output}");
        assert!(
            output.contains("Clean up any running sessions manually"),
            "output:\n{output}"
        );
    }

    #[test]
    fn loop_stop_is_idempotent_on_terminal_status() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let (create_output, _) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            None,
            "Goal",
            3,
            true,
        );
        let loop_id = extract_loop_id(&create_output);

        // Stop once
        loop_stop(&conn, &loop_id);
        // Stop again — should be idempotent
        let (output, code) = loop_stop(&conn, &loop_id);
        assert_eq!(code, 0, "output:\n{output}");
        assert!(output.contains("status: cancelled"), "output:\n{output}");
        assert!(output.contains("no-op"), "output:\n{output}");
    }

    #[test]
    fn loop_stop_treats_completed_unreviewed_as_terminal() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let (create_output, _) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            None,
            "Goal",
            3,
            true,
        );
        let loop_id = extract_loop_id(&create_output);

        // Manually transition to completed_unreviewed
        use planeai_core::loop_run::LoopStatus;
        use planeai_core::loop_service::LoopService;
        LoopService::update_loop_status(&conn, &loop_id, LoopStatus::CompletedUnreviewed).unwrap();

        // Stop should be a no-op
        let (output, code) = loop_stop(&conn, &loop_id);
        assert_eq!(code, 0, "output:\n{output}");
        assert!(
            output.contains("status: completed_unreviewed"),
            "output:\n{output}"
        );
        assert!(output.contains("no-op"), "output:\n{output}");
    }

    #[test]
    fn loop_tick_rejects_terminal_status() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let (create_output, _) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            None,
            "Goal",
            3,
            true,
        );
        let loop_id = extract_loop_id(&create_output);

        loop_stop(&conn, &loop_id);

        let (output, code) = loop_tick(&conn, &loop_id);
        assert_eq!(code, 1, "expected error exit code, output:\n{output}");
        assert!(
            output.contains("terminal status"),
            "expected terminal status error, output:\n{output}"
        );
    }

    #[test]
    fn loop_tree_handles_zero_sessions() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let (create_output, _) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            None,
            "Goal",
            3,
            false,
        );
        let loop_id = extract_loop_id(&create_output);

        let (output, code) = loop_tree(&conn, &loop_id);
        assert_eq!(code, 0, "output:\n{output}");
        assert!(output.contains("sessions: 0 sessions"), "output:\n{output}");
    }

    #[test]
    fn loop_prefix_resolution_works() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let (create_output, _) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            None,
            "Goal",
            3,
            false,
        );
        let loop_id = extract_loop_id(&create_output);
        let prefix = &loop_id[..8];

        // Should resolve via prefix
        let (output, code) = loop_observe(&conn, prefix, 20);
        assert_eq!(code, 0, "output:\n{output}");
        assert!(output.contains(&loop_id), "output:\n{output}");
    }

    #[test]
    fn loop_tree_shows_sessions_with_children() {
        let conn = setup_db();
        let project = crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        // Create a loop
        let (create_output, _) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            None,
            "Goal",
            3,
            false,
        );
        let loop_id = extract_loop_id(&create_output);

        // Create sessions and register them with the loop
        let maker_id = "11111111-aaaa-bbbb-cccc-dddddddddddd".to_string();
        let child_id = "22222222-aaaa-bbbb-cccc-dddddddddddd".to_string();

        crate::db::create_session_with_id(
            &conn,
            &maker_id,
            &project.id,
            "Maker",
            None,
            "main",
            None,
            Some("claude"),
            "daemon",
            true,
            None,
            None,
            None,
        )
        .unwrap();

        crate::db::create_session_with_id(
            &conn,
            &child_id,
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
            Some(&maker_id),
        )
        .unwrap();

        // Add maker as a loop session
        use planeai_core::loop_service::{AddLoopSessionParams, LoopService};
        LoopService::add_loop_session(
            &conn,
            AddLoopSessionParams {
                loop_id: loop_id.clone(),
                session_id: maker_id.clone(),
                role: "maker".to_string(),
                round: 0,
                provider: Some("claude".to_string()),
                status: "active".to_string(),
            },
        )
        .unwrap();

        // loop tree should show both the maker and its child
        let (output, code) = loop_tree(&conn, &loop_id[..8]);
        assert_eq!(code, 0, "output:\n{output}");
        assert!(
            output.contains("sessions[2]"),
            "expected 2 sessions (maker + child), output:\n{output}"
        );
        assert!(output.contains("11111111"), "output:\n{output}");
        assert!(output.contains("22222222"), "output:\n{output}");
        assert!(output.contains("Maker"), "output:\n{output}");
        assert!(output.contains("Sub-worker"), "output:\n{output}");
    }

    // ─── Handoff AXI Tests ───────────────────────────────────────────────────

    fn setup_loop_db() -> rusqlite::Connection {
        use planeai_core::services::open_db_at;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let conn = open_db_at(&path).unwrap();
        std::mem::forget(dir);
        conn
    }

    fn create_test_loop_with_session(conn: &rusqlite::Connection) -> (String, String) {
        use planeai_core::loop_run::{LoopStatus, LoopStrategy};
        use planeai_core::loop_service::{AddLoopSessionParams, CreateLoopParams, LoopService};

        let loop_run = LoopService::create_loop(
            conn,
            CreateLoopParams {
                project_id: "proj-1".into(),
                task_key: Some("PLA-201".into()),
                created_by_session_id: None,
                strategy: LoopStrategy::new("maker-verifier"),
                goal: "Test handoff".into(),
                max_rounds: 3,
                policy_json: None,
                budget_json: None,
            },
        )
        .unwrap();

        LoopService::update_loop_status(conn, &loop_run.id, LoopStatus::Running).unwrap();

        let session_id = "aaaabbbb-1111-2222-3333-444455556666".to_string();
        LoopService::add_loop_session(
            conn,
            AddLoopSessionParams {
                loop_id: loop_run.id.clone(),
                session_id: session_id.clone(),
                role: "maker".to_string(),
                round: 1,
                provider: Some("claude".to_string()),
                status: "running".to_string(),
            },
        )
        .unwrap();

        (loop_run.id, session_id)
    }

    #[test]
    fn handoff_path_emits_toon_with_correct_fields() {
        let conn = setup_loop_db();
        let (loop_id, session_id) = create_test_loop_with_session(&conn);
        let cwd = "/tmp/test-project";

        let (output, code) = loop_handoff_path(&conn, &loop_id[..8], &session_id[..8], cwd);
        assert_eq!(code, 0, "output:\n{output}");
        assert!(output.contains("handoff_path:"), "output:\n{output}");
        assert!(
            output.contains(&format!("loop_id: {loop_id}")),
            "output:\n{output}"
        );
        assert!(
            output.contains(&format!("session_id: {session_id}")),
            "output:\n{output}"
        );
        assert!(output.contains("role: maker"), "output:\n{output}");
        assert!(output.contains("handoff.json"), "output:\n{output}");
        assert!(output.contains("next_actions[2]:"), "output:\n{output}");
        assert!(
            output.contains("write a planeai.handoff.v1 JSON file"),
            "output:\n{output}"
        );
    }

    #[test]
    fn handoff_path_fails_for_unknown_session() {
        let conn = setup_loop_db();
        let (loop_id, _) = create_test_loop_with_session(&conn);

        let (output, code) = loop_handoff_path(&conn, &loop_id[..8], "nonexist", "/tmp");
        assert_eq!(code, 1);
        assert!(output.contains("error:"), "output:\n{output}");
        assert!(output.contains("session not found"), "output:\n{output}");
    }

    #[test]
    fn handoff_record_emits_toon_on_success() {
        let conn = setup_loop_db();
        let (loop_id, session_id) = create_test_loop_with_session(&conn);

        // Create a handoff file
        let dir = tempfile::tempdir().unwrap();
        let handoff_path = dir.path().join("handoff.json");
        let handoff_json = serde_json::json!({
            "schema": "planeai.handoff.v1",
            "loop_id": loop_id,
            "session_id": session_id,
            "status": "completed",
            "summary": "Feature implemented",
            "branch": "feat/test",
            "commit": "abc123",
            "changed_files": ["src/main.rs"],
            "risks": ["Might break on Windows"],
            "evidence": [{
                "kind": "test",
                "name": "cargo test",
                "result": "pass",
                "source": "direct"
            }]
        });
        std::fs::write(&handoff_path, handoff_json.to_string()).unwrap();

        // Use the temp dir as the CWD (so path validation passes)
        let cwd = dir.path().to_string_lossy().to_string();

        let (output, code) =
            loop_handoff_record(&conn, &loop_id[..8], &session_id[..8], &handoff_path, &cwd);
        assert_eq!(code, 0, "output:\n{output}");
        assert!(output.contains("handoff_recorded:"), "output:\n{output}");
        assert!(
            output.contains(&format!("loop_id: {loop_id}")),
            "output:\n{output}"
        );
        assert!(
            output.contains(&format!("session_id: {session_id}")),
            "output:\n{output}"
        );
        assert!(
            output.contains("schema: planeai.handoff.v1"),
            "output:\n{output}"
        );
        assert!(output.contains("status: completed"), "output:\n{output}");
        assert!(
            output.contains("loop_status: observing"),
            "output:\n{output}"
        );
        assert!(
            output.contains("session_status: completed"),
            "output:\n{output}"
        );
        assert!(output.contains("state_changed: true"), "output:\n{output}");
        assert!(output.contains("risks[1]:"), "output:\n{output}");
        assert!(
            output.contains("Might break on Windows"),
            "output:\n{output}"
        );
        assert!(output.contains("next_actions[1]:"), "output:\n{output}");

        std::mem::forget(dir);
    }

    #[test]
    fn handoff_record_persists_artifact_and_event() {
        use planeai_core::loop_service::LoopService;

        let conn = setup_loop_db();
        let (loop_id, session_id) = create_test_loop_with_session(&conn);

        let dir = tempfile::tempdir().unwrap();
        let handoff_path = dir.path().join("handoff.json");
        let handoff_json = serde_json::json!({
            "schema": "planeai.handoff.v1",
            "loop_id": loop_id,
            "session_id": session_id,
            "status": "blocked",
            "summary": "Blocked by migration",
            "risks": ["Migration conflict"]
        });
        std::fs::write(&handoff_path, handoff_json.to_string()).unwrap();

        let cwd = dir.path().to_string_lossy().to_string();
        let (_, code) = loop_handoff_record(&conn, &loop_id, &session_id, &handoff_path, &cwd);
        assert_eq!(code, 0);

        // Check event was stored
        let events = LoopService::list_loop_events(&conn, &loop_id).unwrap();
        let handoff_events: Vec<_> = events
            .iter()
            .filter(|e| e.kind == "handoff_recorded")
            .collect();
        assert_eq!(handoff_events.len(), 1);
        assert_eq!(
            handoff_events[0].payload_json["status"].as_str().unwrap(),
            "blocked"
        );

        // Check loop status was updated to blocked
        let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
        assert_eq!(updated.status, planeai_core::loop_run::LoopStatus::Blocked);

        // Check session status was updated
        let sessions = LoopService::list_loop_sessions(&conn, &loop_id).unwrap();
        assert_eq!(sessions[0].status, "blocked");

        std::mem::forget(dir);
    }

    #[test]
    fn handoff_record_fails_on_invalid_json() {
        let conn = setup_loop_db();
        let (loop_id, session_id) = create_test_loop_with_session(&conn);

        let dir = tempfile::tempdir().unwrap();
        let handoff_path = dir.path().join("handoff.json");
        std::fs::write(&handoff_path, "not valid json").unwrap();

        let cwd = dir.path().to_string_lossy().to_string();
        let (output, code) = loop_handoff_record(&conn, &loop_id, &session_id, &handoff_path, &cwd);
        assert_eq!(code, 1);
        assert!(
            output.contains("error: invalid handoff file"),
            "output:\n{output}"
        );
        assert!(output.contains("details["), "output:\n{output}");

        std::mem::forget(dir);
    }

    #[test]
    fn handoff_record_fails_on_id_mismatch() {
        let conn = setup_loop_db();
        let (loop_id, session_id) = create_test_loop_with_session(&conn);

        let dir = tempfile::tempdir().unwrap();
        let handoff_path = dir.path().join("handoff.json");
        let handoff_json = serde_json::json!({
            "schema": "planeai.handoff.v1",
            "loop_id": "wrong_loop_id",
            "session_id": session_id,
            "status": "completed",
            "summary": "Done"
        });
        std::fs::write(&handoff_path, handoff_json.to_string()).unwrap();

        let cwd = dir.path().to_string_lossy().to_string();
        let (output, code) = loop_handoff_record(&conn, &loop_id, &session_id, &handoff_path, &cwd);
        assert_eq!(code, 1);
        assert!(
            output.contains("error: invalid handoff file"),
            "output:\n{output}"
        );
        assert!(output.contains("loop_id mismatch"), "output:\n{output}");

        std::mem::forget(dir);
    }

    #[test]
    fn handoff_record_fails_on_path_outside_project() {
        let conn = setup_loop_db();
        let (loop_id, session_id) = create_test_loop_with_session(&conn);

        // Create file in /tmp but use a different cwd
        let dir = tempfile::tempdir().unwrap();
        let other_dir = tempfile::tempdir().unwrap();
        let handoff_path = dir.path().join("handoff.json");
        std::fs::write(&handoff_path, "{}").unwrap();

        let cwd = other_dir.path().to_string_lossy().to_string();
        let (output, code) = loop_handoff_record(&conn, &loop_id, &session_id, &handoff_path, &cwd);
        assert_eq!(code, 1);
        assert!(
            output.contains("outside the project root"),
            "output:\n{output}"
        );

        std::mem::forget(dir);
        std::mem::forget(other_dir);
    }

    // ─── Verifier Gate Tests ─────────────────────────────────────────────────

    fn create_test_loop_with_session_in_dir(
        conn: &rusqlite::Connection,
        project_path: &str,
        worktree_path: Option<&str>,
    ) -> (String, String) {
        planeai_core::test_fixtures::setup_loop_with_session(conn, project_path, worktree_path)
    }

    #[test]
    fn verify_successful_command_renders_toon_pass() {
        let conn = setup_loop_db();
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_string_lossy().to_string();
        let (loop_id, session_id) =
            create_test_loop_with_session_in_dir(&conn, &project_path, None);

        let (output, code) = loop_verify(
            &conn,
            &loop_id[..8],
            &session_id[..8],
            "echo-test",
            "echo hello",
            planeai_core::verifier::DEFAULT_TIMEOUT_MS,
            planeai_core::verifier::DEFAULT_MAX_OUTPUT_BYTES,
        );
        assert_eq!(code, 0, "output:\n{output}");
        assert!(output.contains("verifier:"), "output:\n{output}");
        assert!(output.contains("name: echo-test"), "output:\n{output}");
        assert!(output.contains("status: pass"), "output:\n{output}");
        assert!(output.contains("exit_code: 0"), "output:\n{output}");
        assert!(output.contains("output_path:"), "output:\n{output}");
        assert!(output.contains("next_actions[2]:"), "output:\n{output}");
        assert!(
            output.contains("planeai-cli axi loop observe"),
            "output:\n{output}"
        );
    }

    #[test]
    fn verify_failing_command_renders_toon_fail() {
        let conn = setup_loop_db();
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_string_lossy().to_string();
        let (loop_id, session_id) =
            create_test_loop_with_session_in_dir(&conn, &project_path, None);

        let (output, code) = loop_verify(
            &conn,
            &loop_id[..8],
            &session_id[..8],
            "failing-test",
            "exit 42",
            planeai_core::verifier::DEFAULT_TIMEOUT_MS,
            planeai_core::verifier::DEFAULT_MAX_OUTPUT_BYTES,
        );
        assert_eq!(code, 1, "output:\n{output}");
        assert!(output.contains("status: fail"), "output:\n{output}");
        assert!(output.contains("exit_code: 42"), "output:\n{output}");
        assert!(output.contains("inspect output at:"), "output:\n{output}");
    }

    #[test]
    fn verify_missing_loop_returns_error() {
        let conn = setup_loop_db();
        let (output, code) = loop_verify(
            &conn,
            "nonexistent",
            "some-session",
            "test",
            "echo hi",
            planeai_core::verifier::DEFAULT_TIMEOUT_MS,
            planeai_core::verifier::DEFAULT_MAX_OUTPUT_BYTES,
        );
        assert_eq!(code, 1);
        assert!(output.contains("error:"), "output:\n{output}");
        assert!(output.contains("loop not found"), "output:\n{output}");
    }

    #[test]
    fn verify_missing_session_returns_error() {
        let conn = setup_loop_db();
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_string_lossy().to_string();
        let (loop_id, _) = create_test_loop_with_session_in_dir(&conn, &project_path, None);

        let (output, code) = loop_verify(
            &conn,
            &loop_id[..8],
            "nonexistent",
            "test",
            "echo hi",
            planeai_core::verifier::DEFAULT_TIMEOUT_MS,
            planeai_core::verifier::DEFAULT_MAX_OUTPUT_BYTES,
        );
        assert_eq!(code, 1);
        assert!(output.contains("error:"), "output:\n{output}");
        assert!(output.contains("session not found"), "output:\n{output}");
    }

    #[test]
    fn verify_missing_worktree_returns_cwd_unavailable_error() {
        let conn = setup_loop_db();
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_string_lossy().to_string();
        let (loop_id, session_id) =
            create_test_loop_with_session_in_dir(&conn, &project_path, Some("/nonexistent/wt"));

        let (output, code) = loop_verify(
            &conn,
            &loop_id[..8],
            &session_id[..8],
            "test",
            "echo hi",
            planeai_core::verifier::DEFAULT_TIMEOUT_MS,
            planeai_core::verifier::DEFAULT_MAX_OUTPUT_BYTES,
        );
        assert_eq!(code, 1);
        assert!(
            output.contains("verifier working directory unavailable"),
            "output:\n{output}"
        );
        assert!(
            output.contains("worktree_path does not exist"),
            "output:\n{output}"
        );
    }

    // ─── Recipe AXI & Tick Runtime Tests ─────────────────────────────────────

    #[test]
    fn recipe_ls_emits_toon() {
        let (output, code) = recipe_ls("/tmp");
        assert_eq!(code, 0, "output:\n{output}");
        assert!(
            output.contains("recipes["),
            "expected recipes table header, output:\n{output}"
        );
        assert!(
            output.contains("maker-verifier"),
            "expected built-in maker-verifier recipe, output:\n{output}"
        );
    }

    #[test]
    fn recipe_show_emits_roles_and_steps() {
        let (output, code) = recipe_show("maker-verifier", "/tmp");
        assert_eq!(code, 0, "output:\n{output}");
        assert!(
            output.contains("recipe:"),
            "expected recipe object header, output:\n{output}"
        );
        assert!(
            output.contains("roles["),
            "expected roles table, output:\n{output}"
        );
        assert!(
            output.contains("steps["),
            "expected steps table, output:\n{output}"
        );
        assert!(
            output.contains("id: maker-verifier"),
            "expected recipe id, output:\n{output}"
        );
        assert!(
            output.contains("valid: true"),
            "expected valid: true, output:\n{output}"
        );
    }

    #[test]
    fn recipe_validate_succeeds() {
        let (output, code) = recipe_validate("maker-verifier", "/tmp");
        assert_eq!(code, 0, "output:\n{output}");
        assert!(
            output.contains("recipe_validation:"),
            "expected recipe_validation header, output:\n{output}"
        );
        assert!(
            output.contains("valid: true"),
            "expected valid: true, output:\n{output}"
        );
    }

    #[test]
    fn recipe_validate_fails_for_nonexistent() {
        let (output, code) = recipe_validate("nonexistent", "/tmp");
        assert_eq!(code, 1, "output:\n{output}");
        assert!(
            output.contains("error"),
            "expected error in output, output:\n{output}"
        );
    }

    #[test]
    fn loop_create_with_recipe_stores_snapshot() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let (output, code) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            Some("maker-verifier"),
            "Build auth",
            3,
            false,
        );
        assert_eq!(code, 0, "output:\n{output}");

        let loop_id = extract_loop_id(&output);
        assert!(
            !loop_id.is_empty(),
            "failed to extract loop_id from output:\n{output}"
        );

        let (obs_output, obs_code) = loop_observe(&conn, &loop_id, 20);
        assert_eq!(obs_code, 0, "observe output:\n{obs_output}");
        assert!(
            obs_output.contains("strategy: maker-verifier"),
            "expected strategy in observe output:\n{obs_output}"
        );
    }

    #[test]
    fn loop_create_strategy_alias_works() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let (output, code) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            None,
            "Build feature",
            3,
            false,
        );
        assert_eq!(code, 0, "output:\n{output}");
        assert!(
            output.contains("loop:"),
            "expected loop TOON object, output:\n{output}"
        );
        assert!(
            output.contains("status: draft"),
            "expected draft status, output:\n{output}"
        );
    }

    #[test]
    fn recipe_tick_session_create_fails_gracefully_when_backend_unavailable() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        // Create a loop with recipe and start=false (draft)
        let (create_output, create_code) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            Some("maker-verifier"),
            "Implement feature",
            3,
            false,
        );
        assert_eq!(create_code, 0, "create output:\n{create_output}");

        let loop_id = extract_loop_id(&create_output);
        assert!(!loop_id.is_empty(), "failed to extract loop_id");

        // First tick: draft->running transition + session.create step fails because
        // /tmp/myapp is not a valid git repo and no backend is available
        let (output, code) = loop_tick(&conn, &loop_id);
        assert_eq!(code, 1, "tick output:\n{output}");
        assert!(
            output.contains("session.create failed"),
            "expected session.create failure message, output:\n{output}"
        );
    }

    #[test]
    fn recipe_tick_session_prompt_fails_when_no_sessions_exist() {
        use planeai_core::loop_recipe::*;
        use planeai_core::loop_recipe_service::*;
        use planeai_core::loop_run::LoopStatus;
        use planeai_core::loop_service::LoopService;
        use std::collections::BTreeMap;

        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        // Create a loop with a snapshot that starts on session.prompt (no sessions exist)
        let steps = vec![RecipeStep {
            id: "prompt_maker".into(),
            kind: STEP_SESSION_PROMPT.into(),
            role: Some("maker".into()),
            prompt: Some("Do the thing".into()),
            from: None,
            on: None,
            status: None,
            next: None,
            select: Some("latest".into()),
            event_kind: None,
            gates: vec![],
        }];

        let snapshot = RecipeSnapshot {
            recipe_schema: RECIPE_SCHEMA_V1.into(),
            recipe_id: "test-recipe".into(),
            recipe_source: "builtin".into(),
            recipe_path: None,
            inputs: BTreeMap::new(),
            runtime: RecipeRuntime {
                current_step: "prompt_maker".into(),
                tick_count: 0,
                round: 1,
                created_session_ids: BTreeMap::new(), // No sessions!
                last_error: None,
            },
            policy: SnapshotPolicy {
                max_rounds: 3,
                max_ticks: 50,
                max_sessions: 5,
                merge_policy: "human".into(),
            },
            roles: BTreeMap::new(),
            steps,
            knowledge: RecipeKnowledge::default(),
            tools: RecipeTools::default(),
        };

        let policy_json = serde_json::to_value(&snapshot).unwrap();

        let loop_run = LoopService::create_loop(
            &conn,
            planeai_core::loop_service::CreateLoopParams {
                project_id: "proj-1".into(),
                task_key: None,
                created_by_session_id: None,
                strategy: planeai_core::loop_run::LoopStrategy::new("test-recipe"),
                goal: "test prompt".into(),
                max_rounds: 3,
                policy_json: Some(policy_json),
                budget_json: None,
            },
        )
        .unwrap();

        LoopService::update_loop_status(&conn, &loop_run.id, LoopStatus::Running).unwrap();

        // Tick — session.prompt should fail because no sessions for role
        let (output, code) = loop_tick(&conn, &loop_run.id);
        assert_eq!(code, 1, "expected failure, output:\n{output}");
        assert!(
            output.contains("no sessions exist for role"),
            "expected no-sessions error, output:\n{output}"
        );
    }

    #[test]
    fn recipe_tick_max_ticks_prevents_runaway() {
        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let (create_output, create_code) = loop_create(
            &conn,
            "/tmp/myapp",
            None,
            None,
            "maker-verifier",
            Some("maker-verifier"),
            "Implement feature",
            3,
            false,
        );
        assert_eq!(create_code, 0, "create output:\n{create_output}");

        let loop_id = extract_loop_id(&create_output);
        assert!(!loop_id.is_empty(), "failed to extract loop_id");

        // Transition to running so tick_recipe is invoked
        use planeai_core::loop_run::LoopStatus;
        use planeai_core::loop_service::LoopService;
        LoopService::update_loop_status(&conn, &loop_id, LoopStatus::Running).unwrap();

        // Set tick_count = max_ticks so next tick is blocked
        conn.execute(
            "UPDATE loop_runs SET policy_json = json_set(policy_json, '$.runtime.tick_count', json_extract(policy_json, '$.policy.max_ticks')) WHERE id = ?1",
            rusqlite::params![loop_id],
        )
        .unwrap();

        // Next tick should fail with max_ticks exceeded
        let (output, code) = loop_tick(&conn, &loop_id);
        assert_eq!(code, 1, "expected failure code, output:\n{output}");
        assert!(
            output.contains("max_ticks"),
            "expected max_ticks error message, output:\n{output}"
        );
    }

    #[test]
    fn recipe_tick_round_next_increments_round() {
        use planeai_core::loop_recipe::*;
        use planeai_core::loop_recipe_service::*;
        use planeai_core::loop_run::LoopStatus;
        use planeai_core::loop_service::LoopService;
        use std::collections::BTreeMap;

        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        // Build a snapshot with a round.next step
        let steps = vec![
            RecipeStep {
                id: "next_round".into(),
                kind: STEP_ROUND_NEXT.into(),
                role: None,
                prompt: None,
                from: None,
                on: None,
                status: None,
                next: Some("after_round".into()),
                select: None,
                event_kind: None,
                gates: vec![],
            },
            RecipeStep {
                id: "after_round".into(),
                kind: STEP_LOOP_EVENT.into(),
                role: None,
                prompt: None,
                from: None,
                on: None,
                status: None,
                next: None,
                select: None,
                event_kind: Some("post_round".into()),
                gates: vec![],
            },
        ];

        let snapshot = RecipeSnapshot {
            recipe_schema: RECIPE_SCHEMA_V1.into(),
            recipe_id: "test-recipe".into(),
            recipe_source: "builtin".into(),
            recipe_path: None,
            inputs: BTreeMap::new(),
            runtime: RecipeRuntime {
                current_step: "next_round".into(),
                tick_count: 0,
                round: 1,
                created_session_ids: BTreeMap::new(),
                last_error: None,
            },
            policy: SnapshotPolicy {
                max_rounds: 3,
                max_ticks: 50,
                max_sessions: 5,
                merge_policy: "human".into(),
            },
            roles: BTreeMap::new(),
            steps,
            knowledge: RecipeKnowledge::default(),
            tools: RecipeTools::default(),
        };

        let policy_json = serde_json::to_value(&snapshot).unwrap();

        // Create loop with this snapshot
        let loop_run = LoopService::create_loop(
            &conn,
            planeai_core::loop_service::CreateLoopParams {
                project_id: "proj-1".into(),
                task_key: None,
                created_by_session_id: None,
                strategy: planeai_core::loop_run::LoopStrategy::new("test-recipe"),
                goal: "test round".into(),
                max_rounds: 3,
                policy_json: Some(policy_json),
                budget_json: None,
            },
        )
        .unwrap();

        // Move to running
        LoopService::update_loop_status(&conn, &loop_run.id, LoopStatus::Running).unwrap();

        // Tick — should execute round.next
        let (output, code) = loop_tick(&conn, &loop_run.id);
        assert_eq!(code, 0, "round.next should succeed, output:\n{output}");
        assert!(output.contains("round.next"), "output:\n{output}");

        // Verify round was incremented in DB
        let updated = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
        assert_eq!(updated.current_round, 2, "current_round should be 2");

        // Verify snapshot runtime.round was updated
        let snap: RecipeSnapshot = serde_json::from_value(updated.policy_json.unwrap()).unwrap();
        assert_eq!(snap.runtime.round, 2);
        assert_eq!(snap.runtime.current_step, "after_round");
    }

    #[test]
    fn recipe_tick_round_next_enforces_max_rounds() {
        use planeai_core::loop_recipe::*;
        use planeai_core::loop_recipe_service::*;
        use planeai_core::loop_run::LoopStatus;
        use planeai_core::loop_service::LoopService;
        use std::collections::BTreeMap;

        let conn = setup_db();
        crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let steps = vec![RecipeStep {
            id: "next_round".into(),
            kind: STEP_ROUND_NEXT.into(),
            role: None,
            prompt: None,
            from: None,
            on: None,
            status: None,
            next: Some("start".into()),
            select: None,
            event_kind: None,
            gates: vec![],
        }];

        let snapshot = RecipeSnapshot {
            recipe_schema: RECIPE_SCHEMA_V1.into(),
            recipe_id: "test-recipe".into(),
            recipe_source: "builtin".into(),
            recipe_path: None,
            inputs: BTreeMap::new(),
            runtime: RecipeRuntime {
                current_step: "next_round".into(),
                tick_count: 5,
                round: 3, // Already at max_rounds
                created_session_ids: BTreeMap::new(),
                last_error: None,
            },
            policy: SnapshotPolicy {
                max_rounds: 3,
                max_ticks: 50,
                max_sessions: 5,
                merge_policy: "human".into(),
            },
            roles: BTreeMap::new(),
            steps,
            knowledge: RecipeKnowledge::default(),
            tools: RecipeTools::default(),
        };

        let policy_json = serde_json::to_value(&snapshot).unwrap();

        let loop_run = LoopService::create_loop(
            &conn,
            planeai_core::loop_service::CreateLoopParams {
                project_id: "proj-1".into(),
                task_key: None,
                created_by_session_id: None,
                strategy: planeai_core::loop_run::LoopStrategy::new("test-recipe"),
                goal: "test limit".into(),
                max_rounds: 3,
                policy_json: Some(policy_json),
                budget_json: None,
            },
        )
        .unwrap();

        LoopService::update_loop_status(&conn, &loop_run.id, LoopStatus::Running).unwrap();

        // Tick — should fail because we're at max_rounds
        let (output, code) = loop_tick(&conn, &loop_run.id);
        assert_eq!(
            code, 0,
            "round.next at limit should return code 0 (sets needs_human), output:\n{output}"
        );
        assert!(
            output.contains("needs_human") || output.contains("max_rounds"),
            "expected max_rounds limit message, output:\n{output}"
        );

        // Verify loop status is now needs_human
        let updated = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
        assert_eq!(updated.status, LoopStatus::NeedsHuman);
    }
}
