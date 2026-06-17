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
/// Delegates to the unified implementation in session_ops.
pub(crate) fn fire_task_hook(
    cfg: &config::Config,
    session: &db::Session,
    hook_name: &str,
    cwd: &str,
    conn: &rusqlite::Connection,
) {
    crate::session_ops::fire_task_hook(cfg, session, hook_name, cwd, conn);
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
        let path = std::path::PathBuf::from(format!("{home}/.kiro/agents/default.json"));
        crate::notify::is_kiro_hook_installed_at(&path)
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
            daemon_scrollback_bytes: None,
            scrollback_lines: None,
            max_mounted_terminals: None,
            web_links: None,
        };
        assert!(!provider_has_hook("nonexistent", &cfg));
    }

    fn test_session(task_key: Option<&str>) -> db::Session {
        db::Session {
            id: "test-id".into(),
            project_id: "proj-1".into(),
            name: "test".into(),
            tmux_name: None,
            branch: "main".into(),
            status: "active".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            worktree_path: None,
            provider: None,
            backend: "tmux".into(),
            provider_session_id: None,
            tab_count: 1,
            auto_approve: false,
            task_key: task_key.map(|s| s.to_string()),
            base_branch: None,
            pr_url: None,
            pr_state: None,
        }
    }

    #[test]
    fn fire_task_hook_no_task_key_returns_early() {
        let cfg = config::Config::default();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let session = test_session(None);
        // Should not panic — just returns early
        fire_task_hook(&cfg, &session, "on_complete", "/tmp/myapp", &conn);
    }

    #[test]
    fn fire_task_hook_no_task_management_returns_early() {
        let cfg = config::Config {
            task_management: None,
            ..config::Config::default()
        };
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let session = test_session(Some("PROJ-1"));
        // Should not panic — returns early when no task_management configured
        fire_task_hook(&cfg, &session, "on_complete", "/tmp/myapp", &conn);
    }

    #[test]
    fn fire_task_hook_with_matching_project_derives_prefix() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let cfg = config::Config {
            task_management: Some(config::TaskManager {
                templates: None,
                on_start: None,
                on_notify: None,
                on_restart: None,
                on_complete: Some(config::LifecycleHook {
                    move_to: "done".into(),
                }),
                on_pr_open: None,
                on_pr_merge: None,
                auto_dispatch: None,
            }),
            ..config::Config::default()
        };

        let session = test_session(Some("MYA-1"));
        // Runs through the full path — project matched, prefix derived.
        // The task update won't find the task (no task tables), but doesn't error.
        fire_task_hook(&cfg, &session, "on_complete", "/tmp/myapp/src", &conn);
    }

    #[test]
    fn fire_task_hook_unknown_hook_name_does_nothing() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let cfg = config::Config {
            task_management: Some(config::TaskManager {
                templates: None,
                on_start: None,
                on_notify: None,
                on_restart: None,
                on_complete: Some(config::LifecycleHook {
                    move_to: "done".into(),
                }),
                on_pr_open: None,
                on_pr_merge: None,
                auto_dispatch: None,
            }),
            ..config::Config::default()
        };

        let session = test_session(Some("MYA-1"));
        // Unknown hook name — does nothing
        fire_task_hook(&cfg, &session, "on_unknown", "/tmp/myapp", &conn);
    }
}
