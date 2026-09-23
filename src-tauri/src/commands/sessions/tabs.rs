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
    connection: std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
    config: crate::config::Config,
) -> Result<PreparedTab, String> {
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
        &session.project_id,
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

    let pty_key = format!("{}:{}", session_id, tab_index);

    // Build canonical env (augmented PATH, TERM, COLORFGBG, PLANEAI_SOCKET, etc.)
    // via prepare_session() — same for both backends.
    let shell_cmd = shell_command(&shell, initial_command.as_deref());
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

    let (target, daemon_spawn) = if session.backend == "daemon" {
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
        (
            pty::PtyTarget::Shell {
                command: shell_cmd,
                cwd: cwd.clone(),
            },
            None,
        )
    };

    Ok(PreparedTab {
        pty_key,
        target,
        env,
        daemon_spawn,
    })
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
    // daemon IPC. Do not lower the persisted count unless the daemon confirms
    // that this exact shell tab is gone (or was already gone).
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

    let connection = db_state.0.clone();
    let session_id_for_update = session_id.clone();
    if let Err(error) = crate::commands::blocking(move || {
        let conn = connection.lock().map_err(|e| e.to_string())?;
        let session = db::get_session(&conn, &session_id_for_update)
            .map_err(|e| e.to_string())?
            .ok_or("session not found")?;
        db::update_tab_count(
            &conn,
            &session_id_for_update,
            (session.tab_count - 1).max(1),
        )
        .map_err(|e| e.to_string())
    })
    .await
    {
        state.0.cancel_tab_close(&plan.pty_key);
        return Err(error);
    }

    // Detach only after the daemon shell has been confirmed dead and the
    // persisted count has been updated, so a failed kill leaves the tab live
    // rather than silently orphaning its shell. The close claim remains until
    // this PTY key is attached again, making duplicate pty-exited events safe.
    state.0.detach(&plan.pty_key);
    Ok(())
}

fn prepare_tab_close(
    session_id: String,
    tab_index: u32,
    connection: std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
) -> Result<TabClosePlan, String> {
    let conn = connection.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;

    Ok(TabClosePlan {
        pty_key: format!("{}:{}", session_id, tab_index),
        daemon_backed: session.backend == "daemon",
        rmux_backed: session.backend == planeai_rmux::BACKEND,
    })
}

#[tauri::command]
pub fn increment_tab_count(session_id: String, db_state: State<DbState>) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    db::update_tab_count(&conn, &session_id, session.tab_count + 1).map_err(|e| e.to_string())?;
    Ok(())
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
}
