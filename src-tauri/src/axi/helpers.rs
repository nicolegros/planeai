//! Shared helpers used across AXI subcommands.

use planeai_tasks::model::Task;
use planeai_toon::{field, int_val, render, str_val, Value};

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
