//! Shared helpers used across AXI subcommands.

use planeai_tasks::model::Task;
use planeai_toon::{field, int_val, render, str_val, Value};

use crate::db;

/// Render a TOON error response with optional help hints.
pub(crate) fn emit_error(msg: &str, help: &[String]) -> String {
    let mut fields = vec![field("error", str_val(msg))];
    if !help.is_empty() {
        fields.push(field("help", Value::List(help.to_vec())));
    }
    render(&fields)
}

/// Build a TOON Value::Object for a task's full detail.
pub(crate) fn task_detail_object(task: &Task) -> Value {
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

/// Truncate a description string to a max length, appending an indicator if truncated.
pub(crate) fn truncate_desc(s: &str, limit: usize) -> String {
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

/// Resolve a project from --project flag or CWD, returning (id, prefix, name, path).
pub(crate) fn resolve_project(
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
pub(crate) fn resolve_loop(
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

/// Resolve a session within a loop's sessions by exact or prefix match.
pub(crate) fn resolve_loop_session(
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
pub(crate) fn resolve_handoff_base_path(
    conn: &rusqlite::Connection,
    session_id: &str,
    cwd: &str,
) -> String {
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
