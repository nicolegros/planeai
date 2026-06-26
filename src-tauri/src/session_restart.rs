use rusqlite::Connection;

use crate::config::Config;
use crate::db::{self, Session};
use crate::session_ops::{fire_task_hook, session_cwd};

pub trait RestartOps {
    fn create_tmux_session(
        &self,
        tmux_name: &str,
        cwd: &str,
        cmd: &str,
        session_id: &str,
        extra_path_dirs: &[String],
    ) -> Result<(), String>;

    fn spawn_daemon_session(
        &self,
        session_id: &str,
        cmd: &str,
        cwd: &str,
        extra_path_dirs: &[String],
    ) -> Result<(), String>;
}

#[tracing::instrument(skip(conn, config, restart_ops), fields(session_id = id))]
pub fn restart(
    conn: &Connection,
    id: &str,
    config: &Config,
    restart_ops: &dyn RestartOps,
) -> Result<Session, String> {
    let session = db::get_session(conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("session not found: {id}"))?;

    if !matches!(session.status.as_str(), "exited" | "archived") {
        return Err("can only restart exited or archived sessions".to_string());
    }

    let provider_key = session
        .provider
        .as_deref()
        .unwrap_or(&config.default_provider);
    let provider_def = config
        .providers
        .get(provider_key)
        .ok_or_else(|| format!("Unknown provider: {provider_key}"))?;
    let has_resume_command = provider_def.resume_command.is_some();
    let resume_cmd = if has_resume_command {
        Some(crate::config::restart_command_for_provider(
            provider_def,
            None,
        ))
    } else {
        None
    };
    let fresh_cmd = crate::config::launch_command(provider_def, session.auto_approve);

    tracing::debug!(backend = %session.backend, ?resume_cmd, %fresh_cmd, "commands resolved");

    let extra_path_dirs = config.resolved_extra_path_dirs();

    let project_path = db::get_project(conn, &session.project_id)
        .ok()
        .flatten()
        .map(|p| p.path);
    let cwd = session
        .worktree_path
        .as_deref()
        .or(project_path.as_deref())
        .unwrap_or("/");

    // Try resume first, fallback to fresh launch
    let cmd_to_use = if let Some(ref resume) = resume_cmd {
        resume.clone()
    } else {
        fresh_cmd.clone()
    };

    let tmux_name = session.tmux_name.as_deref();
    let try_spawn = |cmd: &str| -> Result<(), String> {
        match session.backend.as_str() {
            "tmux" => {
                let tn = tmux_name.ok_or("tmux session has no tmux_name")?;
                restart_ops.create_tmux_session(tn, cwd, cmd, id, &extra_path_dirs)
            }
            "daemon" => restart_ops.spawn_daemon_session(id, cmd, cwd, &extra_path_dirs),
            "local" => Ok(()), // PTY spawned on attach, nothing to pre-create
            other => Err(format!("unsupported backend: {other}")),
        }
    };

    let spawn_result = try_spawn(&cmd_to_use);

    // Fallback: if resume failed, retry with fresh command
    let spawn_result = if spawn_result.is_err() && resume_cmd.is_some() {
        tracing::warn!(err = ?spawn_result, "resume failed, falling back to fresh launch");
        try_spawn(&fresh_cmd)
    } else {
        spawn_result
    };

    spawn_result?;

    db::restore_session(conn, id).map_err(|e| e.to_string())?;
    let updated = db::get_session(conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("session not found after restore: {id}"))?;

    // Fire task hook after restore
    if let Some(ref _key) = updated.task_key {
        if let Some(cwd) = session_cwd(conn, &updated) {
            fire_task_hook(config, &updated, "on_restart", &cwd, conn);
        }
    }

    Ok(updated)
}

/// Production RestartOps using real tmux calls and daemon spawn.
pub fn real_restart_ops() -> impl RestartOps {
    struct RealRestartOps;
    impl RestartOps for RealRestartOps {
        fn create_tmux_session(
            &self,
            tmux_name: &str,
            cwd: &str,
            cmd: &str,
            session_id: &str,
            extra_path_dirs: &[String],
        ) -> Result<(), String> {
            #[cfg(not(windows))]
            {
                // If the tmux session still exists, just reattach — don't recreate
                if crate::tmux::has_session(tmux_name) {
                    tracing::info!(tmux_name, "restart: tmux session still alive, reattaching");
                    return Ok(());
                }
                crate::tmux::create_session_with_cmd_and_path(
                    tmux_name,
                    cwd,
                    cmd,
                    session_id,
                    extra_path_dirs,
                )
            }
            #[cfg(windows)]
            {
                let _ = (tmux_name, cwd, cmd, session_id, extra_path_dirs);
                Err("tmux backend not available on Windows".to_string())
            }
        }

        fn spawn_daemon_session(
            &self,
            session_id: &str,
            cmd: &str,
            cwd: &str,
            extra_path_dirs: &[String],
        ) -> Result<(), String> {
            tracing::info!(
                session_id = &session_id[..8.min(session_id.len())],
                cmd,
                cwd,
                "restart_ops: spawning daemon session"
            );

            // Ensure daemon is running before trying to spawn
            let socket_path = planeai_ipc::daemon_socket_path();
            let daemon_bin = crate::paths::resolve_daemon_binary_fallback();
            let scrollback = 1_048_576;
            crate::daemon::ensure_running(&daemon_bin, &socket_path, scrollback)?;

            let mut path_buf = String::new();
            let env =
                planeai_core::command::build_daemon_env(extra_path_dirs, session_id, &mut path_buf);
            let (program, args) = planeai_core::command::shell_args(cmd);
            let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            crate::daemon::spawn_session(session_id, program, &args_refs, cwd, Some(&env))
        }
    }
    RealRestartOps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db;
    use std::cell::RefCell;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        conn
    }

    struct MockRestartOps {
        calls: RefCell<Vec<(String, String, String, String)>>,
        daemon_calls: RefCell<Vec<(String, String, String)>>,
        fail_resume: bool,
    }

    impl MockRestartOps {
        fn new() -> Self {
            Self {
                calls: RefCell::new(vec![]),
                daemon_calls: RefCell::new(vec![]),
                fail_resume: false,
            }
        }

        fn failing_resume() -> Self {
            Self {
                calls: RefCell::new(vec![]),
                daemon_calls: RefCell::new(vec![]),
                fail_resume: true,
            }
        }
    }

    impl RestartOps for MockRestartOps {
        fn create_tmux_session(
            &self,
            tmux_name: &str,
            cwd: &str,
            cmd: &str,
            session_id: &str,
            _extra_path_dirs: &[String],
        ) -> Result<(), String> {
            if self.fail_resume && cmd.contains("--resume") {
                return Err("resume failed".to_string());
            }
            self.calls.borrow_mut().push((
                tmux_name.to_string(),
                cwd.to_string(),
                cmd.to_string(),
                session_id.to_string(),
            ));
            Ok(())
        }

        fn spawn_daemon_session(
            &self,
            session_id: &str,
            cmd: &str,
            cwd: &str,
            _extra_path_dirs: &[String],
        ) -> Result<(), String> {
            if self.fail_resume && cmd.contains("--resume") {
                return Err("resume failed".to_string());
            }
            self.daemon_calls.borrow_mut().push((
                session_id.to_string(),
                cmd.to_string(),
                cwd.to_string(),
            ));
            Ok(())
        }
    }

    #[test]
    fn restart_restores_exited_session() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "eeeeffff-2222-3333-4444-555566667777";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "restart-me",
            Some("planeai-myapp-eee"),
            "main",
            None,
            None,
            "tmux",
            false,
            None,
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::new();
        let updated = restart(&conn, id, &cfg, &ops).unwrap();

        assert_eq!(updated.status, "active");
        assert_eq!(ops.calls.borrow().len(), 1);
        assert_eq!(ops.calls.borrow()[0].0, "planeai-myapp-eee");
    }

    #[test]
    fn restart_rejects_active_session() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "ffffaaaa-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "test-session",
            None,
            "main",
            None,
            None,
            "tmux",
            false,
            None,
            None,
        )
        .unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::new();
        let err = restart(&conn, id, &cfg, &ops).unwrap_err();
        assert!(err.contains("can only restart exited or archived sessions"));
    }

    #[test]
    fn restart_restores_archived_session() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "ffffbbbb-2222-3333-4444-555566667777";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "archived-me",
            Some("planeai-myapp-fff"),
            "main",
            None,
            None,
            "tmux",
            false,
            None,
            None,
        )
        .unwrap();
        db::archive_session(&conn, id).unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::new();
        let updated = restart(&conn, id, &cfg, &ops).unwrap();

        assert_eq!(updated.status, "active");
        assert_eq!(ops.calls.borrow().len(), 1);
        assert_eq!(ops.calls.borrow()[0].0, "planeai-myapp-fff");
    }

    #[test]
    fn restart_daemon_session_spawns_in_daemon() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "aaaa1111-3333-4444-5555-666677778888";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "daemon-restart",
            None,
            "main",
            None,
            None,
            "daemon",
            false,
            None,
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::new();
        let updated = restart(&conn, id, &cfg, &ops).unwrap();

        assert_eq!(updated.status, "active");
        // Should use daemon_calls, not tmux calls
        assert_eq!(ops.calls.borrow().len(), 0);
        assert_eq!(ops.daemon_calls.borrow().len(), 1);
        assert_eq!(ops.daemon_calls.borrow()[0].0, id);
    }

    #[test]
    fn restart_local_session_restores_without_spawning() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "cccc3333-5555-6666-7777-888899990000";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "local-restart",
            None,
            "main",
            None,
            None,
            "local",
            false,
            None,
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::new();
        let updated = restart(&conn, id, &cfg, &ops).unwrap();

        assert_eq!(updated.status, "active");
        assert_eq!(ops.calls.borrow().len(), 0);
        assert_eq!(ops.daemon_calls.borrow().len(), 0);
    }

    #[test]
    fn restart_falls_back_to_fresh_when_resume_fails() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "bbbb2222-4444-5555-6666-777788889999";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "resume-fallback",
            Some("planeai-myapp-bbb"),
            "main",
            None,
            Some("kiro"),
            "tmux",
            false,
            None,
            None,
        )
        .unwrap();
        db::set_provider_session_id(&conn, id, "sess-abc").unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::failing_resume();
        let updated = restart(&conn, id, &cfg, &ops).unwrap();

        assert_eq!(updated.status, "active");
        // First call was resume (failed), second was fresh (succeeded)
        assert_eq!(ops.calls.borrow().len(), 1);
        let call = &ops.calls.borrow()[0];
        // Fresh command should NOT contain --resume
        assert!(
            !call.2.contains("--resume"),
            "expected fresh launch, got: {}",
            call.2
        );
    }

    #[test]
    fn restart_daemon_session_uses_resume_command() {
        // Regression test for PLA-169: daemon sessions must use the interactive
        // resume command (e.g. "kiro-cli chat --resume") instead of a fresh launch.
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "cccc3333-5555-6666-7777-888899990000";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "daemon-resume",
            None,
            "main",
            None,
            Some("kiro"),
            "daemon",
            false,
            None,
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::new();
        let updated = restart(&conn, id, &cfg, &ops).unwrap();

        assert_eq!(updated.status, "active");
        assert_eq!(ops.daemon_calls.borrow().len(), 1);
        let call = &ops.daemon_calls.borrow()[0];
        assert!(
            call.1.contains("--resume"),
            "expected resume command, got: {}",
            call.1
        );
        assert_eq!(call.1, "kiro-cli chat --resume");
    }
}
