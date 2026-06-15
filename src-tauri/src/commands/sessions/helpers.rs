use std::sync::Mutex;

use crate::config;
use crate::db;

static KIRO_HOOK_CACHE: Mutex<Option<bool>> = Mutex::new(None);
static CLAUDE_HOOK_CACHE: Mutex<Option<bool>> = Mutex::new(None);
static COPILOT_HOOK_CACHE: Mutex<Option<bool>> = Mutex::new(None);

/// Resolve the working directory for a session's project.
pub(crate) fn session_cwd(conn: &rusqlite::Connection, session: &db::Session) -> Option<String> {
    if let Some(ref wt) = session.worktree_path {
        return Some(wt.clone());
    }
    db::get_project(conn, &session.project_id)
        .ok()
        .flatten()
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
        let db_path = crate::paths::db_path();
        // Derive prefix from project name by looking up the path
        let conn_result = rusqlite::Connection::open(&db_path);
        if let Ok(conn) = conn_result {
            let projects = db::list_projects(&conn).unwrap_or_default();
            let prefix = projects
                .iter()
                .find(|p| cwd.starts_with(&p.path))
                .map(|p| planeai_tasks::sqlite::derive_prefix(&p.name))
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
}

pub(crate) fn resolve_task_manager(cfg: &config::Config) -> Result<&config::TaskManager, String> {
    cfg.task_management
        .as_ref()
        .ok_or("No task management configured".to_string())
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
    let mut cache = KIRO_HOOK_CACHE.lock().unwrap();
    *cache.get_or_insert_with(|| {
        let home = config::home_dir();
        let path = format!("{home}/.kiro/agents/default.json");
        std::fs::read_to_string(&path)
            .map(|c| c.contains("planeai-stop-notify"))
            .unwrap_or(false)
    })
}

pub(crate) fn is_claude_hook_installed() -> bool {
    let mut cache = CLAUDE_HOOK_CACHE.lock().unwrap();
    *cache.get_or_insert_with(|| {
        let home = config::home_dir();
        let path = std::path::PathBuf::from(format!("{home}/.claude/settings.json"));
        crate::notify::is_claude_hook_installed_at(&path)
    })
}

pub(crate) fn is_copilot_hook_installed() -> bool {
    let mut cache = COPILOT_HOOK_CACHE.lock().unwrap();
    *cache.get_or_insert_with(|| {
        let copilot_dir = std::env::var("COPILOT_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::PathBuf::from(format!("{}/.copilot", config::home_dir()))
            });
        crate::notify::is_copilot_hook_installed_at(&copilot_dir)
    })
}

/// Invalidate the hook cache so next call re-reads from disk.
pub(crate) fn invalidate_hook_cache() {
    *KIRO_HOOK_CACHE.lock().unwrap() = None;
    *CLAUDE_HOOK_CACHE.lock().unwrap() = None;
    *COPILOT_HOOK_CACHE.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_returns_stored_value_without_re_reading() {
        // Pre-seed cache with a known value
        *KIRO_HOOK_CACHE.lock().unwrap() = Some(true);

        // Even though no file exists, cached value is returned
        assert!(is_kiro_hook_installed());

        // Cleanup
        *KIRO_HOOK_CACHE.lock().unwrap() = None;
    }

    #[test]
    fn invalidate_hook_cache_clears_all_caches() {
        // Seed all caches
        *KIRO_HOOK_CACHE.lock().unwrap() = Some(true);
        *CLAUDE_HOOK_CACHE.lock().unwrap() = Some(true);
        *COPILOT_HOOK_CACHE.lock().unwrap() = Some(true);

        invalidate_hook_cache();

        assert!(KIRO_HOOK_CACHE.lock().unwrap().is_none());
        assert!(CLAUDE_HOOK_CACHE.lock().unwrap().is_none());
        assert!(COPILOT_HOOK_CACHE.lock().unwrap().is_none());
    }

    #[test]
    fn cache_populated_on_first_call_then_reused() {
        // Ensure cache is empty
        *KIRO_HOOK_CACHE.lock().unwrap() = None;

        // First call populates the cache (will be false since no file at test home)
        let first = is_kiro_hook_installed();

        // Overwrite cache to prove second call uses cache, not disk
        *KIRO_HOOK_CACHE.lock().unwrap() = Some(!first);
        let second = is_kiro_hook_installed();

        assert_eq!(second, !first);

        // Cleanup
        *KIRO_HOOK_CACHE.lock().unwrap() = None;
    }

    #[test]
    fn invalidate_causes_re_read_on_next_call() {
        // Seed cache with true
        *KIRO_HOOK_CACHE.lock().unwrap() = Some(true);
        assert!(is_kiro_hook_installed());

        // Invalidate
        invalidate_hook_cache();
        assert!(KIRO_HOOK_CACHE.lock().unwrap().is_none());

        // Next call re-reads from disk (re-populates cache)
        let _ = is_kiro_hook_installed();
        assert!(KIRO_HOOK_CACHE.lock().unwrap().is_some());

        // Cleanup
        *KIRO_HOOK_CACHE.lock().unwrap() = None;
    }
}
