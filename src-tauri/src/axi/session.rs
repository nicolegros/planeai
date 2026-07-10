//! AXI session subcommands — read, create, ls, prompt, children, tree.

use planeai_toon::{field, int_val, render, str_val, Value};

use super::helpers::emit_error;

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
