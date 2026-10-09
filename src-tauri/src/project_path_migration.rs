//! Rewrites absolute paths stored under a project folder to project-relative ones, so a
//! project whose folder moves keeps its loop artifacts, verifier logs and editor tabs.
//! Idempotent: rows already relative, or outside their project, are left untouched.

use std::collections::HashMap;

use planeai_core::project_path::encode;
use rusqlite::{params, Connection, Result};
use serde_json::Value;

const EDITOR_INFIX: &str = ":editor:";

pub fn migrate(conn: &Connection) -> Result<()> {
    let projects: HashMap<String, String> = conn
        .prepare("SELECT id, path FROM projects")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_>>()?;

    // A worktree session redirected into the main checkout is a checkout session.
    for (id, path) in &projects {
        conn.execute(
            "UPDATE sessions SET worktree_path = NULL WHERE project_id = ?1 AND worktree_path = ?2",
            params![id, path],
        )?;
    }

    relativize_loop_paths(conn, &projects)?;
    relativize_editor_tabs(conn, &projects)
}

fn relativize_loop_paths(conn: &Connection, projects: &HashMap<String, String>) -> Result<()> {
    let loops: Vec<(String, String, Option<String>)> = conn
        .prepare("SELECT id, project_id, policy_json FROM loop_runs")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_>>()?;

    for (loop_id, project_id, policy_json) in loops {
        let Some(root) = projects.get(&project_id) else {
            continue;
        };
        if let Some(policy) = policy_json.as_deref().and_then(parse_json) {
            if let Some(updated) = relativize_keys(root, policy, &["recipe_path"]) {
                conn.execute(
                    "UPDATE loop_runs SET policy_json = ?1 WHERE id = ?2",
                    params![updated.to_string(), loop_id],
                )?;
            }
        }
        relativize_column(conn, root, "verifier_runs", "output_path", &loop_id)?;
        relativize_column(conn, root, "loop_artifacts", "path", &loop_id)?;

        let events: Vec<(i64, String)> = conn
            .prepare("SELECT id, payload_json FROM loop_events WHERE loop_id = ?1")?
            .query_map([&loop_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_>>()?;
        for (event_id, payload) in events {
            let Some(payload) = parse_json(&payload) else {
                continue;
            };
            if let Some(updated) = relativize_keys(root, payload, &["path", "output_path"]) {
                conn.execute(
                    "UPDATE loop_events SET payload_json = ?1 WHERE id = ?2",
                    params![updated.to_string(), event_id],
                )?;
            }
        }
    }
    Ok(())
}

fn relativize_column(
    conn: &Connection,
    root: &str,
    table: &str,
    column: &str,
    loop_id: &str,
) -> Result<()> {
    let rows: Vec<(String, String)> = conn
        .prepare(&format!(
            "SELECT id, {column} FROM {table} WHERE loop_id = ?1 AND {column} IS NOT NULL"
        ))?
        .query_map([loop_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_>>()?;
    for (id, path) in rows {
        let stored = encode(root, &path);
        if stored != path {
            conn.execute(
                &format!("UPDATE {table} SET {column} = ?1 WHERE id = ?2"),
                params![stored, id],
            )?;
        }
    }
    Ok(())
}

fn parse_json(text: &str) -> Option<Value> {
    serde_json::from_str(text).ok()
}

/// The object with its string `keys` made project-relative, or None when nothing changed.
fn relativize_keys(root: &str, mut object: Value, keys: &[&str]) -> Option<Value> {
    let mut changed = false;
    for key in keys {
        if let Some(Value::String(path)) = object.get_mut(*key) {
            let stored = encode(root, path);
            if stored != *path {
                *path = stored;
                changed = true;
            }
        }
    }
    changed.then_some(object)
}

fn relativize_editor_tabs(conn: &Connection, projects: &HashMap<String, String>) -> Result<()> {
    let working_dirs: HashMap<String, String> = conn
        .prepare("SELECT id, project_id, worktree_path FROM sessions")?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .filter_map(|row| {
            let (id, project_id, worktree) = row.ok()?;
            let dir = worktree.or_else(|| projects.get(&project_id).cloned())?;
            Some((id, dir))
        })
        .collect();

    for table in ["task_workspaces", "session_layouts"] {
        let layouts: Vec<(i64, String)> = conn
            .prepare(&format!(
                "SELECT rowid, layout_json FROM {table} WHERE instr(layout_json, ?1) > 0"
            ))?
            .query_map([EDITOR_INFIX], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_>>()?;
        for (rowid, layout_json) in layouts {
            let Some(mut layout) = parse_json(&layout_json) else {
                continue;
            };
            if relativize_layout(&mut layout, &working_dirs) {
                conn.execute(
                    &format!("UPDATE {table} SET layout_json = ?1 WHERE rowid = ?2"),
                    params![layout.to_string(), rowid],
                )?;
            }
        }
    }
    Ok(())
}

/// Rewrites editor tabs whose file lives under their session's working dir. Returns
/// whether anything changed.
fn relativize_layout(node: &mut Value, working_dirs: &HashMap<String, String>) -> bool {
    let mut renamed: Vec<(String, String)> = Vec::new();
    if let Some(tabs) = node.get_mut("tabs").and_then(Value::as_array_mut) {
        for tab in tabs {
            if let Some(rename) = relativize_editor_tab(tab, working_dirs) {
                renamed.push(rename);
            }
        }
    }
    if let Some(Value::String(active)) = node.get_mut("activeTab") {
        if let Some((_, new_key)) = renamed.iter().find(|(old, _)| old == active) {
            *active = new_key.clone();
        }
    }
    let mut changed = !renamed.is_empty();
    for child in ["tree", "children"] {
        match node.get_mut(child) {
            Some(Value::Array(children)) => {
                for child in children {
                    changed |= relativize_layout(child, working_dirs);
                }
            }
            Some(child @ Value::Object(_)) => changed |= relativize_layout(child, working_dirs),
            _ => {}
        }
    }
    changed
}

/// The tab's (old, new) pty key when its absolute file path was made relative.
fn relativize_editor_tab(
    tab: &mut Value,
    working_dirs: &HashMap<String, String>,
) -> Option<(String, String)> {
    let pty_key = tab.get("ptyKey")?.as_str()?.to_string();
    let (session_id, file_path) = pty_key.split_once(EDITOR_INFIX)?;
    let dir = working_dirs.get(session_id)?;
    let relative = encode(dir, file_path);
    if relative == file_path || relative == "." {
        return None;
    }
    let new_key = format!("{session_id}{EDITOR_INFIX}{relative}");
    tab["ptyKey"] = Value::String(new_key.clone());
    tab["filePath"] = Value::String(relative);
    Some((pty_key, new_key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO projects (id, name, path, prefix) VALUES ('p1', 'app', '/repos/app', 'APP');
             INSERT INTO sessions (id, project_id, name, branch, status, created_at, worktree_path, backend)
               VALUES ('s-main', 'p1', 'main', 'main', 'active', 'now', '/repos/app', 'local'),
                      ('s-wt', 'p1', 'wt', 'feat', 'active', 'now', '/home/u/.planeai/worktrees/app/ab12', 'local');
             INSERT INTO loop_runs (id, project_id, strategy, goal, status, max_rounds, created_at, updated_at, policy_json)
               VALUES ('l1', 'p1', 'maker-verifier', 'g', 'running', 3, 'now', 'now',
                       '{\"recipe_path\":\"/repos/app/.planeai/loops/r.yaml\",\"recipe_id\":\"r\"}');
             INSERT INTO verifier_runs (id, loop_id, verifier_type, name, command, status, output_path, created_at)
               VALUES ('v1', 'l1', 'command', 't', 'true', 'pass', '/repos/app/.planeai/loops/l1/verifiers/v1.log', 'now');
             INSERT INTO loop_artifacts (id, loop_id, kind, path, created_at)
               VALUES ('a1', 'l1', 'handoff', '/repos/app/.planeai/h.json', 'now'),
                      ('a2', 'l1', 'handoff', '/home/u/.planeai/worktrees/app/ab12/h.json', 'now');
             INSERT INTO loop_events (loop_id, ts, kind, payload_json)
               VALUES ('l1', 'now', 'verifier_completed',
                       '{\"output_path\":\"/repos/app/.planeai/loops/l1/verifiers/v1.log\",\"cwd\":\"/repos/app\"}');",
        )
        .unwrap();
        let layout = serde_json::json!({
            "tree": {"type": "leaf", "id": "leaf-1", "activeTab": "s-wt:editor:/home/u/.planeai/worktrees/app/ab12/src/lib.rs", "tabs": [
                {"ptyKey": "s-wt", "type": "agent", "label": "wt", "icon": "bot"},
                {"ptyKey": "s-wt:editor:/home/u/.planeai/worktrees/app/ab12/src/lib.rs", "type": "editor",
                 "label": "lib.rs", "icon": "file", "filePath": "/home/u/.planeai/worktrees/app/ab12/src/lib.rs"},
                {"ptyKey": "s-main:editor:/etc/hosts", "type": "editor",
                 "label": "hosts", "icon": "file", "filePath": "/etc/hosts"}
            ]},
            "focusedLeafId": "leaf-1"
        });
        conn.execute(
            "INSERT INTO task_workspaces (project_id, task_key, layout_json) VALUES ('p1', 'APP-1', ?1)",
            [layout.to_string()],
        )
        .unwrap();
        let main_layout = serde_json::json!({
            "tree": {"type": "split", "id": "sp", "direction": "horizontal", "ratio": 0.5, "children": [
                {"type": "leaf", "id": "a", "activeTab": "s-main", "tabs": [{"ptyKey": "s-main", "type": "agent"}]},
                {"type": "leaf", "id": "b", "activeTab": "s-main:editor:/repos/app/README.md", "tabs": [
                    {"ptyKey": "s-main:editor:/repos/app/README.md", "type": "editor", "filePath": "/repos/app/README.md"}
                ]}
            ]},
            "focusedLeafId": "b"
        });
        conn.execute(
            "INSERT INTO session_layouts (session_id, layout_json, updated_at) VALUES ('s-main', ?1, 'now')",
            [main_layout.to_string()],
        )
        .unwrap();
        conn
    }

    fn text(conn: &Connection, sql: &str) -> String {
        conn.query_row(sql, [], |row| row.get(0)).unwrap()
    }

    fn snapshot(conn: &Connection) -> Vec<String> {
        [
            "SELECT IFNULL(worktree_path, 'NULL') FROM sessions WHERE id = 's-main'",
            "SELECT worktree_path FROM sessions WHERE id = 's-wt'",
            "SELECT json_extract(policy_json, '$.recipe_path') FROM loop_runs",
            "SELECT output_path FROM verifier_runs",
            "SELECT group_concat(path, ' | ') FROM (SELECT path FROM loop_artifacts ORDER BY id)",
            "SELECT json_extract(payload_json, '$.output_path') || ' cwd=' || json_extract(payload_json, '$.cwd') FROM loop_events",
            "SELECT json_extract(layout_json, '$.tree.activeTab') FROM task_workspaces",
            "SELECT json_extract(layout_json, '$.tree.tabs[1].filePath') FROM task_workspaces",
            "SELECT json_extract(layout_json, '$.tree.tabs[2].ptyKey') FROM task_workspaces",
            "SELECT json_extract(layout_json, '$.tree.children[1].activeTab') FROM session_layouts",
            "SELECT json_extract(layout_json, '$.tree.children[1].tabs[0].filePath') FROM session_layouts",
        ]
        .iter()
        .map(|sql| text(conn, sql))
        .collect()
    }

    #[test]
    fn paths_under_a_project_become_relative_and_rerunning_changes_nothing() {
        let conn = setup();

        migrate(&conn).unwrap();
        let first = snapshot(&conn);
        migrate(&conn).unwrap();

        assert_eq!(
            first,
            vec![
                "NULL",
                "/home/u/.planeai/worktrees/app/ab12",
                ".planeai/loops/r.yaml",
                ".planeai/loops/l1/verifiers/v1.log",
                ".planeai/h.json | /home/u/.planeai/worktrees/app/ab12/h.json",
                ".planeai/loops/l1/verifiers/v1.log cwd=/repos/app",
                "s-wt:editor:src/lib.rs",
                "src/lib.rs",
                "s-main:editor:/etc/hosts",
                "s-main:editor:README.md",
                "README.md",
            ]
        );
        assert_eq!(snapshot(&conn), first);
    }
}
