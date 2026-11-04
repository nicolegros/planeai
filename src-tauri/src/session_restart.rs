use rusqlite::Connection;

use crate::config::Config;
use crate::db::{self, Session};
use crate::session_ops::fire_task_hook;
use planeai_core::mcp::McpEndpoint;

pub trait RestartOps {
    fn tmux_session_alive(&self, tmux_name: &str) -> bool;

    fn create_tmux_session(
        &self,
        tmux_name: &str,
        cwd: &str,
        cmd: &str,
        session_id: &str,
        extra_path_dirs: &[String],
        mcp: Option<&McpEndpoint>,
    ) -> Result<(), String>;

    fn spawn_daemon_session(
        &self,
        session_id: &str,
        cmd: &str,
        cwd: &str,
        extra_path_dirs: &[String],
        mcp: Option<&McpEndpoint>,
    ) -> Result<(), String>;

    /// Spawn the agent resource in its task workspace.
    ///
    /// `workspace` is passed in because only the caller knows the session's task
    /// linkage, and two agents on one task must share a workspace.
    fn spawn_rmux_session(
        &self,
        session_id: &str,
        workspace: &planeai_rmux::WorkspaceName,
        cmd: &str,
        cwd: &str,
        extra_path_dirs: &[String],
        mcp: Option<&McpEndpoint>,
    ) -> Result<(), String>;
}

/// `mcp` is the MCP server while it is listening.
#[tracing::instrument(skip(conn, config, restart_ops), fields(session_id = id))]
pub fn restart(
    conn: &Connection,
    id: &str,
    config: &Config,
    mcp: Option<&planeai_core::mcp::McpLaunch>,
    restart_ops: &dyn RestartOps,
) -> Result<Session, String> {
    let session = db::get_session(conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("session not found: {id}"))?;

    if !matches!(session.status.as_str(), "exited" | "archived") {
        return Err("can only restart exited or archived sessions".to_string());
    }
    let project = db::get_project(conn, &session.project_id).map_err(|e| e.to_string())?;
    if let Some(project) = &project {
        project.require_folder()?;
    }

    // Provider sessions resume through their plugin when next used (ADR-0014).
    if session.backend == crate::session_ops::PLUGIN_BACKEND {
        return restore(conn, id, config);
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
            mcp,
        ))
    } else {
        None
    };
    let fresh_cmd = provider_def.first_launch_command(session.auto_approve, None, mcp);

    tracing::info!(
        backend = %session.backend,
        provider = %provider_key,
        ?resume_cmd,
        %fresh_cmd,
        resume_command_configured = ?provider_def.resume_command,
        "restart: commands resolved"
    );

    let extra_path_dirs = config.resolved_extra_path_dirs();

    let project_path = project.map(|p| p.path);
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
    if session.backend == "tmux" {
        let tn = tmux_name.ok_or("tmux session has no tmux_name")?;
        // The agent still runs and holds the session's token; minting would revoke it.
        if restart_ops.tmux_session_alive(tn) {
            tracing::info!(
                tmux_name = tn,
                "restart: tmux session still alive, reattaching"
            );
            return restore(conn, id, config);
        }
    }
    let rmux_workspace = session.rmux_workspace();
    let mcp_url = mcp.map(|mcp| mcp.url.as_str());
    let mcp = planeai_core::mcp::launch_endpoint(conn, mcp_url, id, &session.backend)?;
    let mcp = mcp.as_ref();
    let try_spawn = |cmd: &str| -> Result<(), String> {
        match session.backend.as_str() {
            "tmux" => {
                let tn = tmux_name.ok_or("tmux session has no tmux_name")?;
                restart_ops.create_tmux_session(tn, cwd, cmd, id, &extra_path_dirs, mcp)
            }
            "daemon" => restart_ops.spawn_daemon_session(id, cmd, cwd, &extra_path_dirs, mcp),
            planeai_rmux::BACKEND => {
                restart_ops.spawn_rmux_session(id, &rmux_workspace, cmd, cwd, &extra_path_dirs, mcp)
            }
            "local" => Ok(()), // PTY spawned on attach, nothing to pre-create
            other => Err(format!("unsupported backend: {other}")),
        }
    };

    let spawn_result = try_spawn(&cmd_to_use);

    // Fallback: if resume failed, retry with fresh command
    let spawn_result = if spawn_result.is_err() && resume_cmd.is_some() {
        tracing::warn!(err = ?spawn_result, %fresh_cmd, "resume failed, falling back to fresh launch");
        try_spawn(&fresh_cmd)
    } else {
        spawn_result
    };

    spawn_result?;
    restore(conn, id, config)
}

fn restore(conn: &Connection, id: &str, config: &Config) -> Result<Session, String> {
    db::restore_session(conn, id).map_err(|e| e.to_string())?;
    let updated = db::get_session(conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("session not found after restore: {id}"))?;

    // Fire task hook after restore
    if updated.task_key.is_some() {
        fire_task_hook(config, &updated, "on_restart", conn);
    }

    Ok(updated)
}

/// Production RestartOps using real tmux calls and daemon spawn.
pub fn real_restart_ops() -> impl RestartOps {
    struct RealRestartOps;
    impl RestartOps for RealRestartOps {
        fn tmux_session_alive(&self, tmux_name: &str) -> bool {
            #[cfg(not(windows))]
            return crate::tmux::has_session(tmux_name);
            #[cfg(windows)]
            {
                let _ = tmux_name;
                false
            }
        }

        fn create_tmux_session(
            &self,
            tmux_name: &str,
            cwd: &str,
            cmd: &str,
            session_id: &str,
            extra_path_dirs: &[String],
            mcp: Option<&McpEndpoint>,
        ) -> Result<(), String> {
            #[cfg(not(windows))]
            {
                crate::tmux::create_session_with_cmd_and_path(
                    tmux_name,
                    cwd,
                    cmd,
                    session_id,
                    extra_path_dirs,
                    mcp,
                )
            }
            #[cfg(windows)]
            {
                let _ = (tmux_name, cwd, cmd, session_id, extra_path_dirs, mcp);
                Err("tmux backend not available on Windows".to_string())
            }
        }

        fn spawn_rmux_session(
            &self,
            session_id: &str,
            workspace: &planeai_rmux::WorkspaceName,
            cmd: &str,
            cwd: &str,
            extra_path_dirs: &[String],
            mcp: Option<&McpEndpoint>,
        ) -> Result<(), String> {
            tracing::info!(
                session_id = &session_id[..8.min(session_id.len())],
                cmd,
                cwd,
                workspace = %workspace,
                "restart_ops: spawning rmux resource"
            );

            let mut path_buf = String::new();
            let env = planeai_core::command::build_daemon_env(
                extra_path_dirs,
                session_id,
                &mut path_buf,
                mcp,
            );
            crate::rmux_ops::spawn_resource_blocking(
                session_id, session_id, workspace, cmd, cwd, &env,
            )
            .map(|_| ())
        }

        fn spawn_daemon_session(
            &self,
            session_id: &str,
            cmd: &str,
            cwd: &str,
            extra_path_dirs: &[String],
            mcp: Option<&McpEndpoint>,
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
            let env = planeai_core::command::build_daemon_env(
                extra_path_dirs,
                session_id,
                &mut path_buf,
                mcp,
            );
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

    /// A project folder that exists, so launches are not refused as a missing folder.
    fn existing_project_dir() -> String {
        tempfile::tempdir()
            .unwrap()
            .keep()
            .to_string_lossy()
            .to_string()
    }

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        conn
    }

    struct MockRestartOps {
        calls: RefCell<Vec<(String, String, String, String)>>,
        daemon_calls: RefCell<Vec<(String, String, String)>>,
        rmux_calls: RefCell<Vec<(String, String, String)>>,
        rmux_workspaces: RefCell<Vec<String>>,
        daemon_mcp: RefCell<Vec<Option<McpEndpoint>>>,
        fail_resume: bool,
        tmux_alive: bool,
    }

    impl MockRestartOps {
        fn new() -> Self {
            Self {
                calls: RefCell::new(vec![]),
                daemon_calls: RefCell::new(vec![]),
                rmux_calls: RefCell::new(vec![]),
                rmux_workspaces: RefCell::new(vec![]),
                daemon_mcp: RefCell::new(vec![]),
                fail_resume: false,
                tmux_alive: false,
            }
        }

        fn failing_resume() -> Self {
            Self {
                calls: RefCell::new(vec![]),
                daemon_calls: RefCell::new(vec![]),
                rmux_calls: RefCell::new(vec![]),
                rmux_workspaces: RefCell::new(vec![]),
                daemon_mcp: RefCell::new(vec![]),
                fail_resume: true,
                tmux_alive: false,
            }
        }
    }

    impl RestartOps for MockRestartOps {
        fn tmux_session_alive(&self, _tmux_name: &str) -> bool {
            self.tmux_alive
        }
        fn spawn_rmux_session(
            &self,
            session_id: &str,
            _workspace: &planeai_rmux::WorkspaceName,
            cmd: &str,
            cwd: &str,
            _extra_path_dirs: &[String],
            _mcp: Option<&McpEndpoint>,
        ) -> Result<(), String> {
            self.rmux_workspaces
                .borrow_mut()
                .push(_workspace.as_str().to_string());
            self.rmux_calls.borrow_mut().push((
                session_id.to_string(),
                cmd.to_string(),
                cwd.to_string(),
            ));
            Ok(())
        }
        fn create_tmux_session(
            &self,
            tmux_name: &str,
            cwd: &str,
            cmd: &str,
            session_id: &str,
            _extra_path_dirs: &[String],
            _mcp: Option<&McpEndpoint>,
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
            mcp: Option<&McpEndpoint>,
        ) -> Result<(), String> {
            self.daemon_mcp.borrow_mut().push(mcp.cloned());
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
        db::create_project(&conn, "myapp", &existing_project_dir()).unwrap();
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
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::new();
        let updated = restart(&conn, id, &cfg, None, &ops).unwrap();

        assert_eq!(updated.status, "active");
        assert_eq!(ops.calls.borrow().len(), 1);
        assert_eq!(ops.calls.borrow()[0].0, "planeai-myapp-eee");
    }

    #[test]
    fn restart_is_refused_while_the_project_folder_is_missing() {
        let conn = setup_db();
        let project = db::create_project(&conn, "myapp", "/nonexistent/planeai/myapp").unwrap();
        let id = "abababab-1111-2222-3333-444455556666";
        db::create_session_with_id(
            &conn,
            id,
            &project.id,
            "moved",
            Some("planeai-myapp-aba"),
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
        let ops = MockRestartOps::new();

        let err = restart(&conn, id, &Config::default(), None, &ops).unwrap_err();

        assert_eq!(err, "Project folder not found. Locate it first.");
        assert_eq!(
            db::get_session(&conn, id).unwrap().unwrap().status,
            "exited"
        );
    }

    #[test]
    fn restart_rejects_active_session() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", &existing_project_dir()).unwrap();
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
            None,
        )
        .unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::new();
        let err = restart(&conn, id, &cfg, None, &ops).unwrap_err();
        assert!(err.contains("can only restart exited or archived sessions"));
    }

    #[test]
    fn restart_restores_archived_session() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", &existing_project_dir()).unwrap();
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
            None,
        )
        .unwrap();
        db::archive_session(&conn, id).unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::new();
        let updated = restart(&conn, id, &cfg, None, &ops).unwrap();

        assert_eq!(updated.status, "active");
        assert_eq!(ops.calls.borrow().len(), 1);
        assert_eq!(ops.calls.borrow()[0].0, "planeai-myapp-fff");
    }

    #[test]
    fn restart_daemon_session_spawns_in_daemon() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", &existing_project_dir()).unwrap();
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
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::new();
        let updated = restart(&conn, id, &cfg, None, &ops).unwrap();

        assert_eq!(updated.status, "active");
        // Should use daemon_calls, not tmux calls
        assert_eq!(ops.calls.borrow().len(), 0);
        assert_eq!(ops.daemon_calls.borrow().len(), 1);
        assert_eq!(ops.daemon_calls.borrow()[0].0, id);
    }

    #[test]
    fn restart_rmux_session_rejoins_its_tasks_workspace_across_projects() {
        let conn = setup_db();
        let owner = db::create_project(&conn, "owner", &existing_project_dir()).unwrap();
        let repo = db::create_project(&conn, "repo", &existing_project_dir()).unwrap();
        let id = "cccc2222-4444-5555-6666-777788889999";
        db::create_session_with_params(
            &conn,
            &planeai_core::services::CreateSessionParams {
                id: id.to_string(),
                project_id: repo.id.to_string(),
                name: "cross".to_string(),
                branch: "main".to_string(),
                backend: planeai_rmux::BACKEND.to_string(),
                task_key: Some("OWN-1".to_string()),
                task_project_id: Some(owner.id.to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let ops = MockRestartOps::new();
        restart(&conn, id, &Config::default(), None, &ops).unwrap();

        let expected = planeai_rmux::WorkspaceKey::for_session(&owner.id, Some("OWN-1"), id).name();
        assert_eq!(
            *ops.rmux_workspaces.borrow(),
            vec![expected.as_str().to_string()]
        );
    }

    #[test]
    fn restart_rmux_session_spawns_in_rmux_only() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", &existing_project_dir()).unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "bbbb2222-4444-5555-6666-777788889999";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "rmux-restart",
            None,
            "main",
            None,
            None,
            planeai_rmux::BACKEND,
            false,
            None,
            None,
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::new();
        let updated = restart(&conn, id, &cfg, None, &ops).unwrap();

        assert_eq!(updated.status, "active");
        // An rmux restart must not reach the tmux or daemon paths.
        assert_eq!(ops.calls.borrow().len(), 0);
        assert_eq!(ops.daemon_calls.borrow().len(), 0);
        assert_eq!(ops.rmux_calls.borrow().len(), 1);
        assert_eq!(ops.rmux_calls.borrow()[0].0, id);
    }

    #[test]
    fn restart_local_session_restores_without_spawning() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", &existing_project_dir()).unwrap();
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
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::new();
        let updated = restart(&conn, id, &cfg, None, &ops).unwrap();

        assert_eq!(updated.status, "active");
        assert_eq!(ops.calls.borrow().len(), 0);
        assert_eq!(ops.daemon_calls.borrow().len(), 0);
    }

    #[test]
    fn restart_provider_session_restores_without_a_configured_provider() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", &existing_project_dir()).unwrap();
        let projects = db::list_projects(&conn).unwrap();
        let pid = &projects[0].id;

        let id = "dddd4444-5555-6666-7777-888899990000";
        db::create_session_with_id(
            &conn,
            id,
            pid,
            "provider-restart",
            None,
            "main",
            None,
            Some("claude-chat:claude"),
            "plugin",
            false,
            None,
            None,
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let ops = MockRestartOps::new();
        let updated = restart(&conn, id, &Config::default(), None, &ops).unwrap();

        assert_eq!(updated.status, "active");
        assert_eq!(ops.calls.borrow().len(), 0);
        assert_eq!(ops.daemon_calls.borrow().len(), 0);
    }

    #[test]
    fn restart_falls_back_to_fresh_when_resume_fails() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", &existing_project_dir()).unwrap();
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
            None,
        )
        .unwrap();
        db::set_provider_session_id(&conn, id, "sess-abc").unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::failing_resume();
        let updated = restart(&conn, id, &cfg, None, &ops).unwrap();

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
        db::create_project(&conn, "myapp", &existing_project_dir()).unwrap();
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
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let cfg = Config::default();
        let ops = MockRestartOps::new();
        let updated = restart(&conn, id, &cfg, None, &ops).unwrap();

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

    #[test]
    fn a_restarted_daemon_agent_gets_a_fresh_mcp_token_for_its_session() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", &existing_project_dir()).unwrap();
        let pid = db::list_projects(&conn).unwrap()[0].id.clone();
        let id = "dddd3333-5555-6666-7777-888899990000";
        db::create_session_with_id(
            &conn,
            id,
            &pid,
            "agent",
            None,
            "main",
            None,
            Some("claude"),
            "daemon",
            false,
            None,
            None,
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();

        let ops = MockRestartOps::new();
        restart(
            &conn,
            id,
            &Config::default(),
            Some(&planeai_core::mcp::McpLaunch::new(
                "http://127.0.0.1:47437/mcp",
            )),
            &ops,
        )
        .unwrap();

        let command = ops.daemon_calls.borrow()[0].1.clone();
        // Quoting is per platform; Windows wraps the JSON in escaped double quotes.
        assert!(
            command.starts_with("claude --resume --mcp-config ")
                && command.contains("http://127.0.0.1:47437/mcp"),
            "{command}"
        );
        let endpoint = ops.daemon_mcp.borrow()[0].clone().unwrap();
        let [(_, url), (_, token)] = endpoint.env();
        assert_eq!(url, "http://127.0.0.1:47437/mcp");
        assert_eq!(
            planeai_core::mcp::resolve(&conn, token)
                .unwrap()
                .unwrap()
                .session_id,
            id
        );
    }

    #[test]
    fn a_restart_while_the_mcp_server_is_not_listening_passes_no_endpoint() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", &existing_project_dir()).unwrap();
        let pid = db::list_projects(&conn).unwrap()[0].id.clone();
        let id = "eeee3333-5555-6666-7777-888899990000";
        db::create_session_with_id(
            &conn,
            id,
            &pid,
            "agent",
            None,
            "main",
            None,
            Some("claude"),
            "daemon",
            false,
            None,
            None,
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, id).unwrap();
        let ops = MockRestartOps::new();
        restart(&conn, id, &Config::default(), None, &ops).unwrap();

        assert_eq!(ops.daemon_calls.borrow()[0].1, "claude --resume");
        assert_eq!(*ops.daemon_mcp.borrow(), vec![None]);
    }

    #[test]
    fn a_restart_that_reattaches_a_live_tmux_session_keeps_its_agents_token() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", &existing_project_dir()).unwrap();
        let pid = db::list_projects(&conn).unwrap()[0].id.clone();
        let id = "ffff3333-5555-6666-7777-888899990000";
        db::create_session_with_id(
            &conn,
            id,
            &pid,
            "agent",
            Some("planeai-myapp-fff"),
            "main",
            None,
            Some("claude"),
            "tmux",
            false,
            None,
            None,
            None,
        )
        .unwrap();
        let url = "http://127.0.0.1:47437/mcp";
        let running = planeai_core::mcp::issue(&conn, id, url).unwrap();
        db::mark_session_exited(&conn, id).unwrap();
        let ops = MockRestartOps {
            tmux_alive: true,
            ..MockRestartOps::new()
        };

        let updated = restart(
            &conn,
            id,
            &Config::default(),
            Some(&planeai_core::mcp::McpLaunch::new(url)),
            &ops,
        )
        .unwrap();

        assert_eq!(updated.status, "active");
        assert!(ops.calls.borrow().is_empty());
        let [_, (_, token)] = running.env();
        assert_eq!(
            planeai_core::mcp::resolve(&conn, token)
                .unwrap()
                .map(|caller| caller.session_id),
            Some(id.to_string())
        );
    }
}
