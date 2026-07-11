//! AXI loop subcommands — create, observe, tick, stop, tree, handoff, verify.

use planeai_toon::{field, int_val, render, str_val, Value};

use super::helpers::{
    emit_error, resolve_handoff_base_path, resolve_loop, resolve_loop_session, resolve_project,
};

// ─── Loop Create ─────────────────────────────────────────────────────────────

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
    use planeai_core::loop_run::{LoopStrategy, LoopTrigger};
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
        if let Err(e) = LoopService::transition_loop(conn, &loop_run.id, LoopTrigger::Start) {
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

// ─── Loop Observe ────────────────────────────────────────────────────────────

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

// ─── Loop Tick ───────────────────────────────────────────────────────────────

pub fn loop_tick(conn: &rusqlite::Connection, id: &str) -> (String, i32) {
    use planeai_core::loop_recipe_service::RecipeSnapshot;
    use planeai_core::loop_run::{LoopStatus, LoopTrigger};
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
        if let Err(e) = LoopService::transition_loop(conn, &loop_run.id, LoopTrigger::Start) {
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

// ─── Loop Stop ───────────────────────────────────────────────────────────────

pub fn loop_stop(conn: &rusqlite::Connection, id: &str) -> (String, i32) {
    use planeai_core::loop_run::{LoopStatus, LoopTrigger};
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
    if let Err(e) = LoopService::transition_loop(conn, &loop_run.id, LoopTrigger::Cancel) {
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

// ─── Loop Tree ───────────────────────────────────────────────────────────────

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
    use planeai_core::loop_run::LoopTrigger;
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

    // Always pass the trigger — the transition table handles state validation
    // (rejects from Draft/terminal states, no-ops from matching states).
    let trigger = Some(LoopTrigger::HandoffReceived(handoff.status.clone()));

    // Atomically record: artifact + event + session status + loop transition
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
            trigger: trigger.clone(),
        },
    ) {
        Ok(r) => r,
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    // Determine final status for output — re-read from DB since transition_in_tx may have changed it
    let final_loop_status = LoopService::get_loop(conn, &loop_run.id)
        .ok()
        .flatten()
        .map(|r| r.status)
        .unwrap_or(loop_run.status.clone());
    let state_changed = trigger.is_some() && final_loop_status != loop_run.status;

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

    // Auto-advance: when a completed handoff arrives on a non-terminal loop,
    // advance the recipe through gates → retry → prompt synchronously.
    // The agent will see the handoff record output only after this function returns,
    // but any prompts sent by auto_advance (via notify socket / daemon) will be
    // queued in the PTY buffer and processed by the agent after it reads our output.
    if handoff.status == HandoffStatus::Completed && !final_loop_status.is_executor_terminal() {
        if let Ok(Some(updated_run)) = LoopService::get_loop(conn, &loop_run.id) {
            if let Some(ref policy_json) = updated_run.policy_json {
                if let Ok(mut snapshot) = serde_json::from_value::<
                    planeai_core::loop_recipe_service::RecipeSnapshot,
                >(policy_json.clone())
                {
                    crate::recipe_tick::auto_advance(conn, &loop_run.id, &mut snapshot, true);

                    // Save final snapshot state
                    let updated_json = serde_json::to_value(&snapshot).unwrap_or_default();
                    let _ = LoopService::update_policy_json(conn, &loop_run.id, &updated_json);
                }
            }
        }
    }

    (render(&result_fields), 0)
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
    let project_path = match crate::db::get_project(conn, &loop_run.project_id) {
        Ok(Some(p)) => p.path,
        Ok(None) => return (emit_error("project not found for this loop", &[]), 1),
        Err(e) => return (emit_error(&e.to_string(), &[]), 1),
    };

    // 4. Resolve session worktree_path
    let session_worktree_path = crate::db::get_session(conn, &session.session_id)
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
