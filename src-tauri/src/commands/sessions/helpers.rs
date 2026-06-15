use std::sync::Mutex;

use crate::config;
use crate::db;

static KIRO_HOOK_CACHE: Mutex<Option<bool>> = Mutex::new(None);
static CLAUDE_HOOK_CACHE: Mutex<Option<bool>> = Mutex::new(None);
static COPILOT_HOOK_CACHE: Mutex<Option<bool>> = Mutex::new(None);

/// Invalidate cached hook-installed results (call after hook installation).
pub fn invalidate_hook_cache() {
    *KIRO_HOOK_CACHE.lock().unwrap() = None;
    *CLAUDE_HOOK_CACHE.lock().unwrap() = None;
    *COPILOT_HOOK_CACHE.lock().unwrap() = None;
}

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
        match std::fs::read_to_string(&path) {
            Ok(content) => content.contains("planeai-stop-notify"),
            Err(_) => false,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_cache_returns_consistent_value() {
        // First call populates cache, subsequent calls return same value
        let first = is_kiro_hook_installed();
        let second = is_kiro_hook_installed();
        assert_eq!(first, second);
    }

    #[test]
    fn invalidate_resets_all_caches() {
        // Populate caches
        is_kiro_hook_installed();
        is_claude_hook_installed();
        is_copilot_hook_installed();

        // Invalidate
        invalidate_hook_cache();

        // Verify caches are cleared
        assert!(KIRO_HOOK_CACHE.lock().unwrap().is_none());
        assert!(CLAUDE_HOOK_CACHE.lock().unwrap().is_none());
        assert!(COPILOT_HOOK_CACHE.lock().unwrap().is_none());
    }

    #[test]
    fn provider_has_hook_unknown_provider_returns_false() {
        let cfg = config::Config {
            appearance: config::Appearance {
                mode: "dark".into(),
                terminal_theme_dark: String::new(),
                terminal_theme_light: String::new(),
                diff_theme_dark: String::new(),
                diff_theme_light: String::new(),
                theme: "default".into(),
            },
            terminal: config::Terminal {
                font_family: "monospace".into(),
                font_size: 14,
                option_as_meta: false,
            },
            providers: std::collections::HashMap::new(),
            default_provider: "kiro".into(),
            session_backend: None,
            vim_mode: None,
            task_management: None,
            projects_base_path: None,
            pr_status: None,
            hide_done_tasks: None,
        };
        assert!(!provider_has_hook("nonexistent", &cfg));
    }
}
