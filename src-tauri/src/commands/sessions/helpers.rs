use crate::config;
use crate::db;
use crate::task_manager;

/// Resolve the working directory for a session's project.
pub(crate) fn session_cwd(conn: &rusqlite::Connection, session: &db::Session) -> Option<String> {
    if let Some(ref wt) = session.worktree_path {
        return Some(wt.clone());
    }
    db::list_projects(conn)
        .ok()
        .and_then(|ps| ps.into_iter().find(|p| p.id == session.project_id))
        .map(|p| p.path)
}

/// Fire a task manager lifecycle hook (on_start, on_notify, on_restart, on_complete).
pub(crate) fn fire_task_hook(
    cfg: &config::Config,
    session: &db::Session,
    hook_name: &str,
    cwd: &str,
) {
    let task_key = match &session.task_key {
        Some(k) => k,
        None => return,
    };
    let tm = match resolve_task_manager(cfg) {
        Ok(tm) => tm,
        Err(_) => return,
    };
    let hook = match hook_name {
        "on_start" => tm.on_start.as_ref(),
        "on_notify" => tm.on_notify.as_ref(),
        "on_restart" => tm.on_restart.as_ref(),
        "on_complete" => tm.on_complete.as_ref(),
        _ => None,
    };
    if let Some(h) = hook {
        let _ = task_manager::move_task(tm, task_key, &h.move_to, std::path::Path::new(cwd));
    }
}

pub(crate) fn resolve_task_manager(cfg: &config::Config) -> Result<&config::TaskManager, String> {
    let key = cfg
        .default_task_manager
        .as_deref()
        .or_else(|| cfg.task_managers.keys().next().map(|s| s.as_str()))
        .ok_or("No task manager configured")?;
    cfg.task_managers
        .get(key)
        .ok_or_else(|| format!("Task manager '{}' not found in config", key))
}

/// Check if a provider has hook-based idle detection.
pub(crate) fn provider_has_hook(provider_key: &str, cfg: &config::Config) -> bool {
    let Some(provider) = cfg.providers.get(provider_key) else {
        return false;
    };
    if provider.command.contains("kiro") {
        is_kiro_hook_installed()
    } else if provider.command.contains("claude") {
        is_claude_hook_installed()
    } else if provider.command.contains("copilot") {
        is_copilot_hook_installed()
    } else {
        false
    }
}

pub(crate) fn is_kiro_hook_installed() -> bool {
    let home = config::home_dir();
    let path = format!("{home}/.kiro/agents/default.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => content.contains("planeai-stop-notify"),
        Err(_) => false,
    }
}

pub(crate) fn is_claude_hook_installed() -> bool {
    let home = config::home_dir();
    let path = std::path::PathBuf::from(format!("{home}/.claude/settings.json"));
    crate::notify::is_claude_hook_installed_at(&path)
}

pub(crate) fn is_copilot_hook_installed() -> bool {
    let copilot_dir = std::env::var("COPILOT_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(format!("{}/.copilot", config::home_dir())));
    crate::notify::is_copilot_hook_installed_at(&copilot_dir)
}
