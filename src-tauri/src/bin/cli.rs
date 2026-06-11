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
    Symphony {
        #[command(subcommand)]
        action: SymphonyAction,
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
    #[command(name = "ls")]
    List {
        #[arg(long)]
        archived: bool,
        #[arg(long)]
        pretty: bool,
    },
    Delete {
        id: String,
        #[arg(long)]
        pretty: bool,
    },
    Archive {
        id: String,
        #[arg(long)]
        pretty: bool,
    },
    Prompt {
        id: String,
        text: Option<String>,
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

#[derive(Subcommand)]
enum SymphonyAction {
    /// Show orchestrator status (running sessions, concurrency)
    Status,
    /// Stop the orchestrator daemon
    Stop,
}

fn main() {
    let cli = Cli::parse();

    let log_dir = planeai::paths::app_data_dir().join("logs");
    let _guard = planeai::logging::init(&log_dir);

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
                    project: project.clone(),
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

                let projects = planeai::db::list_projects(&conn).unwrap_or_else(|e| {
                    eprintln!("{{\"error\": \"{e}\"}}");
                    std::process::exit(1);
                });
                let proj = match projects.iter().find(|p| p.name == project) {
                    Some(p) => p,
                    None => {
                        eprintln!("{{\"error\": \"unknown project: {project}\"}}");
                        std::process::exit(1);
                    }
                };

                let session_id = uuid::Uuid::new_v4().to_string();
                let plan = match planeai::cli::build_session_plan(&session_id, &opts, &env, proj) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("{{\"error\": \"{e}\"}}");
                        std::process::exit(1);
                    }
                };

                match planeai::cli::execute_plan(&plan, &conn, &env) {
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
            SessionAction::List { archived, pretty } => {
                match planeai::session_ops::list(&conn, archived) {
                    Ok(sessions) => {
                        if pretty {
                            let projects = planeai::db::list_projects(&conn).unwrap_or_default();
                            print!(
                                "{}",
                                planeai::session_ops::format_table(&sessions, &projects)
                            );
                        } else {
                            println!("{}", serde_json::to_string(&sessions).unwrap());
                        }
                    }
                    Err(e) => {
                        eprintln!("{{\"error\": \"{e}\"}}");
                        std::process::exit(1);
                    }
                }
            }
            SessionAction::Delete { id, pretty } => {
                let session = match planeai::session_ops::resolve_session_by_prefix(&conn, &id) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("{{\"error\": \"{e}\"}}");
                        std::process::exit(1);
                    }
                };

                let cfg_dir = planeai::config::config_dir("planeai");
                let (cfg, _) = planeai::config::load(&cfg_dir);
                let cleanup_ops = planeai::cleanup::real_ops();

                match planeai::session_ops::destroy(&conn, &session.id, &Some(cfg), &cleanup_ops) {
                    Ok(result) => {
                        notify_session_changed(&result.session.id);
                        let output = serde_json::to_string(&result.session).unwrap();
                        if pretty {
                            let v: serde_json::Value = serde_json::from_str(&output).unwrap();
                            println!("{}", serde_json::to_string_pretty(&v).unwrap());
                        } else {
                            println!("{output}");
                        }
                        if !result.cleanup_errors.is_empty() {
                            for e in &result.cleanup_errors {
                                eprintln!("{{\"warning\": \"{e}\"}}");
                            }
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("{{\"error\": \"{e}\"}}");
                        std::process::exit(1);
                    }
                }
            }
            SessionAction::Archive { id, pretty } => {
                let session = match planeai::session_ops::resolve_session_by_prefix(&conn, &id) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("{{\"error\": \"{e}\"}}");
                        std::process::exit(1);
                    }
                };

                let cfg_dir = planeai::config::config_dir("planeai");
                let (cfg, _) = planeai::config::load(&cfg_dir);

                match planeai::session_ops::archive(&conn, &session.id, &Some(cfg)) {
                    Ok(session) => {
                        notify_session_changed(&session.id);
                        let output = serde_json::to_string(&session).unwrap();
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
            SessionAction::Prompt { id, text, pretty } => {
                let prompt_text = match text {
                    Some(t) => t,
                    None => {
                        use std::io::Read;
                        let mut buf = String::new();
                        std::io::stdin()
                            .read_to_string(&mut buf)
                            .unwrap_or_else(|e| {
                                eprintln!("{{\"error\": \"failed to read stdin: {e}\"}}");
                                std::process::exit(1);
                            });
                        buf
                    }
                };

                let ops =
                    planeai::session_ops::real_prompt_ops(planeai::paths::notify_socket_path());
                match planeai::session_ops::send_prompt(&conn, &id, &prompt_text, &ops) {
                    Ok(result) => {
                        let output = serde_json::json!({
                            "status": "sent",
                            "session_id": result.session_id,
                            "backend": result.backend,
                        });
                        if pretty {
                            println!("{}", serde_json::to_string_pretty(&output).unwrap());
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
        Commands::Symphony { action } => match action {
            SymphonyAction::Status => match symphony_command("status") {
                Ok(response) => println!("{response}"),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            },
            SymphonyAction::Stop => match symphony_command("stop") {
                Ok(_) => println!("{{\"status\": \"stopped\"}}"),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            },
        },
    }
}

fn notify_session_changed(session_id: &str) {
    use planeai::ipc::{self, Channel};
    let app_dir = planeai::paths::app_data_dir();
    if ipc::channel_exists(Channel::Notify, &app_dir) {
        if let Ok(mut stream) = ipc::connect(Channel::Notify, &app_dir) {
            use std::io::Write;
            let msg =
                format!("{{\"event\":\"session_changed\",\"session_id\":\"{session_id}\"}}\n");
            let _ = stream.write_all(msg.as_bytes());
        }
    }
}

fn symphony_command(cmd: &str) -> Result<String, String> {
    use planeai::ipc::{self, Channel};
    use std::io::{BufRead, BufReader, Write};

    let app_dir = planeai::paths::app_data_dir();
    if !ipc::channel_exists(Channel::Symphony, &app_dir) {
        return Err("{\"error\": \"orchestrator is not running\"}".to_string());
    }
    let mut stream = ipc::connect(Channel::Symphony, &app_dir)
        .map_err(|e| format!("{{\"error\": \"cannot connect to orchestrator: {e}\"}}"))?;
    stream
        .write_all(format!("{cmd}\n").as_bytes())
        .map_err(|e| format!("{{\"error\": \"send failed: {e}\"}}"))?;

    if cmd == "stop" {
        return Ok(String::new());
    }

    let reader = BufReader::new(stream);
    let mut response = String::new();
    if let Some(line) = reader.lines().next() {
        match line {
            Ok(l) => response.push_str(&l),
            Err(e) => return Err(format!("{{\"error\": \"read failed: {e}\"}}")),
        }
    }
    Ok(response)
}
