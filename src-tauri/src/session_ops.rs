#![allow(dead_code)]

use rusqlite::Connection;

use crate::cleanup::{CleanupContext, CleanupOps, KillOps};
use crate::config::Config;
use crate::db::{self, Session};

const MIN_PREFIX_LEN: usize = 4;

#[derive(Debug, PartialEq)]
pub enum ResolveError {
    TooShort,
    NotFound(String),
    Ambiguous(Vec<String>),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort => write!(f, "prefix must be at least {MIN_PREFIX_LEN} characters"),
            Self::NotFound(p) => write!(f, "no session matching prefix: {p}"),
            Self::Ambiguous(ids) => {
                let short: Vec<&str> = ids.iter().map(|id| &id[..8]).collect();
                write!(f, "ambiguous prefix, matches: {}", short.join(", "))
            }
        }
    }
}

pub struct DestroyResult {
    pub session: Session,
    pub cleanup_errors: Vec<String>,
}

pub fn archive(
    conn: &Connection,
    id: &str,
    config: &Option<Config>,
    kill_ops: &KillOps,
) -> Result<Session, String> {
    let session = db::get_session(conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("session not found: {id}"))?;

    eprintln!(
        "[session] archiving \"{}\" (id={}, status={})",
        session.name,
        &id[..8],
        session.status
    );
    tracing::info!(name = %session.name, id = &id[..8], status = %session.status, "archiving session");

    // Fire task hook before mutation
    if let (Some(cfg), Some(ref key)) = (config, &session.task_key) {
        if let Some(cwd) = session_cwd(conn, &session) {
            eprintln!("[session] firing on_complete hook for task {key}");
            tracing::info!(task_key = %key, "firing on_complete hook");
            fire_task_hook(cfg, &session, "on_complete", &cwd, conn);
        }
    }

    // Kill the agent backend process.
    // Errors are logged but not propagated — a failed kill should not block
    // archiving since the session may already be dead (exited, crashed, etc).
    let kill_errors = crate::cleanup::kill_backend(
        &session.backend,
        session.tmux_name.as_deref(),
        Some(session.id.as_str()),
        session.tab_count,
        kill_ops,
    );
    if !kill_errors.is_empty() {
        eprintln!("[session] kill errors during archive: {:?}", kill_errors);
        tracing::warn!(?kill_errors, "errors killing backend during archive");
    }

    db::archive_session(conn, id).map_err(|e| e.to_string())?;
    eprintln!("[session] archived \"{}\"", session.name);
    tracing::info!(name = %session.name, "session archived");

    Ok(session)
}

/// Archive all active/exited sessions linked to a task key.
/// Returns the number of sessions archived.
pub fn archive_sessions_for_task(
    conn: &Connection,
    task_key: &str,
    config: &Option<Config>,
) -> usize {
    let sessions = match planeai_core::services::SessionService::list_by_task_key(conn, task_key) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(task_key = %task_key, error = %e, "failed to list sessions for task");
            return 0;
        }
    };

    if sessions.is_empty() {
        return 0;
    }

    let kill_ops = crate::cleanup::real_kill_ops();
    let mut count = 0;

    for session in &sessions {
        tracing::info!(
            session_id = %&session.id[..8],
            session_name = %session.name,
            task_key = %task_key,
            "archiving session — task moved to done"
        );
        if let Err(e) = archive(conn, &session.id, config, &kill_ops) {
            tracing::warn!(
                session_id = %&session.id[..8],
                error = %e,
                "failed to archive session for done task"
            );
        } else {
            count += 1;
        }
    }

    count
}

pub fn list(conn: &Connection, archived: bool) -> Result<Vec<Session>, String> {
    if archived {
        db::list_archived_sessions(conn).map_err(|e| e.to_string())
    } else {
        db::list_sessions(conn).map_err(|e| e.to_string())
    }
}

pub fn format_table(sessions: &[Session], projects: &[db::Project]) -> String {
    let headers = ["ID", "NAME", "PROJECT", "BRANCH", "STATUS", "CREATED"];

    let rows: Vec<[String; 6]> = sessions
        .iter()
        .map(|s| {
            let project_name = projects
                .iter()
                .find(|p| p.id == s.project_id)
                .map(|p| p.name.as_str())
                .unwrap_or("?");
            let date = &s.created_at[..10.min(s.created_at.len())];
            [
                s.id[..8].to_string(),
                s.name.clone(),
                project_name.to_string(),
                s.branch.clone(),
                s.status.clone(),
                date.to_string(),
            ]
        })
        .collect();

    // Compute column widths
    let mut widths = headers.map(|h| h.len());
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }

    let mut out = String::new();
    // Header
    for (i, h) in headers.iter().enumerate() {
        if i > 0 {
            out.push_str("  ");
        }
        out.push_str(&format!("{:<width$}", h, width = widths[i]));
    }
    out.push('\n');
    // Rows
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                out.push_str("  ");
            }
            out.push_str(&format!("{:<width$}", cell, width = widths[i]));
        }
        out.push('\n');
    }

    out
}

pub fn destroy(
    conn: &Connection,
    id: &str,
    config: &Option<Config>,
    cleanup_ops: &CleanupOps,
) -> Result<DestroyResult, String> {
    let session = db::get_session(conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("session not found: {id}"))?;

    // Fire task hook before mutation
    if let (Some(cfg), Some(_)) = (config, &session.task_key) {
        if let Some(cwd) = session_cwd(conn, &session) {
            fire_task_hook(cfg, &session, "on_complete", &cwd, conn);
        }
    }

    // Soft-delete
    db::destroy_session(conn, id).map_err(|e| e.to_string())?;

    // Run cleanup
    let project_path = db::get_project(conn, &session.project_id)
        .ok()
        .flatten()
        .map(|p| p.path);

    let ctx = CleanupContext {
        backend: session.backend.clone(),
        tmux_name: session.tmux_name.clone(),
        worktree_path: session.worktree_path.clone(),
        project_path: project_path.clone(),
        branch: if session.worktree_path.is_some() {
            Some(session.branch.clone())
        } else {
            None
        },
        session_id: Some(session.id.clone()),
        tab_count: session.tab_count,
    };
    let cleanup_errors = crate::cleanup::run_cleanup(&ctx, cleanup_ops);

    Ok(DestroyResult {
        session,
        cleanup_errors,
    })
}

pub fn session_cwd(conn: &Connection, session: &Session) -> Option<String> {
    if let Some(ref wt) = session.worktree_path {
        return Some(wt.clone());
    }
    db::get_project(conn, &session.project_id)
        .ok()
        .flatten()
        .map(|p| p.path)
}

/// Fire a task manager lifecycle hook (on_start, on_notify, on_restart, on_complete).
/// Uses the caller's connection — no new DB connections opened.
pub fn fire_task_hook(
    cfg: &Config,
    session: &Session,
    hook_name: &str,
    cwd: &str,
    conn: &Connection,
) {
    let task_key = match &session.task_key {
        Some(k) => k,
        None => return,
    };
    let tm = match cfg.task_management.as_ref() {
        Some(tm) => tm,
        None => return,
    };
    let hook = match hook_name {
        "on_start" => tm.on_start.as_ref(),
        "on_notify" => tm.on_notify.as_ref(),
        "on_restart" => tm.on_restart.as_ref(),
        "on_complete" => tm.on_complete.as_ref(),
        _ => None,
    };
    if let Some(h) = hook {
        let db_path = planeai_paths::db_path();
        let projects = db::list_projects(conn).unwrap_or_default();
        let prefix = projects
            .iter()
            .find(|p| cwd.starts_with(&p.path))
            .map(|p| p.prefix.clone())
            .unwrap_or_default();
        if !prefix.is_empty() {
            if let Ok(repo) = planeai_tasks::sqlite::SqliteRepository::open(
                db_path.to_str().unwrap_or_default(),
                &prefix,
            ) {
                use planeai_tasks::model::{Status, UpdateParams};
                use planeai_tasks::provider::TaskProvider;
                if let Some(s) = Status::parse(&h.move_to) {
                    let _ = repo.update(
                        task_key,
                        UpdateParams {
                            status: Some(s),
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct PromptResult {
    pub session_id: String,
    pub backend: String,
}

pub trait PromptOps {
    fn tmux_send_keys(&self, tmux_name: &str, text: &str) -> Result<(), String>;
    fn notify_socket_send(&self, session_id: &str, text: &str) -> Result<(), String>;
    fn tmux_has_session(&self, tmux_name: &str) -> bool;
    fn daemon_send(&self, session_id: &str, text: &str) -> Result<(), String>;
}

#[cfg(not(windows))]
pub fn real_prompt_ops(_socket_path: std::path::PathBuf) -> impl PromptOps {
    struct RealPromptOps;
    impl PromptOps for RealPromptOps {
        fn tmux_send_keys(&self, tmux_name: &str, text: &str) -> Result<(), String> {
            crate::tmux::send_keys(tmux_name, text)
        }
        fn notify_socket_send(&self, session_id: &str, text: &str) -> Result<(), String> {
            use std::io::Write;
            use std::os::unix::net::UnixStream;
            let sock = planeai_paths::app_data_dir().join("notify.sock");
            if !sock.exists() {
                return Err("GUI is not running (socket not found)".to_string());
            }
            let mut stream = UnixStream::connect(&sock).map_err(|e| e.to_string())?;
            let msg = serde_json::json!({
                "event": "send_prompt",
                "session_id": session_id,
                "text": text,
            });
            stream
                .write_all(format!("{}\n", msg).as_bytes())
                .map_err(|e| e.to_string())
        }
        fn tmux_has_session(&self, tmux_name: &str) -> bool {
            crate::tmux::has_session(tmux_name)
        }
        fn daemon_send(&self, session_id: &str, text: &str) -> Result<(), String> {
            daemon_send_prompt(session_id, text)
        }
    }
    RealPromptOps
}

#[cfg(windows)]
pub fn real_prompt_ops(_socket_path: std::path::PathBuf) -> impl PromptOps {
    struct WindowsPromptOps;
    impl PromptOps for WindowsPromptOps {
        fn tmux_send_keys(&self, tmux_name: &str, text: &str) -> Result<(), String> {
            use std::process::Command;
            let output = Command::new("tmux")
                .args(["send-keys", "-t", tmux_name, "-l", text])
                .output()
                .map_err(|e| format!("failed to run tmux: {e}"))?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).to_string());
            }
            let output = Command::new("tmux")
                .args(["send-keys", "-t", tmux_name, "Enter"])
                .output()
                .map_err(|e| format!("failed to run tmux: {e}"))?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).to_string());
            }
            Ok(())
        }
        fn notify_socket_send(&self, session_id: &str, text: &str) -> Result<(), String> {
            use std::io::Write;
            let pipe_name = format!("\\\\.\\pipe\\planeai-notify");
            let mut stream = std::fs::OpenOptions::new()
                .write(true)
                .open(&pipe_name)
                .map_err(|e| format!("GUI is not running (pipe not found): {e}"))?;
            let msg = serde_json::json!({
                "event": "send_prompt",
                "session_id": session_id,
                "text": text,
            });
            stream
                .write_all(format!("{}\n", msg).as_bytes())
                .map_err(|e| e.to_string())
        }
        fn tmux_has_session(&self, tmux_name: &str) -> bool {
            use std::process::Command;
            Command::new("tmux")
                .args(["has-session", "-t", tmux_name])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
        fn daemon_send(&self, session_id: &str, text: &str) -> Result<(), String> {
            daemon_send_prompt(session_id, text)
        }
    }
    WindowsPromptOps
}

/// Send a prompt to a daemon-backend session via the daemon data connection.
fn daemon_send_prompt(session_id: &str, text: &str) -> Result<(), String> {
    use std::io::Write;

    let app_dir = planeai_paths::app_data_dir();
    let mut stream = planeai_ipc::connect(planeai_ipc::Channel::Daemon, &app_dir)
        .map_err(|e| format!("daemon connect failed: {e}"))?;

    // Data connection type byte
    stream
        .write_all(&[0x01])
        .map_err(|e| format!("handshake failed: {e}"))?;

    // Session ID as 36-byte UTF-8
    stream
        .write_all(session_id.as_bytes())
        .map_err(|e| format!("session id send failed: {e}"))?;

    // Send FRAME_INPUT: [type=0x02][4-byte big-endian len][payload]
    let payload = text.as_bytes();
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(0x02); // FRAME_INPUT
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    stream
        .write_all(&frame)
        .map_err(|e| format!("frame write failed: {e}"))?;

    // Send Enter key
    let enter = b"\n";
    let mut enter_frame = Vec::with_capacity(6);
    enter_frame.push(0x02);
    enter_frame.extend_from_slice(&(enter.len() as u32).to_be_bytes());
    enter_frame.extend_from_slice(enter);
    stream
        .write_all(&enter_frame)
        .map_err(|e| format!("enter frame write failed: {e}"))?;

    Ok(())
}

pub fn send_prompt(
    conn: &Connection,
    id_prefix: &str,
    text: &str,
    ops: &dyn PromptOps,
) -> Result<PromptResult, String> {
    tracing::info!(prefix = id_prefix, "send_prompt: resolving session");
    let session = resolve_session_by_prefix(conn, id_prefix).map_err(|e| e.to_string())?;

    if session.status != "active" {
        tracing::warn!(session_id = %session.id, status = %session.status, "send_prompt: session not active");
        return Err(format!(
            "session is not active (status: {})",
            session.status
        ));
    }

    // Acquire prompt lock — RAII guard ensures release even on early `?` returns
    let guard =
        planeai_core::prompt_lock::acquire_guard(conn, &session.id).map_err(|e| e.to_string())?;

    tracing::info!(session_id = %session.id, backend = %session.backend, "send_prompt: dispatching");

    match session.backend.as_str() {
        "tmux" => {
            let tmux_name = session
                .tmux_name
                .as_deref()
                .ok_or("tmux session has no tmux_name")?;
            if !ops.tmux_has_session(tmux_name) {
                tracing::warn!(tmux_name, "send_prompt: tmux session not running");
                return Err("tmux session is not running".to_string());
            }
            ops.tmux_send_keys(tmux_name, text)?;
            tracing::info!(tmux_name, "send_prompt: sent via tmux send-keys");
        }
        "local" => {
            ops.notify_socket_send(&session.id, text)?;
            tracing::info!(session_id = %session.id, "send_prompt: sent via notify socket to local PTY");
        }
        "daemon" => {
            ops.daemon_send(&session.id, text)?;
            tracing::info!(session_id = %session.id, "send_prompt: sent via daemon data connection");
        }
        other => return Err(format!("unsupported backend: {other}")),
    }

    // Prompt was sent successfully. Release explicitly for observability, but
    // don't fail the caller — the prompt was already delivered. Guard would
    // release on drop anyway.
    if let Err(e) = guard.release() {
        tracing::warn!(session_id = %session.id, error = %e, "send_prompt: failed to release prompt lock after successful send");
    }

    Ok(PromptResult {
        session_id: session.id,
        backend: session.backend,
    })
}

/// Read buffer content from a daemon-backend session via the daemon control connection.
pub fn read_daemon_buffer(session_id: &str, lines: usize) -> Result<String, String> {
    use std::io::{BufRead, Write};

    let app_dir = planeai_paths::app_data_dir();
    let mut stream = planeai_ipc::connect(planeai_ipc::Channel::Daemon, &app_dir)
        .map_err(|e| format!("daemon is not running: {e}"))?;

    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .map_err(|e| format!("set_read_timeout failed: {e}"))?;

    // Control connection type byte
    stream
        .write_all(&[0x00])
        .map_err(|e| format!("handshake failed: {e}"))?;

    // Send read_buffer command
    let req = serde_json::json!({
        "cmd": "read_buffer",
        "session_id": session_id,
        "lines": lines,
    });
    stream
        .write_all(format!("{}\n", req).as_bytes())
        .map_err(|e| format!("write failed: {e}"))?;

    // Read response (skip events)
    let mut reader = std::io::BufReader::new(stream);
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("read failed: {e}"))?;
        if line.is_empty() {
            return Err("connection closed".to_string());
        }
        let val: serde_json::Value =
            serde_json::from_str(line.trim()).map_err(|e| format!("parse failed: {e}"))?;
        if val.get("event").is_some() {
            continue; // Skip broadcast events
        }
        if let Some(err) = val.get("error") {
            let msg = err.as_str().unwrap_or("unknown error");
            if msg.contains("unknown variant") {
                return Err(
                    "daemon does not support read_buffer (restart planeai to upgrade)".to_string(),
                );
            }
            return Err(msg.to_string());
        }
        return Ok(val["text"].as_str().unwrap_or("").to_string());
    }
}

/// Result of a cursor-based daemon buffer read.
pub struct DaemonCursorReadResult {
    pub text: String,
    pub cursor: u64,
    pub truncated: bool,
}

/// Read buffer content from a daemon-backend session since the given cursor.
pub fn read_daemon_buffer_after(
    session_id: &str,
    after: u64,
    max_bytes: usize,
) -> Result<DaemonCursorReadResult, String> {
    use std::io::{BufRead, Write};

    let app_dir = planeai_paths::app_data_dir();
    let mut stream = planeai_ipc::connect(planeai_ipc::Channel::Daemon, &app_dir)
        .map_err(|e| format!("daemon is not running: {e}"))?;

    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .map_err(|e| format!("set_read_timeout failed: {e}"))?;

    // Control connection type byte
    stream
        .write_all(&[0x00])
        .map_err(|e| format!("handshake failed: {e}"))?;

    // Send read_buffer_after command
    let req = serde_json::json!({
        "cmd": "read_buffer_after",
        "session_id": session_id,
        "after": after,
        "max_bytes": max_bytes,
    });
    stream
        .write_all(format!("{}\n", req).as_bytes())
        .map_err(|e| format!("write failed: {e}"))?;

    // Read response (skip events)
    let mut reader = std::io::BufReader::new(stream);
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("read failed: {e}"))?;
        if line.is_empty() {
            return Err("connection closed".to_string());
        }
        let val: serde_json::Value =
            serde_json::from_str(line.trim()).map_err(|e| format!("parse failed: {e}"))?;
        if val.get("event").is_some() {
            continue; // Skip broadcast events
        }
        if let Some(err) = val.get("error") {
            let msg = err.as_str().unwrap_or("unknown error");
            if msg.contains("unknown variant") {
                return Err(
                    "daemon does not support cursor reads (restart planeai to upgrade)".to_string(),
                );
            }
            return Err(msg.to_string());
        }
        let text = val["text"].as_str().unwrap_or("").to_string();
        let cursor = val["cursor"].as_u64().unwrap_or(0);
        let truncated = val["truncated"].as_bool().unwrap_or(false);
        return Ok(DaemonCursorReadResult {
            text,
            cursor,
            truncated,
        });
    }
}

/// Read output from a tmux-backend session via tmux capture-pane.
pub fn read_tmux_pane(tmux_name: &str, lines: usize) -> Result<String, String> {
    let mut cmd = std::process::Command::new("tmux");
    cmd.args([
        "capture-pane",
        "-p",
        "-t",
        tmux_name,
        "-S",
        &format!("-{lines}"),
    ]);
    planeai_core::command::no_window(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("failed to run tmux: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Result of a cursor-based tmux pane read.
pub struct TmuxCursorReadResult {
    pub text: String,
    pub cursor: String,
    pub truncated: bool,
}

/// Read tmux pane output since the given cursor.
///
/// Tmux cursor format: `tmux:<line_count>:<hash>` where hash is a truncated
/// hash of the last N lines we returned. On next read, we capture the full pane,
/// find the overlapping region by matching the hash, and return only new content.
///
/// If the cursor cannot be validated (history rolled off, pane reset), we return
/// truncated=true with a fresh cursor.
pub fn read_tmux_pane_after(
    tmux_name: &str,
    cursor: &str,
    max_bytes: usize,
) -> Result<TmuxCursorReadResult, String> {
    // Capture full scrollback (up to 10000 lines for cursor matching)
    let mut cmd = std::process::Command::new("tmux");
    cmd.args(["capture-pane", "-p", "-t", tmux_name, "-S", "-10000"]);
    planeai_core::command::no_window(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("failed to run tmux: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let full_text = String::from_utf8_lossy(&output.stdout).to_string();
    let all_lines: Vec<&str> = full_text.lines().collect();

    // Parse cursor: "tmux:<line_index>:<hash>"
    let (prev_line_count, prev_hash) = parse_tmux_cursor(cursor)?;

    // Try to validate cursor: check if lines up to prev_line_count still hash the same
    let truncated = if prev_line_count == 0 {
        false
    } else if prev_line_count <= all_lines.len() {
        let check_lines = &all_lines[..prev_line_count];
        let current_hash = hash_lines(check_lines);
        current_hash != prev_hash
    } else {
        true // History is shorter than our cursor position
    };

    let new_content = if truncated {
        // Can't trust cursor — return all content
        full_text.clone()
    } else {
        // Return only lines after the cursor position
        if prev_line_count >= all_lines.len() {
            String::new()
        } else {
            all_lines[prev_line_count..].join("\n")
        }
    };

    // Apply max_bytes cap: truncate to complete lines within the byte budget so
    // the line-based cursor can advance precisely. Any partial trailing line is
    // omitted and will be returned on the next poll.
    let (text, was_capped) = if max_bytes > 0 && new_content.len() > max_bytes {
        let safe_end = new_content
            .char_indices()
            .take_while(|(i, _)| *i < max_bytes)
            .last()
            .map_or(0, |(i, c)| i + c.len_utf8());
        let capped = &new_content[..safe_end];
        if let Some(last_nl) = capped.rfind('\n') {
            (new_content[..last_nl].to_string(), true)
        } else {
            (capped.to_string(), true)
        }
    } else {
        (new_content, false)
    };

    // Build cursor: if max_bytes capped the output, only advance to cover the
    // lines actually delivered so remaining content is returned on the next poll.
    let new_cursor = if was_capped && !truncated {
        let newline_count = text.matches('\n').count();
        if newline_count == 0 {
            build_tmux_cursor(&all_lines[..prev_line_count])
        } else {
            let delivered_line_count = newline_count + 1;
            let cursor_line_count = prev_line_count + delivered_line_count;
            let cursor_lines = &all_lines[..cursor_line_count];
            build_tmux_cursor(cursor_lines)
        }
    } else {
        build_tmux_cursor(&all_lines)
    };

    Ok(TmuxCursorReadResult {
        text,
        cursor: new_cursor,
        truncated,
    })
}

/// Build an initial tmux cursor (for first read without --after).
#[allow(dead_code)]
pub fn build_tmux_cursor_from_pane(tmux_name: &str) -> Result<String, String> {
    let mut cmd = std::process::Command::new("tmux");
    cmd.args(["capture-pane", "-p", "-t", tmux_name, "-S", "-10000"]);
    planeai_core::command::no_window(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("failed to run tmux: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let full_text = String::from_utf8_lossy(&output.stdout).to_string();
    let all_lines: Vec<&str> = full_text.lines().collect();
    Ok(build_tmux_cursor(&all_lines))
}

fn parse_tmux_cursor(cursor: &str) -> Result<(usize, u64), String> {
    let parts: Vec<&str> = cursor.splitn(3, ':').collect();
    if parts.len() != 3 || parts[0] != "tmux" {
        return Err(format!("invalid tmux cursor: {cursor}"));
    }
    let line_count: usize = parts[1]
        .parse()
        .map_err(|_| format!("invalid cursor line count: {}", parts[1]))?;
    let hash: u64 = parts[2]
        .parse()
        .map_err(|_| format!("invalid cursor hash: {}", parts[2]))?;
    Ok((line_count, hash))
}

fn build_tmux_cursor(lines: &[&str]) -> String {
    let line_count = lines.len();
    let hash = hash_lines(lines);
    format!("tmux:{line_count}:{hash}")
}

fn hash_lines(lines: &[&str]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    lines.len().hash(&mut hasher);
    // Hash first 5 lines for anchoring (detects history trimming)
    let first_end = lines.len().min(5);
    for line in &lines[..first_end] {
        line.hash(&mut hasher);
    }
    // Hash last 10 lines for tail stability
    let start = lines.len().saturating_sub(10);
    for line in &lines[start..] {
        line.hash(&mut hasher);
    }
    hasher.finish()
}

pub fn resolve_session_by_prefix(conn: &Connection, prefix: &str) -> Result<Session, ResolveError> {
    if prefix.len() < MIN_PREFIX_LEN {
        return Err(ResolveError::TooShort);
    }

    let sql = format!(
        "SELECT {} FROM sessions WHERE id LIKE ?1",
        db::SESSION_COLUMNS
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|_| ResolveError::NotFound(prefix.to_string()))?;

    let pattern = format!("{prefix}%");
    let sessions: Vec<Session> = stmt
        .query_map(rusqlite::params![pattern], db::row_to_session)
        .map_err(|_| ResolveError::NotFound(prefix.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    match sessions.len() {
        0 => Err(ResolveError::NotFound(prefix.to_string())),
        1 => Ok(sessions.into_iter().next().unwrap()),
        _ => Err(ResolveError::Ambiguous(
            sessions.into_iter().map(|s| s.id).collect(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::CleanupOps;
    use crate::config::Config;
    use crate::db;
    use std::cell::RefCell;

    struct MockPromptOps {
        sent_keys: RefCell<Vec<(String, String)>>,
        sent_socket: RefCell<Vec<(String, String)>>,
        has_session: bool,
    }

    impl MockPromptOps {
        fn new(has_session: bool) -> Self {
            Self {
                sent_keys: RefCell::new(vec![]),
                sent_socket: RefCell::new(vec![]),
                has_session,
            }
        }
    }

    impl PromptOps for MockPromptOps {
        fn tmux_send_keys(&self, tmux_name: &str, text: &str) -> Result<(), String> {
            self.sent_keys
                .borrow_mut()
                .push((tmux_name.to_string(), text.to_string()));
            Ok(())
        }
        fn notify_socket_send(&self, session_id: &str, text: &str) -> Result<(), String> {
            self.sent_socket
                .borrow_mut()
                .push((session_id.to_string(), text.to_string()));
            Ok(())
        }
        fn tmux_has_session(&self, _tmux_name: &str) -> bool {
            self.has_session
        }
        fn daemon_send(&self, _session_id: &str, _text: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        conn
    }

    fn create_session_with_known_id(conn: &Connection, id: &str, project_id: &str) {
        db::create_session_with_id(
            conn,
            id,
            project_id,
            "test-session",
            None,
            "main",
            None,
            None,
            "tmux",
            false,
            None,
            None,
            None,
        )
        .unwrap();
    }

    fn test_cleanup_ops() -> CleanupOps {
        CleanupOps {
            kill: test_kill_ops(),
            remove_worktree: Box::new(|_, _| Ok(())),
            remove_dir: Box::new(|_| Ok(())),
            delete_branch: Box::new(|_, _| Ok(())),
        }
    }

    fn test_kill_ops() -> KillOps {
        KillOps {
            kill_tmux: Box::new(|_| Ok(())),
            kill_daemon_session: Box::new(|_| Ok(())),
        }
    }

    fn failing_cleanup_ops() -> CleanupOps {
        CleanupOps {
            kill: KillOps {
                kill_tmux: Box::new(|_| Err("tmux not found".to_string())),
                kill_daemon_session: Box::new(|_| Err("daemon error".to_string())),
            },
            remove_worktree: Box::new(|_, _| Err("locked".to_string())),
            remove_dir: Box::new(|_| Err("permission denied".to_string())),
            delete_branch: Box::new(|_, _| Err("branch in use".to_string())),
        }
    }

    fn test_config_with_task_manager() -> Option<Config> {
        // Config with a task manager configured — hook fires but move_task
        // won't actually run (no CLI available in tests), which is fine
        Some(Config::default())
    }

    #[test]
    fn resolve_full_id_returns_session() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "aaaabbbb-1111-2222-3333-444455556666";
        create_session_with_known_id(&conn, id, pid);

        let session = resolve_session_by_prefix(&conn, id).unwrap();
        assert_eq!(session.id, id);
    }

    #[test]
    fn resolve_unambiguous_prefix_returns_session() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "aaaabbbb-1111-2222-3333-444455556666";
        create_session_with_known_id(&conn, id, pid);

        let session = resolve_session_by_prefix(&conn, "aaaa").unwrap();
        assert_eq!(session.id, id);
    }

    #[test]
    fn resolve_ambiguous_prefix_returns_error_with_matches() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        create_session_with_known_id(&conn, "aaaa1111-0000-0000-0000-000000000000", pid);
        create_session_with_known_id(&conn, "aaaa2222-0000-0000-0000-000000000000", pid);

        let err = resolve_session_by_prefix(&conn, "aaaa").unwrap_err();
        match err {
            ResolveError::Ambiguous(ids) => {
                assert_eq!(ids.len(), 2);
                assert!(ids.contains(&"aaaa1111-0000-0000-0000-000000000000".to_string()));
                assert!(ids.contains(&"aaaa2222-0000-0000-0000-000000000000".to_string()));
            }
            _ => panic!("expected Ambiguous, got {:?}", err),
        }
    }

    #[test]
    fn resolve_short_prefix_returns_too_short_error() {
        let conn = setup_db();
        let err = resolve_session_by_prefix(&conn, "abc").unwrap_err();
        assert_eq!(err, ResolveError::TooShort);
    }

    #[test]
    fn resolve_unknown_prefix_returns_not_found() {
        let conn = setup_db();
        let err = resolve_session_by_prefix(&conn, "zzzz").unwrap_err();
        assert_eq!(err, ResolveError::NotFound("zzzz".to_string()));
    }

    #[test]
    fn destroy_soft_deletes_and_runs_cleanup() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "aaaabbbb-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "my-session",
            Some("planeai-myapp-abc"),
            "feat-x",
            Some("/tmp/wt/abc"),
            None,
            "tmux",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        let ops = test_cleanup_ops();
        let result = destroy(&conn, id, &None, &ops).unwrap();

        // Returns pre-mutation session (status was 'active')
        assert_eq!(result.session.id, id);
        assert_eq!(result.session.status, "active");

        // DB is soft-deleted
        let s = db::get_session(&conn, id).unwrap().unwrap();
        assert_eq!(s.status, "destroyed");

        // Cleanup was called
        assert!(result.cleanup_errors.is_empty());
    }

    #[test]
    fn destroy_fires_task_hook() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "bbbbcccc-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "task-session",
            None,
            "main",
            None,
            None,
            "tmux",
            false,
            Some("PROJ-123"),
            None,
            None,
        )
        .unwrap();

        let cfg = test_config_with_task_manager();
        let ops = test_cleanup_ops();
        let result = destroy(&conn, id, &cfg, &ops).unwrap();

        assert_eq!(result.session.task_key, Some("PROJ-123".to_string()));
        // Task hook fires but we can't easily assert the side effect without
        // a real task manager CLI — the important thing is it doesn't error
    }

    #[test]
    fn destroy_returns_cleanup_errors() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "ccccdddd-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "fail-session",
            Some("planeai-x"),
            "main",
            Some("/tmp/wt/x"),
            None,
            "tmux",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        let ops = failing_cleanup_ops();
        let result = destroy(&conn, id, &None, &ops).unwrap();

        assert!(!result.cleanup_errors.is_empty());
        // Session is still soft-deleted even when cleanup fails
        let s = db::get_session(&conn, id).unwrap().unwrap();
        assert_eq!(s.status, "destroyed");
    }

    #[test]
    fn archive_kills_backend_session() {
        thread_local! {
            static KILLED: RefCell<Vec<String>> = const { RefCell::new(vec![]) };
        }
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "ddddeeee-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "to-archive",
            Some("planeai-myapp-ddd"),
            "feat-done",
            Some("/tmp/wt/done"),
            None,
            "tmux",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        let ops = KillOps {
            kill_tmux: Box::new(|name| {
                KILLED.with(|k| k.borrow_mut().push(name.to_string()));
                Ok(())
            }),
            kill_daemon_session: Box::new(|_| Ok(())),
        };

        archive(&conn, id, &None, &ops).unwrap();

        KILLED.with(|k| {
            assert_eq!(k.borrow().as_slice(), &["planeai-myapp-ddd"]);
        });
    }

    #[test]
    fn archive_local_backend_session_does_not_kill() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "ddddeeee-2222-3333-4444-555566667777";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "local-archive",
            None,
            "main",
            None,
            None,
            "local",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        let ops = KillOps {
            kill_tmux: Box::new(|_| panic!("should not be called for local backend")),
            kill_daemon_session: Box::new(|_| panic!("should not be called for local backend")),
        };

        archive(&conn, id, &None, &ops).unwrap();

        let session = db::get_session(&conn, id).unwrap().unwrap();
        assert_eq!(session.status, "archived");
    }

    #[test]
    fn archive_kills_daemon_backend_session() {
        thread_local! {
            static KILLED: RefCell<Vec<String>> = const { RefCell::new(vec![]) };
        }
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "ddddeeee-2222-3333-4444-555566667777";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "daemon-archive",
            None,
            "main",
            None,
            None,
            "daemon",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        let ops = KillOps {
            kill_tmux: Box::new(|_| Ok(())),
            kill_daemon_session: Box::new(|sid| {
                KILLED.with(|k| k.borrow_mut().push(sid.to_string()));
                Ok(())
            }),
        };

        archive(&conn, id, &None, &ops).unwrap();

        KILLED.with(|k| {
            assert_eq!(k.borrow().as_slice(), &[id]);
        });
    }

    #[test]
    fn archive_sets_status_and_returns_pre_mutation_session() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "ddddeeee-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "to-archive",
            None,
            "feat-done",
            Some("/tmp/wt/done"),
            None,
            "tmux",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        let session = archive(&conn, id, &None, &test_kill_ops()).unwrap();

        // Returns pre-mutation session
        assert_eq!(session.status, "active");
        assert_eq!(session.id, id);

        // DB is now archived
        let s = db::get_session(&conn, id).unwrap().unwrap();
        assert_eq!(s.status, "archived");
    }

    #[test]
    fn archive_fires_task_hook() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "eeeeffff-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "task-archive",
            None,
            "main",
            None,
            None,
            "tmux",
            false,
            Some("PROJ-456"),
            None,
            None,
        )
        .unwrap();

        let cfg = test_config_with_task_manager();
        let session = archive(&conn, id, &cfg, &test_kill_ops()).unwrap();

        assert_eq!(session.task_key, Some("PROJ-456".to_string()));
    }

    #[test]
    fn list_returns_active_and_exited_by_default() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        // active session
        create_session_with_known_id(&conn, "aaaa0000-0000-0000-0000-000000000000", pid);
        // exited session
        let id_exited = "bbbb0000-0000-0000-0000-000000000000";
        create_session_with_known_id(&conn, id_exited, pid);
        db::mark_session_exited(&conn, id_exited).unwrap();
        // archived session (should NOT appear)
        let id_archived = "cccc0000-0000-0000-0000-000000000000";
        create_session_with_known_id(&conn, id_archived, pid);
        db::archive_session(&conn, id_archived).unwrap();

        let sessions = list(&conn, false).unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions
            .iter()
            .all(|s| s.status == "active" || s.status == "exited"));
    }

    #[test]
    fn list_returns_archived_when_flag_set() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        // active (should NOT appear)
        create_session_with_known_id(&conn, "aaaa1111-0000-0000-0000-000000000000", pid);
        // archived session
        let id_archived = "cccc1111-0000-0000-0000-000000000000";
        create_session_with_known_id(&conn, id_archived, pid);
        db::archive_session(&conn, id_archived).unwrap();

        let sessions = list(&conn, true).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, "archived");
    }

    #[test]
    fn format_table_renders_correct_columns() {
        let projects = vec![db::Project {
            id: "proj-1".to_string(),
            name: "myapp".to_string(),
            path: "/tmp/myapp".to_string(),
            status: "active".to_string(),
            prefix: "MYA".to_string(),
        }];
        let sessions = vec![db::Session {
            id: "aaaabbbb-1111-2222-3333-444455556666".to_string(),
            project_id: "proj-1".to_string(),
            name: "fix-bug".to_string(),
            tmux_name: None,
            branch: "feat-x".to_string(),
            status: "active".to_string(),
            created_at: "2026-01-15T10:30:00Z".to_string(),
            worktree_path: None,
            provider: None,
            backend: "tmux".to_string(),
            provider_session_id: None,
            tab_count: 1,
            auto_approve: false,
            task_key: None,
            base_branch: None,
            pr_url: None,
            pr_state: None,
            attached_once: false,
            parent_session_id: None,
        }];

        let table = format_table(&sessions, &projects);
        let lines: Vec<&str> = table.lines().collect();

        // Header row
        assert!(lines[0].contains("ID"));
        assert!(lines[0].contains("NAME"));
        assert!(lines[0].contains("PROJECT"));
        assert!(lines[0].contains("BRANCH"));
        assert!(lines[0].contains("STATUS"));
        assert!(lines[0].contains("CREATED"));

        // Data row
        assert!(lines[1].contains("aaaabbbb"));
        assert!(lines[1].contains("fix-bug"));
        assert!(lines[1].contains("myapp"));
        assert!(lines[1].contains("feat-x"));
        assert!(lines[1].contains("active"));
        assert!(lines[1].contains("2026-01-15"));
    }

    #[test]
    fn send_prompt_resolves_prefix_and_returns_result() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "aaaabbbb-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "my-session",
            Some("planeai-myapp-aaaa"),
            "main",
            None,
            None,
            "tmux",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        let ops = MockPromptOps::new(true);
        let result = send_prompt(&conn, "aaaa", "fix the bug", &ops).unwrap();

        assert_eq!(result.session_id, id);
        assert_eq!(result.backend, "tmux");
        assert_eq!(ops.sent_keys.borrow().len(), 1);
        assert_eq!(
            ops.sent_keys.borrow()[0],
            ("planeai-myapp-aaaa".to_string(), "fix the bug".to_string())
        );
    }

    #[test]
    fn send_prompt_local_backend_uses_notify_socket() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "bbbbcccc-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "local-session",
            None,
            "main",
            None,
            None,
            "local",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        let ops = MockPromptOps::new(true);
        let _result = send_prompt(&conn, "bbbb", "hello agent", &ops);
        // Local backend sends via notify socket
        assert_eq!(ops.sent_keys.borrow().len(), 0);
        assert_eq!(ops.sent_socket.borrow().len(), 1);
    }

    #[test]
    fn send_prompt_daemon_backend_uses_daemon_connection() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "bbbbcccc-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "daemon-session",
            None,
            "main",
            None,
            None,
            "daemon",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        // daemon_send_prompt will attempt a real IPC connection. If a daemon is running
        // it may succeed; if not, it errors. Either way, verify the daemon path was taken
        // (not tmux or socket).
        let ops = MockPromptOps::new(true);
        let _result = send_prompt(&conn, "bbbb", "hello agent", &ops);
        // Verify neither tmux nor socket was called (daemon path was used)
        assert_eq!(ops.sent_keys.borrow().len(), 0);
        assert_eq!(ops.sent_socket.borrow().len(), 0);
    }

    #[test]
    fn send_prompt_rejects_exited_session() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "ccccdddd-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "exited",
            Some("planeai-x"),
            "main",
            None,
            None,
            "tmux",
            false,
            None,
            None,
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let ops = MockPromptOps::new(true);
        let err = send_prompt(&conn, "cccc", "hi", &ops).unwrap_err();
        assert!(err.contains("not active"));
    }

    #[test]
    fn send_prompt_rejects_tmux_session_not_running() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "ddddeeee-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "dead-tmux",
            Some("planeai-dead"),
            "main",
            None,
            None,
            "tmux",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        let ops = MockPromptOps::new(false); // has_session returns false
        let err = send_prompt(&conn, "dddd", "hi", &ops).unwrap_err();
        assert!(err.contains("not running"));
    }

    #[test]
    fn send_prompt_rejects_concurrent_prompt() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "eeeeffff-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "locked-session",
            Some("planeai-myapp-eeee"),
            "main",
            None,
            None,
            "tmux",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        // Manually acquire a lock to simulate a concurrent prompt
        let _lock = planeai_core::prompt_lock::acquire(&conn, id).unwrap();

        let ops = MockPromptOps::new(true);
        let err = send_prompt(&conn, "eeee", "second prompt", &ops).unwrap_err();
        assert!(err.contains("already in progress"));
        // tmux_send_keys should never have been called
        assert_eq!(ops.sent_keys.borrow().len(), 0);
    }

    #[test]
    fn send_prompt_releases_lock_on_backend_error() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "ffffaaaa-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "error-session",
            Some("planeai-myapp-ffff"),
            "main",
            None,
            None,
            "tmux",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        // tmux session not running — will cause an error after lock is acquired
        let ops = MockPromptOps::new(false);
        let err = send_prompt(&conn, "ffff", "hi", &ops).unwrap_err();
        assert!(err.contains("not running"));

        // Lock should have been released — next acquire should succeed
        let lock = planeai_core::prompt_lock::acquire(&conn, id).unwrap();
        assert_eq!(lock.session_id, id);
    }

    #[test]
    fn parse_tmux_cursor_valid() {
        let (line_count, hash) = super::parse_tmux_cursor("tmux:42:12345678901234").unwrap();
        assert_eq!(line_count, 42);
        assert_eq!(hash, 12345678901234);
    }

    #[test]
    fn parse_tmux_cursor_invalid_prefix() {
        let result = super::parse_tmux_cursor("daemon:42:12345");
        assert!(result.is_err());
    }

    #[test]
    fn parse_tmux_cursor_invalid_format() {
        let result = super::parse_tmux_cursor("tmux:abc:def");
        assert!(result.is_err());
    }

    #[test]
    fn build_tmux_cursor_roundtrips() {
        let lines: Vec<&str> = vec!["line 1", "line 2", "line 3"];
        let cursor = super::build_tmux_cursor(&lines);
        assert!(cursor.starts_with("tmux:3:"));
        let (count, hash) = super::parse_tmux_cursor(&cursor).unwrap();
        assert_eq!(count, 3);
        assert!(hash > 0);
    }

    #[test]
    fn hash_lines_deterministic() {
        let lines: Vec<&str> = vec!["hello", "world"];
        let h1 = super::hash_lines(&lines);
        let h2 = super::hash_lines(&lines);
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_lines_different_for_different_content() {
        let lines1: Vec<&str> = vec!["hello", "world"];
        let lines2: Vec<&str> = vec!["hello", "mars"];
        assert_ne!(super::hash_lines(&lines1), super::hash_lines(&lines2));
    }
}
