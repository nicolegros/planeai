use std::sync::Arc;

use tauri::ipc::{Channel, Response};
use tauri::{AppHandle, Manager, State};

use crate::config;
use crate::db;
use crate::pty;
use crate::state::{ConfigState, DbState, PtyState, TerminalTabsState};
use crate::terminal_tabs::{Attached, TabHost, TabId, TabSpec};

struct DaemonShellTabSpawn {
    command: String,
    args: Vec<String>,
    cwd: String,
    env: std::collections::HashMap<String, String>,
}

/// Where a tab's process runs, and what starting it takes.
enum TabLaunch {
    /// A PTY of this app.
    Local(pty::PtyTarget),
    Daemon(DaemonShellTabSpawn),
    /// A pane of its session's rmux workspace, adopted when one already hosts the tab.
    Rmux {
        session_id: String,
        workspace: planeai_rmux::WorkspaceName,
        argv: Vec<String>,
        cwd: String,
    },
}

/// The backend running a session's terminal tabs.
enum TabBackend {
    Local,
    Daemon,
    Rmux {
        workspace: planeai_rmux::WorkspaceName,
    },
}

impl TabBackend {
    fn of(session: &db::Session) -> Self {
        match session.backend.as_str() {
            "daemon" => Self::Daemon,
            planeai_rmux::BACKEND => Self::Rmux {
                workspace: session.rmux_workspace(),
            },
            _ => Self::Local,
        }
    }
}

struct PreparedTab {
    pty_key: String,
    launch: TabLaunch,
    env: Vec<(String, String)>,
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

/// Reserve a terminal tab for a session; its process starts when a terminal attaches to it.
#[tauri::command]
pub async fn open_tab(
    session_id: String,
    spec: TabSpec,
    tabs: State<'_, TerminalTabsState>,
) -> Result<u32, String> {
    let tabs = tabs.0.clone();
    crate::commands::blocking(move || tabs.open(&session_id, spec).map(|tab| tab.index())).await
}

/// Start a terminal tab, or connect to the process already running it. False when the tab
/// ended instead, which its `tab-ended` reports.
#[tauri::command]
pub async fn attach_tab(
    session_id: String,
    tab_index: u32,
    dark_mode: Option<bool>,
    on_data: Channel<Response>,
    tabs: State<'_, TerminalTabsState>,
) -> Result<bool, String> {
    let output = TabOutput {
        channel: on_data,
        dark_mode: dark_mode.unwrap_or(true),
    };
    let attached = tabs
        .0
        .attach(&TabId::new(session_id, tab_index), output)
        .await?;
    Ok(attached == Attached::Live)
}

pub struct TabOutput {
    channel: Channel<Response>,
    dark_mode: bool,
}

/// Runs terminal tabs on their session's backend: a local PTY, a daemon shell or an rmux pane.
pub struct PtyTabHost {
    app: AppHandle,
}

impl PtyTabHost {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    /// Connect a terminal to the process running a live tab, failing when it is gone.
    async fn reconnect(&self, tab: &TabId, channel: Channel<Response>) -> Result<(), String> {
        let connection = self.app.state::<DbState>().0.clone();
        let pty = self.app.state::<PtyState>().0.clone();
        let backend = crate::commands::blocking({
            let tab = tab.clone();
            move || tab_backend(&tab, connection)
        })
        .await?
        .ok_or("session not found")?;
        let pty_key = tab.key();
        // None of these starts a process.
        let target = match backend {
            TabBackend::Local => return pty.reconnect(&pty_key, channel),
            TabBackend::Daemon => {
                let socket_path = planeai_ipc::daemon_socket_path();
                if !crate::daemon_client::is_shell_tab_running(&socket_path, &pty_key).await? {
                    return Err(release_gone(&pty, &pty_key));
                }
                pty::PtyTarget::Daemon {
                    session_id: pty_key.clone(),
                    socket_path,
                }
            }
            // Found by name, as a spawn adopts it: a recorded pane id may since name another pane.
            TabBackend::Rmux { workspace } => {
                let (name, key) = (workspace.clone(), pty_key.clone());
                let pane = crate::commands::blocking(move || {
                    crate::rmux_ops::find_tab_resource(&name, &key)
                })
                .await?;
                let Some(handle) = pane else {
                    return Err(release_gone(&pty, &pty_key));
                };
                pty::PtyTarget::Rmux {
                    pty_key: pty_key.clone(),
                    workspace,
                    handle,
                }
            }
        };
        crate::commands::blocking(move || pty.attach(&pty_key, target, channel, Vec::new())).await
    }
}

impl TabHost for PtyTabHost {
    type Output = TabOutput;

    async fn attach(
        &self,
        tab: &TabId,
        spec: &TabSpec,
        output: TabOutput,
        reconnect: bool,
    ) -> Result<(), String> {
        if reconnect {
            return self.reconnect(tab, output.channel).await;
        }
        let connection = self.app.state::<DbState>().0.clone();
        let config = self
            .app
            .state::<ConfigState>()
            .0
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        let pty = self.app.state::<PtyState>().0.clone();
        let (tab, spec) = (tab.clone(), spec.clone());
        let prepared = crate::commands::blocking(move || {
            prepare_tab_spawn(&tab, &spec, output.dark_mode, connection, config)
        })
        .await?;
        let PreparedTab {
            pty_key,
            launch,
            env,
        } = prepared;

        // A tab saved before an app restart may still run in the daemon or rmux: both adopt it.
        let target = match launch {
            TabLaunch::Local(target) => target,
            TabLaunch::Daemon(spawn) => {
                let socket_path = planeai_ipc::daemon_socket_path();
                crate::daemon_client::spawn_shell_tab(
                    &socket_path,
                    &pty_key,
                    &spawn.command,
                    &spawn.args,
                    &spawn.cwd,
                    &spawn.env,
                )
                .await?;
                pty::PtyTarget::Daemon {
                    session_id: pty_key.clone(),
                    socket_path,
                }
            }
            // A shell tab is its own window in the session's task workspace, so it persists
            // exactly like the agent pane instead of dying with the app.
            TabLaunch::Rmux {
                session_id,
                workspace,
                argv,
                cwd,
            } => {
                let (key, pane_workspace, pane_env) =
                    (pty_key.clone(), workspace.clone(), env.clone());
                let handle = crate::commands::blocking(move || {
                    let env = pane_env
                        .iter()
                        .map(|(key, value)| (key.as_str(), value.as_str()))
                        .collect();
                    crate::rmux_ops::spawn_resource_blocking(
                        &key,
                        &session_id,
                        &pane_workspace,
                        argv,
                        &cwd,
                        &env,
                    )
                })
                .await?;
                pty::PtyTarget::Rmux {
                    pty_key: pty_key.clone(),
                    workspace,
                    handle,
                }
            }
        };

        let channel = output.channel;
        crate::commands::blocking(move || pty.attach(&pty_key, target, channel, env)).await
    }

    async fn end(&self, tab: &TabId) -> Result<(), String> {
        let connection = self.app.state::<DbState>().0.clone();
        let pty = self.app.state::<PtyState>().0.clone();
        // A session deleted already (from the CLI, or with its project) leaves only local tabs to
        // end, such as a handoff's program, which must not outlive it.
        let backend = crate::commands::blocking({
            let tab = tab.clone();
            move || tab_backend(&tab, connection)
        })
        .await?
        .unwrap_or(TabBackend::Local);
        let pty_key = tab.key();
        match backend {
            TabBackend::Local => {}
            TabBackend::Daemon => {
                crate::daemon_client::kill_shell_tab(&planeai_ipc::daemon_socket_path(), &pty_key)
                    .await?;
            }
            // An rmux shell tab is its own window, so closing the tab closes that pane
            // and leaves the agent and sibling tabs untouched.
            TabBackend::Rmux { workspace } => {
                let key = pty_key.clone();
                crate::commands::blocking(move || {
                    crate::rmux_ops::close_tab_resource(&workspace, &key)
                })
                .await?;
            }
        }
        // Detached only once the daemon shell or rmux pane is confirmed gone, so a failed
        // kill keeps the tab rather than silently orphaning its shell.
        pty.detach(&pty_key);
        Ok(())
    }

    fn release(&self, tab: &TabId) {
        self.app.state::<PtyState>().0.detach(&tab.key());
    }

    fn is_running(&self, tab: &TabId) -> bool {
        self.app.state::<PtyState>().0.is_running(&tab.key())
    }
}

/// A live tab's process is gone: released, so the tab ends as exited rather than wait for an exit
/// that may never be reported.
fn release_gone(pty: &pty::PtyManager, pty_key: &str) -> String {
    pty.detach(pty_key);
    pty::gone_error(pty_key)
}

/// The backend of a tab's session; `None` once the session is gone.
fn tab_backend(
    tab: &TabId,
    connection: Arc<std::sync::Mutex<rusqlite::Connection>>,
) -> Result<Option<TabBackend>, String> {
    let conn = connection.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, tab.session_id()).map_err(|e| e.to_string())?;
    Ok(session.as_ref().map(TabBackend::of))
}

fn prepare_tab_spawn(
    tab: &TabId,
    spec: &TabSpec,
    dark_mode: bool,
    connection: Arc<std::sync::Mutex<rusqlite::Connection>>,
    config: crate::config::Config,
) -> Result<PreparedTab, String> {
    let conn = connection.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, tab.session_id())
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
    drop(conn);

    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(windows) {
            "cmd.exe".to_string()
        } else {
            "/bin/zsh".to_string()
        }
    });

    let pty_key = tab.key();

    let shell_cmd = match spec {
        TabSpec::Shell => shell_command(&shell, None),
        TabSpec::Command { command } => shell_command(&shell, Some(command)),
        TabSpec::Program { argv } => argv.join(" "),
    };
    // Under WSL a shell or command tab runs in the distro, whatever the backend. Programs are
    // handed off by plugins on the host, so they stay there.
    let wsl = config
        .wsl_target()
        .filter(|_| !matches!(spec, TabSpec::Program { .. }));
    let in_wsl = wsl.as_ref().map(|target| {
        let argv = match spec {
            TabSpec::Command { command } => {
                target.shell_argv(&format!("{command}; exec \"${{SHELL:-/bin/sh}}\" -l"), &cwd)
            }
            _ => target.login_shell_argv(&cwd),
        };
        (argv, target.cwds(&cwd).0)
    });
    // The same env (augmented PATH, TERM, COLORFGBG, PLANEAI_SOCKET) for every backend.
    let env = {
        let extra_path_dirs = config.resolved_extra_path_dirs();
        super::helpers::build_local_env(
            &pty_key,
            std::path::PathBuf::from(&cwd),
            &shell_cmd,
            dark_mode,
            extra_path_dirs,
            wsl,
        )?
    };

    tracing::info!(
        pty_key = %pty_key,
        backend = %session.backend,
        ?spec,
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
    let runs_program = matches!(spec, TabSpec::Program { .. });
    let launch = match (TabBackend::of(&session), in_wsl) {
        (TabBackend::Daemon, Some((mut argv, cwd))) => TabLaunch::Daemon(DaemonShellTabSpawn {
            command: argv.remove(0),
            args: argv,
            cwd,
            env: env.iter().cloned().collect(),
        }),
        (TabBackend::Rmux { workspace }, Some((argv, cwd))) => TabLaunch::Rmux {
            session_id: session.id.clone(),
            workspace,
            argv,
            cwd,
        },
        (TabBackend::Local, Some((argv, cwd))) => {
            TabLaunch::Local(pty::PtyTarget::Program { argv, cwd })
        }
        (backend, None) => match backend {
            TabBackend::Daemon | TabBackend::Rmux { .. } if runs_program => {
                return Err(no_program_here());
            }
            TabBackend::Daemon => {
                #[cfg(not(windows))]
                let (command, args): (String, Vec<String>) = match spec {
                    TabSpec::Command { .. } => (
                        "/bin/sh".to_string(),
                        vec!["-c".to_string(), shell_cmd.clone()],
                    ),
                    _ => (
                        shell.clone(),
                        shell_args().iter().map(|arg| (*arg).to_string()).collect(),
                    ),
                };
                #[cfg(windows)]
                let (command, args): (String, Vec<String>) = {
                    let command_shell =
                        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
                    match spec {
                        TabSpec::Command { command } => {
                            (command_shell, vec!["/K".to_string(), command.clone()])
                        }
                        _ => (shell.clone(), Vec::new()),
                    }
                };
                TabLaunch::Daemon(DaemonShellTabSpawn {
                    command,
                    args,
                    cwd,
                    env: env.iter().cloned().collect(),
                })
            }
            TabBackend::Rmux { workspace } => TabLaunch::Rmux {
                session_id: session.id.clone(),
                workspace,
                argv: planeai_rmux::shell_argv(&shell_cmd),
                cwd,
            },
            TabBackend::Local => TabLaunch::Local(match spec {
                TabSpec::Program { argv } => {
                    let mut argv = argv.clone();
                    resolve_program(&mut argv, &env);
                    pty::PtyTarget::Program { argv, cwd }
                }
                _ => pty::PtyTarget::Shell {
                    command: shell_cmd,
                    cwd,
                },
            }),
        },
    };

    Ok(PreparedTab {
        pty_key,
        launch,
        env,
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

/// Close a terminal tab; it stays when its process could not be ended.
#[tauri::command]
pub async fn close_tab(
    session_id: String,
    tab_index: u32,
    tabs: State<'_, TerminalTabsState>,
) -> Result<(), String> {
    tabs.0.close(&TabId::new(session_id, tab_index)).await
}

/// The tabs among `pty_keys` that ended, so a saved layout still holding them drops them.
#[tauri::command]
pub async fn ended_tabs(
    pty_keys: Vec<String>,
    tabs: State<'_, TerminalTabsState>,
) -> Result<Vec<String>, String> {
    let candidates = pty_keys
        .iter()
        .filter_map(|key| TabId::parse(key))
        .collect();
    let ended = tabs.0.ended_among(candidates).await?;
    Ok(ended.iter().map(TabId::key).collect())
}

/// Whether the tab still runs its program, so a reloaded webview can tell a live provider
/// handoff from a tab that came back as a plain shell after an app restart.
#[tauri::command]
pub async fn is_program_running(
    pty_key: String,
    tabs: State<'_, TerminalTabsState>,
) -> Result<bool, String> {
    Ok(TabId::parse(&pty_key).is_some_and(|tab| tabs.0.is_program_running(&tab)))
}

#[tauri::command]
pub fn check_tmux_available() -> bool {
    config::tmux_available()
}

/// Whether an rmux daemon binary is reachable, for the Preferences warning.
#[tauri::command]
pub async fn check_rmux_available() -> bool {
    crate::commands::blocking(|| Ok(config::rmux_available()))
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
        let spec = match argv {
            Some(argv) => TabSpec::Program { argv },
            None => TabSpec::Shell,
        };
        prepare_tab_spawn(
            &TabId::new("s1", 2),
            &spec,
            true,
            session_db(backend),
            crate::config::Config::default(),
        )
    }

    fn session_db(backend: &str) -> Arc<std::sync::Mutex<rusqlite::Connection>> {
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
        Arc::new(std::sync::Mutex::new(conn))
    }

    #[test]
    fn a_tab_is_reached_through_its_sessions_backend() {
        let tab = TabId::new("s1", 2);
        let backend = |name: &str| tab_backend(&tab, session_db(name)).unwrap();
        assert!(matches!(backend("plugin"), Some(TabBackend::Local)));
        assert!(matches!(backend("daemon"), Some(TabBackend::Daemon)));
        assert!(matches!(
            backend(planeai_rmux::BACKEND),
            Some(TabBackend::Rmux { .. })
        ));
        let gone = session_db("plugin");
        gone.lock()
            .unwrap()
            .execute("DELETE FROM sessions WHERE id = 's1'", [])
            .unwrap();
        assert!(tab_backend(&tab, gone).unwrap().is_none());
    }

    #[test]
    fn a_program_runs_without_a_shell_in_a_local_tab() {
        let argv = vec![
            "/opt/claude".to_string(),
            "--resume".into(),
            "it's 100%".into(),
        ];
        let tab = tab_for("plugin", Some(argv.clone())).unwrap();
        match tab.launch {
            TabLaunch::Local(pty::PtyTarget::Program { argv: target, .. }) => {
                assert_eq!(target, argv)
            }
            _ => panic!("expected a local program"),
        }
    }

    #[test]
    fn an_rmux_tab_is_planned_as_a_pane_of_its_workspace_without_spawning_it() {
        let tab = tab_for(planeai_rmux::BACKEND, None).unwrap();
        assert_eq!(tab.pty_key, "s1:2");
        match tab.launch {
            TabLaunch::Rmux { session_id, .. } => assert_eq!(session_id, "s1"),
            _ => panic!("expected an rmux pane"),
        }
    }

    #[test]
    fn a_program_needs_a_local_tab() {
        assert!(tab_for("daemon", Some(vec!["claude".into()])).is_err());
        assert!(tab_for(planeai_rmux::BACKEND, Some(vec!["claude".into()])).is_err());
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
