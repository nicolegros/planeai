use tauri::ipc::Channel;
use tauri::State;

use crate::config;
use crate::db;
use crate::pty;
use crate::state::{ConfigState, DbState, PtyState};

struct DaemonShellTabSpawn {
    session_id: String,
    command: String,
    args: Vec<String>,
    cwd: String,
    env: std::collections::HashMap<String, String>,
}

struct PreparedTab {
    pty_key: String,
    target: pty::PtyTarget,
    env: Vec<(String, String)>,
    daemon_spawn: Option<DaemonShellTabSpawn>,
}

fn shell_args() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &[]
    }
    #[cfg(not(windows))]
    {
        &["-l"]
    }
}

fn shell_command(shell: &str, initial_command: Option<&str>) -> String {
    #[cfg(windows)]
    {
        let shell = format!("\"{}\"", shell.replace('"', "\\\""));
        return match initial_command {
            Some(command) => {
                let command_shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
                format!("\"{}\" /K {command}", command_shell.replace('"', "\\\""))
            }
            None => shell,
        };
    }
    #[cfg(not(windows))]
    {
        let args = shell_args();
        let login_shell = if args.is_empty() {
            shell.to_string()
        } else {
            format!("{shell} {}", args.join(" "))
        };
        initial_command
            .map(|command| format!("{command}; exec {login_shell}"))
            .unwrap_or(login_shell)
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn spawn_tab(
    session_id: String,
    tab_index: u32,
    dark_mode: Option<bool>,
    initial_command: Option<String>,
    initial_argv: Option<Vec<String>>,
    on_data: Channel<tauri::ipc::Response>,
    db_state: State<'_, DbState>,
    config_state: State<'_, ConfigState>,
    state: State<'_, PtyState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let connection = db_state.0.clone();
    let config = config_state.0.lock().map_err(|e| e.to_string())?.clone();
    let pty = state.0.clone();

    let prepared = crate::commands::blocking(move || {
        prepare_tab_spawn(
            session_id,
            tab_index,
            dark_mode,
            initial_command,
            initial_argv,
            connection,
            config,
        )
    })
    .await?;

    if let Some(daemon_spawn) = prepared.daemon_spawn.as_ref() {
        crate::daemon_client::spawn_shell_tab(
            &planeai_ipc::daemon_socket_path(),
            &daemon_spawn.session_id,
            &daemon_spawn.command,
            &daemon_spawn.args,
            &daemon_spawn.cwd,
            &daemon_spawn.env,
        )
        .await?;
    }

    crate::commands::blocking(move || {
        pty.attach(
            &prepared.pty_key,
            prepared.target,
            app,
            on_data,
            prepared.env,
        )
    })
    .await
}

#[allow(clippy::too_many_arguments)]
fn prepare_tab_spawn(
    session_id: String,
    tab_index: u32,
    dark_mode: Option<bool>,
    initial_command: Option<String>,
    initial_argv: Option<Vec<String>>,
    connection: std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
    config: crate::config::Config,
) -> Result<PreparedTab, String> {
    if initial_argv.as_ref().is_some_and(Vec::is_empty) {
        return Err("a tab's program cannot be empty".to_string());
    }
    let conn = connection.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
    let project_path = projects
        .iter()
        .find(|p| p.id == session.project_id)
        .map(|p| p.path.as_str())
        .unwrap_or("/");
    let cwd = session
        .worktree_path
        .as_deref()
        .unwrap_or(project_path)
        .to_string();
    let rmux_workspace = planeai_rmux::WorkspaceKey::for_session(
        session.task_project_id(),
        session.task_key.as_deref(),
        &session.id,
    )
    .name();
    drop(conn);

    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(windows) {
            "cmd.exe".to_string()
        } else {
            "/bin/zsh".to_string()
        }
    });

    let pty_key = pty::tab_key(&session_id, tab_index);

    // Build canonical env (augmented PATH, TERM, COLORFGBG, PLANEAI_SOCKET, etc.)
    // via prepare_session() — same for both backends.
    let shell_cmd = match &initial_argv {
        Some(argv) => argv.join(" "),
        None => shell_command(&shell, initial_command.as_deref()),
    };
    let env = {
        let extra_path_dirs = config.resolved_extra_path_dirs();
        super::helpers::build_local_env(
            &pty_key,
            std::path::PathBuf::from(&cwd),
            &shell_cmd,
            dark_mode.unwrap_or(true),
            extra_path_dirs,
        )?
    };

    tracing::info!(
        pty_key = %pty_key,
        backend = %session.backend,
        has_initial_command = initial_command.is_some(),
        resolved_command = %shell_cmd,
        "prepare_tab_spawn"
    );

    // Programs run without a shell only in local PTYs, which provider sessions' tabs are.
    let no_program_here = || {
        format!(
            "{} sessions cannot run a program without a shell",
            session.backend
        )
    };
    let (target, daemon_spawn) = if session.backend == "daemon" {
        if initial_argv.is_some() {
            return Err(no_program_here());
        }
        #[cfg(not(windows))]
        let (daemon_command, daemon_args): (String, Vec<String>) = match initial_command.as_deref()
        {
            Some(_) => (
                "/bin/sh".to_string(),
                vec!["-c".to_string(), shell_cmd.clone()],
            ),
            None => (
                shell.clone(),
                shell_args().iter().map(|arg| (*arg).to_string()).collect(),
            ),
        };
        #[cfg(windows)]
        let (daemon_command, daemon_args): (String, Vec<String>) = {
            let command_shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
            match initial_command.as_deref() {
                Some(command) => (command_shell, vec!["/K".to_string(), command.to_string()]),
                None => (shell.clone(), Vec::new()),
            }
        };
        let daemon_spawn = DaemonShellTabSpawn {
            session_id: pty_key.clone(),
            command: daemon_command,
            args: daemon_args,
            cwd: cwd.clone(),
            env: env.iter().cloned().collect(),
        };
        (
            pty::PtyTarget::Daemon {
                session_id: pty_key.clone(),
                socket_path: planeai_ipc::daemon_socket_path(),
            },
            Some(daemon_spawn),
        )
    } else if session.backend == planeai_rmux::BACKEND {
        if initial_argv.is_some() {
            return Err(no_program_here());
        }
        // A shell tab becomes its own window in the session's task workspace, so
        // it persists exactly like the agent pane instead of dying with the app.
        let owned_env: Vec<(String, String)> = env.clone();
        let borrowed: std::collections::HashMap<&str, &str> = owned_env
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        let handle = crate::rmux_ops::spawn_resource_blocking(
            &pty_key,
            &session.id,
            &rmux_workspace,
            &shell_cmd,
            &cwd,
            &borrowed,
        )?;
        (
            pty::PtyTarget::Rmux {
                pty_key: pty_key.clone(),
                workspace: rmux_workspace,
                handle,
            },
            None,
        )
    } else {
        let target = match initial_argv {
            Some(mut argv) => {
                resolve_program(&mut argv, &env);
                pty::PtyTarget::Program {
                    argv,
                    cwd: cwd.clone(),
                }
            }
            None => pty::PtyTarget::Shell {
                command: shell_cmd,
                cwd: cwd.clone(),
            },
        };
        (target, None)
    };

    Ok(PreparedTab {
        pty_key,
        target,
        env,
        daemon_spawn,
    })
}

/// Resolves a bare program name with the tab's own PATH, as a shell would, so Windows runs
/// the `.cmd` shim npm installs; a name not found is left for the spawn to report.
fn resolve_program(argv: &mut [String], env: &[(String, String)]) {
    let path = env
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value.as_str())
        .unwrap_or_default();
    if let Some(program) = argv
        .first()
        .and_then(|name| crate::config::find_executable_in(name, path))
    {
        argv[0] = program.to_string_lossy().into_owned();
    }
}

struct TabClosePlan {
    pty_key: String,
    daemon_backed: bool,
    rmux_backed: bool,
}

#[tauri::command]
pub async fn close_tab(
    session_id: String,
    tab_index: u32,
    db_state: State<'_, DbState>,
    state: State<'_, PtyState>,
) -> Result<(), String> {
    let connection = db_state.0.clone();
    let plan = crate::commands::blocking({
        let session_id = session_id.clone();
        move || prepare_tab_close(session_id, tab_index, connection)
    })
    .await?;

    if !state.0.claim_tab_close(&plan.pty_key) {
        return Ok(());
    }

    // The database lock was released by prepare_tab_close before this bounded
    // daemon IPC.
    if plan.daemon_backed {
        if let Err(error) =
            crate::daemon_client::kill_shell_tab(&planeai_ipc::daemon_socket_path(), &plan.pty_key)
                .await
        {
            state.0.cancel_tab_close(&plan.pty_key);
            return Err(error);
        }
    }

    // An rmux shell tab is its own window, so closing the tab closes that pane
    // and leaves the agent and sibling tabs untouched.
    if plan.rmux_backed {
        let pty_key = plan.pty_key.clone();
        if let Err(error) =
            crate::commands::blocking(move || crate::rmux_ops::close_resource(&pty_key)).await
        {
            state.0.cancel_tab_close(&plan.pty_key);
            return Err(error);
        }
    }

    // Detach only after the daemon shell has been confirmed dead, so a failed
    // kill leaves the tab live rather than silently orphaning its shell. The claim
    // makes a concurrent duplicate close a no-op; once this close finishes, the
    // frontend drops a late duplicate for the tab it already removed.
    state.0.detach(&plan.pty_key);
    Ok(())
}

fn prepare_tab_close(
    session_id: String,
    tab_index: u32,
    connection: std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
) -> Result<TabClosePlan, String> {
    let conn = connection.lock().map_err(|e| e.to_string())?;
    // A session deleted already (from the CLI, or with its project) leaves only local tabs to
    // end, such as a handoff's program, which must not outlive it.
    let backend = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .map(|session| session.backend);

    Ok(TabClosePlan {
        pty_key: pty::tab_key(&session_id, tab_index),
        daemon_backed: backend.as_deref() == Some("daemon"),
        rmux_backed: backend.as_deref() == Some(planeai_rmux::BACKEND),
    })
}

/// The tabs among `pty_keys` still running their program, so a reloaded webview can tell a
/// live provider handoff from a tab that came back as a plain shell after an app restart.
#[tauri::command]
pub async fn running_program_tabs(
    pty_keys: Vec<String>,
    state: State<'_, PtyState>,
) -> Result<Vec<String>, String> {
    Ok(state.0.running_programs(&pty_keys))
}

#[tauri::command]
pub fn check_tmux_available() -> bool {
    config::tmux_available()
}

/// Whether an rmux daemon binary is reachable, for the Preferences warning.
///
/// A bundled sidecar wins over PATH, matching how the backend resolves it.
#[tauri::command]
pub async fn check_rmux_available(app: tauri::AppHandle) -> bool {
    crate::commands::blocking(move || {
        Ok(crate::paths::resolve_rmux_daemon_binary(&app).is_file() || config::rmux_available())
    })
    .await
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn uses_a_login_shell_on_unix() {
        assert_eq!(shell_args(), ["-l"]);
        assert_eq!(shell_command("/bin/zsh", None), "/bin/zsh -l");
    }

    #[cfg(not(windows))]
    #[test]
    fn runs_an_initial_command_before_preserving_the_login_shell() {
        assert_eq!(
            shell_command("/bin/zsh", Some("nvim '/work/my repo/file.rs'")),
            "nvim '/work/my repo/file.rs'; exec /bin/zsh -l"
        );
    }

    #[cfg(windows)]
    #[test]
    fn uses_cmd_for_an_initial_command_even_when_shell_is_custom() {
        let command = shell_command(r"C:\Program Files\Git\bin\bash.exe", Some("nvim file.rs"));
        assert!(command.contains(" /K nvim file.rs"));
        assert!(!command.contains("bash.exe\" /K"));
    }

    #[cfg(windows)]
    #[test]
    fn does_not_pass_a_login_flag_to_cmd() {
        assert!(shell_args().is_empty());
        assert_eq!(shell_command("cmd.exe", None), "\"cmd.exe\"");
        assert_eq!(
            shell_command(r"C:\Program Files\Git\bin\bash.exe", None),
            r#""C:\Program Files\Git\bin\bash.exe""#
        );
    }

    fn tab_for(backend: &str, argv: Option<Vec<String>>) -> Result<PreparedTab, String> {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "app", "/tmp").unwrap();
        db::create_session_with_id(
            &conn,
            "s1",
            &project.id,
            "chat",
            None,
            "main",
            None,
            None,
            backend,
            false,
            None,
            None,
            None,
        )
        .unwrap();
        prepare_tab_spawn(
            "s1".into(),
            2,
            Some(true),
            None,
            argv,
            std::sync::Arc::new(std::sync::Mutex::new(conn)),
            crate::config::Config::default(),
        )
    }

    #[test]
    fn a_program_runs_without_a_shell_in_a_local_tab() {
        let argv = vec![
            "/opt/claude".to_string(),
            "--resume".into(),
            "it's 100%".into(),
        ];
        let tab = tab_for("plugin", Some(argv.clone())).unwrap();
        match tab.target {
            pty::PtyTarget::Program { argv: target, .. } => assert_eq!(target, argv),
            other => panic!("expected a program target, got {other:?}"),
        }
        assert!(tab.daemon_spawn.is_none());
    }

    #[test]
    fn a_program_needs_a_local_tab_and_a_name() {
        assert!(tab_for("daemon", Some(vec!["claude".into()])).is_err());
        assert!(tab_for(planeai_rmux::BACKEND, Some(vec!["claude".into()])).is_err());
        assert!(tab_for("plugin", Some(Vec::new())).is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn a_tab_program_resolves_with_the_tabs_path() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("fake-agent");
        std::fs::write(&program, "").unwrap();
        let env = vec![("PATH".to_string(), dir.path().display().to_string())];

        let mut argv = vec!["fake-agent".to_string(), "--resume".to_string()];
        resolve_program(&mut argv, &env);
        assert_eq!(
            argv,
            [program.display().to_string(), "--resume".to_string()]
        );

        let mut missing = vec!["missing-agent".to_string()];
        resolve_program(&mut missing, &env);
        assert_eq!(missing, ["missing-agent"]);
    }
}
