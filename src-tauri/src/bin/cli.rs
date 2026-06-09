use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "planeai-cli")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    Create {
        #[arg(long)]
        project: String,
        #[arg(long)]
        branch: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        new_branch: bool,
        #[arg(long)]
        worktree: bool,
        #[arg(long)]
        base_branch: Option<String>,
        #[arg(long)]
        yolo: bool,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        task_key: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        pretty: bool,
    },
}

#[derive(Subcommand)]
enum ProjectAction {
    List {
        #[arg(long)]
        pretty: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    let db_path = planeai::paths::db_path();
    let conn = rusqlite::Connection::open(&db_path).unwrap_or_else(|e| {
        eprintln!("{{\"error\": \"failed to open database: {e}\"}}");
        std::process::exit(1);
    });

    match cli.command {
        Commands::Project { action } => match action {
            ProjectAction::List { pretty } => {
                let output = planeai::cli::run_project_list(&conn);
                if pretty {
                    let v: serde_json::Value = serde_json::from_str(&output).unwrap();
                    println!("{}", serde_json::to_string_pretty(&v).unwrap());
                } else {
                    println!("{output}");
                }
            }
        },
        Commands::Session { action } => match action {
            SessionAction::Create {
                project,
                branch,
                name,
                new_branch,
                worktree,
                base_branch,
                yolo,
                provider,
                task_key,
                prompt,
                pretty,
            } => {
                let cfg_dir = planeai::config::config_dir("planeai");
                let (cfg, _) = planeai::config::load(&cfg_dir);
                let backend = planeai::config::resolve_backend(&cfg).to_string();

                let env = planeai::cli::Env {
                    backend,
                    socket_path: planeai::paths::notify_socket_path(),
                    config: cfg,
                };

                let opts = planeai::cli::SessionCreateOpts {
                    project,
                    branch,
                    name,
                    new_branch,
                    worktree,
                    base_branch,
                    yolo,
                    provider,
                    task_key,
                    prompt,
                };

                let real_backend = RealBackend;
                match planeai::cli::run_session_create_with_env(&conn, &opts, &real_backend, &env) {
                    Ok(output) => {
                        if pretty {
                            let v: serde_json::Value = serde_json::from_str(&output).unwrap();
                            println!("{}", serde_json::to_string_pretty(&v).unwrap());
                        } else {
                            println!("{output}");
                        }
                    }
                    Err(e) => {
                        eprintln!("{{\"error\": \"{e}\"}}");
                        std::process::exit(1);
                    }
                }
            }
        },
    }
}

struct RealBackend;

impl planeai::cli::Backend for RealBackend {
    fn checkout_branch(
        &self,
        repo: &str,
        branch: &str,
        new: bool,
        base: Option<&str>,
    ) -> Result<(), String> {
        planeai::git::checkout_branch(repo, branch, new, base)
    }
    fn create_worktree(
        &self,
        repo: &str,
        path: &str,
        branch: &str,
        base: &str,
    ) -> Result<(), String> {
        planeai::git::worktree_add(repo, path, branch, base)
    }
    fn create_tmux_session(
        &self,
        name: &str,
        cwd: &str,
        cmd: &str,
        session_id: &str,
    ) -> Result<(), String> {
        planeai::tmux::create_session_with_cmd(name, cwd, cmd, session_id)
    }
}

#[cfg(test)]
mod tests {
    use planeai::config::Config;
    use planeai::db;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        conn
    }

    fn test_config() -> Config {
        Config::default()
    }

    use planeai::cli::Backend;
    use std::cell::RefCell;

    struct RecordingBackend {
        calls: RefCell<Vec<String>>,
    }

    impl RecordingBackend {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl Backend for RecordingBackend {
        fn checkout_branch(
            &self,
            _repo: &str,
            branch: &str,
            new: bool,
            base: Option<&str>,
        ) -> Result<(), String> {
            self.calls.borrow_mut().push(format!(
                "checkout:{branch}:new={new}:base={}",
                base.unwrap_or("none")
            ));
            Ok(())
        }
        fn create_worktree(
            &self,
            _repo: &str,
            _path: &str,
            branch: &str,
            base: &str,
        ) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(format!("worktree:{branch}:base={base}"));
            Ok(())
        }
        fn create_tmux_session(
            &self,
            name: &str,
            _cwd: &str,
            _cmd: &str,
            _session_id: &str,
        ) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("tmux:{name}"));
            Ok(())
        }
    }

    #[test]
    fn project_list_returns_registered_projects_as_json() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/home/user/myapp").unwrap();
        db::create_project(&conn, "backend", "/home/user/backend").unwrap();

        let output = planeai::cli::run_project_list(&conn);

        let parsed: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["name"], "myapp");
        assert_eq!(parsed[0]["path"], "/home/user/myapp");
        assert_eq!(parsed[1]["name"], "backend");
        assert_eq!(parsed[1]["path"], "/home/user/backend");
    }

    #[test]
    fn session_create_with_valid_project_creates_db_record() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/home/user/myapp").unwrap();

        let opts = planeai::cli::SessionCreateOpts {
            project: "myapp".to_string(),
            branch: "main".to_string(),
            name: Some("fix-bug".to_string()),
            new_branch: false,
            worktree: false,
            base_branch: None,
            yolo: false,
            provider: None,
            task_key: None,
            prompt: None,
        };

        let result = planeai::cli::run_session_create(&conn, &opts, &planeai::cli::NoOpBackend);

        assert!(result.is_ok());
        let output = result.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["name"], "fix-bug");
        assert_eq!(parsed["branch"], "main");
        assert_eq!(parsed["status"], "active");

        // Verify DB record exists
        let sessions = db::list_sessions(&conn).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "fix-bug");
        assert_eq!(sessions[0].branch, "main");
    }

    #[test]
    fn session_create_with_new_branch_calls_backend_with_new_flag() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/home/user/myapp").unwrap();
        let backend = RecordingBackend::new();

        let opts = planeai::cli::SessionCreateOpts {
            project: "myapp".to_string(),
            branch: "feat-x".to_string(),
            name: None,
            new_branch: true,
            worktree: false,
            base_branch: Some("main".to_string()),
            yolo: false,
            provider: None,
            task_key: None,
            prompt: None,
        };

        let result = planeai::cli::run_session_create(&conn, &opts, &backend);
        assert!(result.is_ok());

        let calls = backend.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], "checkout:feat-x:new=true:base=main");
        assert!(calls[1].starts_with("tmux:"));
    }

    #[test]
    fn session_create_with_worktree_calls_create_worktree() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/home/user/myapp").unwrap();
        let backend = RecordingBackend::new();

        let opts = planeai::cli::SessionCreateOpts {
            project: "myapp".to_string(),
            branch: "feat-wt".to_string(),
            name: Some("wt-session".to_string()),
            new_branch: false,
            worktree: true,
            base_branch: Some("main".to_string()),
            yolo: false,
            provider: None,
            task_key: None,
            prompt: None,
        };

        let result = planeai::cli::run_session_create(&conn, &opts, &backend);
        assert!(result.is_ok());

        let calls = backend.calls.borrow();
        assert!(calls[0].starts_with("worktree:feat-wt:base=main"));

        // Session should have worktree_path set
        let sessions = db::list_sessions(&conn).unwrap();
        assert!(sessions[0].worktree_path.is_some());
    }

    #[test]
    fn session_create_with_unknown_project_errors() {
        let conn = setup_db();

        let opts = planeai::cli::SessionCreateOpts {
            project: "nonexistent".to_string(),
            branch: "main".to_string(),
            name: None,
            new_branch: false,
            worktree: false,
            base_branch: None,
            yolo: false,
            provider: None,
            task_key: None,
            prompt: None,
        };

        let result = planeai::cli::run_session_create(&conn, &opts, &planeai::cli::NoOpBackend);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown project"));
    }

    #[test]
    fn session_create_with_direct_backend_and_no_socket_errors() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/home/user/myapp").unwrap();

        let opts = planeai::cli::SessionCreateOpts {
            project: "myapp".to_string(),
            branch: "main".to_string(),
            name: None,
            new_branch: false,
            worktree: false,
            base_branch: None,
            yolo: false,
            provider: None,
            task_key: None,
            prompt: None,
        };

        let env = planeai::cli::Env {
            backend: "direct".to_string(),
            socket_path: std::path::PathBuf::from("/nonexistent/notify.sock"),
            config: test_config(),
        };

        let result = planeai::cli::run_session_create_with_env(
            &conn,
            &opts,
            &planeai::cli::NoOpBackend,
            &env,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("GUI is not running"));
    }

    #[test]
    fn session_create_with_tmux_backend_spawns_tmux_session() {
        let conn = setup_db();
        db::create_project(&conn, "myapp", "/home/user/myapp").unwrap();
        let backend = RecordingBackend::new();

        let opts = planeai::cli::SessionCreateOpts {
            project: "myapp".to_string(),
            branch: "main".to_string(),
            name: Some("my-session".to_string()),
            new_branch: false,
            worktree: false,
            base_branch: None,
            yolo: false,
            provider: None,
            task_key: None,
            prompt: None,
        };

        let env = planeai::cli::Env {
            backend: "tmux".to_string(),
            socket_path: std::path::PathBuf::from("/tmp/fake.sock"),
            config: test_config(),
        };

        let result = planeai::cli::run_session_create_with_env(&conn, &opts, &backend, &env);
        assert!(result.is_ok());

        let calls = backend.calls.borrow();
        assert!(calls.iter().any(|c| c.starts_with("tmux:")));

        // Session should have tmux_name set
        let sessions = db::list_sessions(&conn).unwrap();
        assert!(sessions[0].tmux_name.is_some());
    }

    #[test]
    fn session_create_sends_socket_notification() {
        use std::io::{BufRead, BufReader};
        use std::os::unix::net::UnixListener;

        let conn = setup_db();
        db::create_project(&conn, "myapp", "/home/user/myapp").unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let sock_path = tmp.path().join("notify.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();
        listener.set_nonblocking(true).unwrap();

        let opts = planeai::cli::SessionCreateOpts {
            project: "myapp".to_string(),
            branch: "main".to_string(),
            name: None,
            new_branch: false,
            worktree: false,
            base_branch: None,
            yolo: false,
            provider: None,
            task_key: None,
            prompt: None,
        };

        let env = planeai::cli::Env {
            backend: "tmux".to_string(),
            socket_path: sock_path.clone(),
            config: test_config(),
        };

        let result = planeai::cli::run_session_create_with_env(
            &conn,
            &opts,
            &planeai::cli::NoOpBackend,
            &env,
        );
        assert!(result.is_ok());

        // Read the message sent to the socket
        let (stream, _) = listener
            .accept()
            .expect("should have received a connection");
        let reader = BufReader::new(stream);
        let line = reader.lines().next().unwrap().unwrap();
        let msg: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(msg["event"], "session_created");
        assert!(msg["session_id"].as_str().is_some());
    }
}
