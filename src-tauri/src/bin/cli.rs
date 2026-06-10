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
        },
        Commands::Symphony { action } => {
            let socket_path = planeai::paths::app_data_dir().join("symphony.sock");
            match action {
                SymphonyAction::Status => match symphony_command(&socket_path, "status") {
                    Ok(response) => println!("{response}"),
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                },
                SymphonyAction::Stop => match symphony_command(&socket_path, "stop") {
                    Ok(_) => println!("{{\"status\": \"stopped\"}}"),
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                },
            }
        }
    }
}

fn symphony_command(socket_path: &std::path::Path, cmd: &str) -> Result<String, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    if !socket_path.exists() {
        return Err("{\"error\": \"orchestrator is not running\"}".to_string());
    }
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("{{\"error\": \"cannot connect to orchestrator: {e}\"}}"))?;
    stream
        .write_all(format!("{cmd}\n").as_bytes())
        .map_err(|e| format!("{{\"error\": \"send failed: {e}\"}}"))?;

    if cmd == "stop" {
        return Ok(String::new());
    }

    // Read response
    let reader = BufReader::new(stream);
    let mut response = String::new();
    if let Some(line) = reader.lines().next() {
        match line {
            Ok(l) => {
                response.push_str(&l);
            }
            Err(e) => return Err(format!("{{\"error\": \"read failed: {e}\"}}")),
        }
    }
    Ok(response)
}
