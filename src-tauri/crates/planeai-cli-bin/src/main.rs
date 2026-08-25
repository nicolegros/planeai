mod plugin_test;

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
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    Symphony {
        #[command(subcommand)]
        action: SymphonyAction,
    },
    /// Validate a local plugin package without launching the desktop application
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Agent eXperience Interface — TOON output for autonomous agents
    Axi {
        #[command(subcommand)]
        action: Option<AxiAction>,
    },
}

#[derive(Subcommand)]
enum PluginAction {
    /// Run the headless JSONL protocol checks for a local plugin package
    Test {
        /// Path to the directory containing planeai-plugin.json
        #[arg(long)]
        package: std::path::PathBuf,
        /// JSONL scenario of plugin RPC requests to run after handshake
        #[arg(long)]
        scenario: Option<std::path::PathBuf>,
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
        /// Parent session ID (for orchestration tracking)
        #[arg(long)]
        parent: Option<String>,
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
    /// List direct child sessions of a parent
    Children {
        id: String,
        #[arg(long)]
        pretty: bool,
    },
    /// Show the full session tree (walks up to root, then shows all descendants)
    Tree {
        id: String,
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

#[derive(Subcommand)]
enum AxiAction {
    /// Task operations
    Task {
        #[command(subcommand)]
        action: AxiTaskAction,
    },
    /// Session operations
    Session {
        #[command(subcommand)]
        action: AxiSessionAction,
    },
    /// Project operations
    Project {
        #[command(subcommand)]
        action: AxiProjectAction,
    },
    /// Loop operations — create and inspect durable loop runs
    Loop {
        #[command(subcommand)]
        action: AxiLoopAction,
    },
}

#[derive(Subcommand)]
enum AxiLoopAction {
    /// Create a new loop run
    Create {
        /// Goal description for the loop (deprecated: use --input goal="...")
        #[arg(long, default_value = "")]
        goal: String,
        /// Strategy identifier (e.g., maker-verifier). Alias for --recipe.
        #[arg(long, default_value = "maker-verifier")]
        strategy: String,
        /// Recipe ID or path (takes precedence over --strategy)
        #[arg(long)]
        recipe: Option<String>,
        /// Maximum rounds before the loop stops
        #[arg(long, default_value_t = 3)]
        max_rounds: i64,
        /// Task key to associate with this loop (deprecated: use --input task_key="...")
        #[arg(long)]
        task: Option<String>,
        /// Project name (otherwise resolved from CWD)
        #[arg(long)]
        project: Option<String>,
        /// Start the loop immediately (status = running instead of draft)
        #[arg(long)]
        start: bool,
        /// Base branch for worktree isolation (deprecated: use --input base_branch="...")
        #[arg(long, hide = true)]
        base_branch: Option<String>,
        /// Comma-separated provider names for n-candidates-arbiter strategy
        /// (shorthand for --input providers="claude,codex,kiro")
        #[arg(long, value_delimiter = ',')]
        providers: Vec<String>,
        /// Provider for the arbiter session in n-candidates-arbiter strategy
        /// (shorthand for --input arbiter_provider="copilot")
        #[arg(long)]
        arbiter_provider: Option<String>,
        /// Set an input key=value pair (repeatable, e.g. --input goal="Fix bug" --input task_key=BUG-1)
        #[arg(long = "input", value_name = "KEY=VALUE")]
        input_kv: Vec<String>,
        /// Set all inputs as a JSON object (e.g. --inputs '{"goal":"Fix bug","task_key":"BUG-1"}')
        #[arg(long = "inputs", value_name = "JSON")]
        inputs_json: Option<String>,
    },
    /// Recipe operations — list, inspect, and validate loop recipes
    Recipe {
        #[command(subcommand)]
        action: AxiLoopRecipeAction,
    },
    /// Observe loop state: summary, sessions, events, artifacts
    Observe {
        /// Loop ID (prefix match supported)
        id: String,
        /// Maximum number of recent events to show
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Advance the loop by one tick (append event, return state)
    Tick {
        /// Loop ID (prefix match supported)
        id: String,
    },
    /// Stop a loop (mark cancelled)
    Stop {
        /// Loop ID (prefix match supported)
        id: String,
    },
    /// Show loop-owned sessions with recursive children
    Tree {
        /// Loop ID (prefix match supported)
        id: String,
    },
    /// Structured handoff operations
    Handoff {
        #[command(subcommand)]
        action: AxiLoopHandoffAction,
    },
    /// Run a verifier gate command and persist the result
    Verify {
        /// Loop ID (prefix match supported)
        #[arg(long, alias = "loop")]
        loop_id: String,
        /// Session ID (prefix match supported)
        #[arg(long)]
        session: String,
        /// Verifier name (e.g., "rust-tests", "eslint")
        #[arg(long)]
        name: String,
        /// Command to execute (run in shell). This is a trusted human/recipe-authored
        /// command — do not pass agent-generated strings.
        #[arg(long)]
        command: String,
        /// Timeout in milliseconds (default: 600000 = 10 min). Use 0 for no timeout.
        #[arg(long, default_value_t = 600_000)]
        timeout_ms: u64,
        /// Maximum output bytes to capture (default: 10MB). Larger output is truncated.
        #[arg(long, default_value_t = 10_485_760)]
        max_output_bytes: usize,
    },
}

#[derive(Subcommand)]
enum AxiLoopHandoffAction {
    /// Print the expected handoff file path for a session
    Path {
        /// Loop ID (prefix match supported)
        #[arg(long, alias = "loop")]
        loop_id: String,
        /// Session ID (prefix match supported)
        #[arg(long)]
        session: String,
    },
    /// Record a structured handoff from a file
    Record {
        /// Loop ID (prefix match supported)
        #[arg(long, alias = "loop")]
        loop_id: String,
        /// Session ID (prefix match supported)
        #[arg(long)]
        session: String,
        /// Path to the handoff JSON file
        #[arg(long)]
        path: std::path::PathBuf,
    },
}

#[derive(Subcommand)]
enum AxiLoopRecipeAction {
    /// List all discovered recipes
    #[command(name = "ls")]
    List,
    /// Show recipe details
    Show {
        /// Recipe ID or path to a recipe YAML file
        id: String,
    },
    /// Validate a recipe
    Validate {
        /// Recipe ID or path to a recipe YAML file
        id: String,
    },
}

#[derive(Subcommand)]
enum AxiTaskAction {
    /// List tasks
    #[command(name = "ls")]
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        #[arg(long)]
        project: Option<String>,
    },
    /// Show task details
    Show {
        key: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Create a new task
    Add {
        title: String,
        #[arg(long, default_value = "")]
        desc: String,
        #[arg(long, default_value_t = 0)]
        priority: i32,
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        blocked_by: Vec<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        base_branch: Option<String>,
        #[arg(long)]
        project: Option<String>,
    },
    /// Move a task to a new status
    Move {
        key: String,
        status: String,
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Subcommand)]
enum AxiSessionAction {
    /// List sessions
    #[command(name = "ls")]
    List {
        #[arg(long)]
        archived: bool,
    },
    /// List direct child sessions of a parent
    Children { id: String },
    /// Show the full session tree (walks up to root, then shows all descendants)
    Tree { id: String },
    /// Create a new session (auto-sets parent from $PLANEAI_SESSION_ID)
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
    },
    /// Send a prompt to a running session
    Prompt { id: String, text: Option<String> },
    /// Read session output (last N lines, ANSI stripped)
    Read {
        id: String,
        #[arg(long, default_value = "100", conflicts_with = "after")]
        lines: usize,
        /// Opaque cursor from a previous read. Returns only output since that cursor.
        #[arg(long)]
        after: Option<String>,
        /// Maximum bytes to return (0 = unlimited). Only used with --after.
        #[arg(long, default_value = "0")]
        max_bytes: usize,
    },
}

#[derive(Subcommand)]
enum AxiProjectAction {
    /// List projects
    #[command(name = "ls")]
    List,
}

#[derive(Subcommand)]
enum TaskAction {
    /// Create a new task
    Add {
        title: String,
        #[arg(long, default_value = "")]
        desc: String,
        #[arg(long, default_value_t = 0)]
        priority: i32,
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        blocked_by: Vec<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        base_branch: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        pretty: bool,
    },
    /// Show a task by key
    Show {
        key: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        pretty: bool,
    },
    /// List tasks
    #[command(name = "ls")]
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        pretty: bool,
    },
    /// Move a task to a new status
    Move {
        key: String,
        status: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        pretty: bool,
    },
    /// Edit an existing task
    Edit {
        key: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        desc: Option<String>,
        #[arg(long)]
        priority: Option<i32>,
        #[arg(long, value_delimiter = ',')]
        tags: Option<Vec<String>>,
        #[arg(long, value_delimiter = ',')]
        blocked_by: Option<Vec<String>>,
        /// Set parent task key (use empty string to clear)
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        base_branch: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        pretty: bool,
    },
    /// Delete a task
    Delete {
        key: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        pretty: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    // Plugin tests are intentionally handled before application logging and the
    // database are initialized, so this command stays completely headless.
    let command = match cli.command {
        Commands::Plugin { action } => {
            let result = match action {
                PluginAction::Test { package, scenario } => {
                    plugin_test::run(&package, scenario.as_deref())
                }
            };
            if let Err(error) = result {
                eprintln!("plugin test failed: {error:#}");
                std::process::exit(1);
            }
            return;
        }
        command => command,
    };

    let log_dir = planeai_paths::app_data_dir().join("logs");
    let _guard = planeai::logging::init(&log_dir);

    let db_path = planeai_paths::db_path();
    let conn = rusqlite::Connection::open(&db_path).unwrap_or_else(|e| {
        eprintln!("{{\"error\": \"failed to open database: {e}\"}}");
        std::process::exit(1);
    });

    match command {
        Commands::Plugin { .. } => {
            unreachable!("plugin commands return before application initialization")
        }
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
                parent,
                pretty,
            } => {
                let parent_session_id = parent.or_else(|| std::env::var("PLANEAI_SESSION_ID").ok());

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
                    parent_session_id,
                };

                match planeai::cli::create_session(&conn, opts) {
                    Ok(session) => {
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
            SessionAction::Children { id, pretty } => {
                let session = match planeai::session_ops::resolve_session_by_prefix(&conn, &id) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("{{\"error\": \"{e}\"}}");
                        std::process::exit(1);
                    }
                };

                match planeai_core::services::SessionService::children(&conn, &session.id) {
                    Ok(children) => {
                        let output = serde_json::to_string(&children).unwrap();
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
            SessionAction::Tree { id, pretty } => {
                let session = match planeai::session_ops::resolve_session_by_prefix(&conn, &id) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("{{\"error\": \"{e}\"}}");
                        std::process::exit(1);
                    }
                };

                match planeai_core::services::SessionService::tree(&conn, &session.id) {
                    Ok(tree) => {
                        let output = serde_json::to_string(&tree).unwrap();
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

                match planeai::session_ops::archive(
                    &conn,
                    &session.id,
                    &Some(cfg),
                    &planeai::cleanup::real_kill_ops(),
                ) {
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
                    planeai::session_ops::real_prompt_ops(planeai_paths::notify_socket_path());
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
        Commands::Task { action } => {
            let cwd = std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let project_flag = match &action {
                TaskAction::Add { project, .. }
                | TaskAction::Show { project, .. }
                | TaskAction::List { project, .. }
                | TaskAction::Move { project, .. }
                | TaskAction::Edit { project, .. }
                | TaskAction::Delete { project, .. } => project.as_deref(),
            };

            let prefix = match planeai::task_cli::resolve_prefix(&conn, project_flag, &cwd) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{{\"error\": \"{e}\"}}");
                    std::process::exit(1);
                }
            };

            let repo = match planeai_tasks::sqlite::SqliteRepository::open(
                db_path.to_str().unwrap(),
                &prefix,
            ) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("{{\"error\": \"{e}\"}}");
                    std::process::exit(1);
                }
            };

            let lifecycle_context = planeai::task_cli::TaskLifecycleContext {
                origin: planeai_core::task_lifecycle::TaskLifecycleOrigin::Cli,
                project_id: conn
                    .query_row(
                        "SELECT id FROM projects WHERE prefix = ?1",
                        [&prefix],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())
                    .unwrap_or_default(),
                project_prefix: prefix.clone(),
            };

            let (result, pretty, key_for_notify) = match action {
                TaskAction::Add {
                    title,
                    desc,
                    priority,
                    tags,
                    blocked_by,
                    parent,
                    base_branch,
                    pretty,
                    ..
                } => {
                    let r = planeai::task_cli::run_task_add_with_lifecycle(
                        &repo,
                        planeai::task_cli::AddParams {
                            title: &title,
                            description: &desc,
                            priority,
                            tags: &tags,
                            blocked_by: &blocked_by,
                            parent: parent.as_deref(),
                            base_branch: base_branch.as_deref(),
                        },
                        &lifecycle_context,
                    );
                    let key = r.as_ref().ok().and_then(|json| {
                        serde_json::from_str::<serde_json::Value>(json)
                            .ok()?
                            .get("key")?
                            .as_str()
                            .map(|s| s.to_string())
                    });
                    (r, pretty, key)
                }
                TaskAction::Show { key, pretty, .. } => {
                    (planeai::task_cli::run_task_show(&repo, &key), pretty, None)
                }
                TaskAction::List {
                    status,
                    tags,
                    pretty,
                    ..
                } => (
                    planeai::task_cli::run_task_list(&repo, status.as_deref(), &tags),
                    pretty,
                    None,
                ),
                TaskAction::Move {
                    key,
                    status,
                    pretty,
                    ..
                } => {
                    let r = planeai::task_cli::run_task_move_with_lifecycle(
                        &repo,
                        &key,
                        &status,
                        &lifecycle_context,
                    );
                    (r, pretty, Some(key))
                }
                TaskAction::Edit {
                    key,
                    title,
                    desc,
                    priority,
                    tags,
                    blocked_by,
                    parent,
                    base_branch,
                    pretty,
                    ..
                } => {
                    let parent_opt = parent.map(|s| if s.is_empty() { None } else { Some(s) });
                    let parent_ref = parent_opt.as_ref().map(|o| o.as_deref());
                    let r = planeai::task_cli::run_task_edit_with_lifecycle(
                        &repo,
                        planeai::task_cli::EditParams {
                            key: &key,
                            title: title.as_deref(),
                            description: desc.as_deref(),
                            priority,
                            tags: tags.as_deref(),
                            blocked_by: blocked_by.as_deref(),
                            parent: parent_ref,
                            base_branch: base_branch.as_deref(),
                        },
                        &lifecycle_context,
                    );
                    (r, pretty, Some(key))
                }
                TaskAction::Delete { key, pretty, .. } => {
                    let r = planeai::task_cli::run_task_delete(&repo, &key);
                    (r, pretty, Some(key))
                }
            };

            match result {
                Ok(output) => {
                    if let Some(key) = &key_for_notify {
                        planeai::task_cli::notify_task_changed(key);
                    }
                    if pretty {
                        let v: serde_json::Value = serde_json::from_str(&output)
                            .unwrap_or(serde_json::Value::String(output.clone()));
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
        Commands::Axi { action } => {
            let exit_code = run_axi(&conn, action);
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        }
    }
}

fn run_axi(conn: &rusqlite::Connection, action: Option<AxiAction>) -> i32 {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let bin_path = collapse_home(&executable_path());

    match action {
        None => {
            // Home view
            let (output, code) = planeai::axi::home(conn, &cwd, &bin_path);
            print!("{output}");
            code
        }
        Some(AxiAction::Task { action }) => run_axi_task(conn, action, &cwd),
        Some(AxiAction::Session { action }) => run_axi_session(conn, action),
        Some(AxiAction::Project { action }) => run_axi_project(conn, action),
        Some(AxiAction::Loop { action }) => run_axi_loop(conn, action, &cwd),
    }
}

fn run_axi_task(conn: &rusqlite::Connection, action: AxiTaskAction, cwd: &str) -> i32 {
    let db_path = planeai_paths::db_path();

    let project_flag = match &action {
        AxiTaskAction::List { project, .. }
        | AxiTaskAction::Show { project, .. }
        | AxiTaskAction::Add { project, .. }
        | AxiTaskAction::Move { project, .. } => project.as_deref(),
    };

    let prefix = match planeai::task_cli::resolve_prefix(conn, project_flag, cwd) {
        Ok(p) => p,
        Err(e) => {
            let output = planeai_toon::render(&[
                planeai_toon::field("error", planeai_toon::str_val(&e)),
                planeai_toon::field(
                    "help",
                    planeai_toon::Value::List(vec![
                        "Run `planeai-cli axi project ls` to see projects".into(),
                    ]),
                ),
            ]);
            print!("{output}");
            return 1;
        }
    };

    let repo =
        match planeai_tasks::sqlite::SqliteRepository::open(db_path.to_str().unwrap(), &prefix) {
            Ok(r) => r,
            Err(e) => {
                print!(
                    "{}",
                    planeai_toon::render(&[planeai_toon::field(
                        "error",
                        planeai_toon::str_val(&e.to_string())
                    )])
                );
                return 1;
            }
        };

    let lifecycle_context = planeai::task_cli::TaskLifecycleContext {
        origin: planeai_core::task_lifecycle::TaskLifecycleOrigin::Axi,
        project_id: conn
            .query_row(
                "SELECT id FROM projects WHERE prefix = ?1",
                [&prefix],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())
            .unwrap_or_default(),
        project_prefix: prefix.clone(),
    };

    let (output, code) = match action {
        AxiTaskAction::List { status, tags, .. } => {
            planeai::axi::task_ls(&repo, status.as_deref(), &tags)
        }
        AxiTaskAction::Show { key, .. } => planeai::axi::task_show(&repo, &key),
        AxiTaskAction::Add {
            title,
            desc,
            priority,
            tags,
            blocked_by,
            parent,
            base_branch,
            ..
        } => {
            let result = planeai::axi::task_add_with_lifecycle(
                &repo,
                planeai::task_cli::AddParams {
                    title: &title,
                    description: &desc,
                    priority,
                    tags: &tags,
                    blocked_by: &blocked_by,
                    parent: parent.as_deref(),
                    base_branch: base_branch.as_deref(),
                },
                &lifecycle_context,
            );
            if code_of(&result) == 0 {
                if let Some(key) = extract_key(&result.0) {
                    planeai::task_cli::notify_task_changed(&key);
                }
            }
            result
        }
        AxiTaskAction::Move { key, status, .. } => {
            let result =
                planeai::axi::task_move_with_lifecycle(&repo, &key, &status, &lifecycle_context);
            if code_of(&result) == 0 {
                planeai::task_cli::notify_task_changed(&key);
            }
            result
        }
    };
    print!("{output}");
    code
}

fn run_axi_session(conn: &rusqlite::Connection, action: AxiSessionAction) -> i32 {
    let (output, code) = match action {
        AxiSessionAction::List { archived } => planeai::axi::session_ls(conn, archived),
        AxiSessionAction::Children { id } => planeai::axi::session_children(conn, &id),
        AxiSessionAction::Tree { id } => planeai::axi::session_tree(conn, &id),
        AxiSessionAction::Create {
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
        } => {
            let parent_session_id = std::env::var("PLANEAI_SESSION_ID").ok();

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
                parent_session_id,
            };

            match planeai::cli::create_session(conn, opts) {
                Ok(session) => planeai::axi::session_create_output(&session),
                Err(e) => return emit_axi_error(&e),
            }
        }
        AxiSessionAction::Prompt { id, text } => {
            let prompt_text = match text {
                Some(t) => t,
                None => {
                    use std::io::Read;
                    let mut buf = String::new();
                    if std::io::stdin().read_to_string(&mut buf).is_err() {
                        return emit_axi_error("failed to read stdin");
                    }
                    buf
                }
            };
            let ops = planeai::session_ops::real_prompt_ops(planeai_paths::notify_socket_path());
            planeai::axi::session_prompt(conn, &id, &prompt_text, &ops)
        }
        AxiSessionAction::Read {
            id,
            lines,
            after,
            max_bytes,
        } => {
            let session = match planeai::session_ops::resolve_session_by_prefix(conn, &id) {
                Ok(s) => s,
                Err(e) => return emit_axi_error(&e.to_string()),
            };

            // If --after is provided, use cursor-based incremental read
            if let Some(cursor_str) = after {
                return match session.backend.as_str() {
                    "daemon" => {
                        // Parse daemon cursor: "daemon:<offset>"
                        let offset = match parse_daemon_cursor(&cursor_str) {
                            Ok(o) => o,
                            Err(e) => return emit_axi_error(&e),
                        };
                        match planeai::session_ops::read_daemon_buffer_after(
                            &session.id,
                            offset,
                            max_bytes,
                        ) {
                            Ok(result) => {
                                let cursor = format!("daemon:{}", result.cursor);
                                let (output, code) = planeai::axi::session_read_cursor_output(
                                    &session.id[..8],
                                    "daemon",
                                    &cursor,
                                    result.truncated,
                                    &result.text,
                                );
                                print!("{output}");
                                code
                            }
                            Err(e) => emit_axi_error(&e),
                        }
                    }
                    "tmux" => {
                        let tmux_name = match session.tmux_name.as_deref() {
                            Some(n) => n,
                            None => return emit_axi_error("tmux session has no tmux_name"),
                        };
                        // Validate cursor prefix
                        if !cursor_str.starts_with("tmux:") {
                            return emit_axi_error(&format!(
                                "invalid cursor for tmux backend: {cursor_str}"
                            ));
                        }
                        match planeai::session_ops::read_tmux_pane_after(
                            tmux_name,
                            &cursor_str,
                            max_bytes,
                        ) {
                            Ok(result) => {
                                let (output, code) = planeai::axi::session_read_cursor_output(
                                    &session.id[..8],
                                    "tmux",
                                    &result.cursor,
                                    result.truncated,
                                    &result.text,
                                );
                                print!("{output}");
                                code
                            }
                            Err(e) => emit_axi_error(&e),
                        }
                    }
                    "local" => emit_axi_error("local backend does not support remote read"),
                    other => emit_axi_error(&format!("unsupported backend: {other}")),
                };
            }

            // Legacy --lines mode (no cursor)
            match session.backend.as_str() {
                "daemon" => match planeai::session_ops::read_daemon_buffer(&session.id, lines) {
                    Ok(text) => planeai::axi::session_read_output(&session.id[..8], &text),
                    Err(e) => return emit_axi_error(&e),
                },
                "tmux" => {
                    let tmux_name = match session.tmux_name.as_deref() {
                        Some(n) => n,
                        None => return emit_axi_error("tmux session has no tmux_name"),
                    };
                    match planeai::session_ops::read_tmux_pane(tmux_name, lines) {
                        Ok(text) => planeai::axi::session_read_output(&session.id[..8], &text),
                        Err(e) => return emit_axi_error(&e),
                    }
                }
                "local" => return emit_axi_error("local backend does not support remote read"),
                other => return emit_axi_error(&format!("unsupported backend: {other}")),
            }
        }
    };
    print!("{output}");
    code
}

fn emit_axi_error(msg: &str) -> i32 {
    let output = planeai_toon::render(&[planeai_toon::field("error", planeai_toon::str_val(msg))]);
    print!("{output}");
    1
}

/// Parse a daemon cursor string "daemon:<offset>" into the byte offset.
fn parse_daemon_cursor(cursor: &str) -> Result<u64, String> {
    let parts: Vec<&str> = cursor.splitn(2, ':').collect();
    if parts.len() != 2 || parts[0] != "daemon" {
        return Err(format!("invalid cursor for daemon backend: {cursor}"));
    }
    parts[1]
        .parse::<u64>()
        .map_err(|_| format!("invalid cursor offset: {}", parts[1]))
}

fn run_axi_project(conn: &rusqlite::Connection, action: AxiProjectAction) -> i32 {
    let (output, code) = match action {
        AxiProjectAction::List => planeai::axi::project_ls(conn),
    };
    print!("{output}");
    code
}

/// Merge inputs from all CLI sources with precedence: JSON blob < key=value pairs < legacy flags.
///
/// Returns the merged input map on success, or `(error_output, exit_code)` on parse errors.
/// Emits a deprecation warning to stderr when legacy flags are used.
fn merge_cli_inputs(
    inputs_json: &Option<String>,
    input_kv: &[String],
    goal: String,
    task: &Option<String>,
    base_branch: &Option<String>,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, (String, i32)> {
    let mut merged: std::collections::BTreeMap<String, serde_json::Value> =
        std::collections::BTreeMap::new();

    // Layer 1: --inputs JSON (lowest precedence)
    if let Some(json_str) = inputs_json {
        match serde_json::from_str::<serde_json::Value>(json_str) {
            Ok(serde_json::Value::Object(map)) => {
                for (k, v) in map {
                    merged.insert(k, v);
                }
            }
            Ok(_) => {
                let output = planeai_toon::render(&[planeai_toon::field(
                    "error",
                    planeai_toon::str_val("--inputs must be a JSON object"),
                )]);
                return Err((output, 1));
            }
            Err(e) => {
                let output = planeai_toon::render(&[planeai_toon::field(
                    "error",
                    planeai_toon::str_val(&format!("--inputs: invalid JSON: {e}")),
                )]);
                return Err((output, 1));
            }
        }
    }

    // Layer 2: --input key=value pairs
    for kv in input_kv {
        if let Some((key, val)) = kv.split_once('=') {
            merged.insert(key.to_string(), serde_json::Value::String(val.to_string()));
        } else {
            let output = planeai_toon::render(&[planeai_toon::field(
                "error",
                planeai_toon::str_val(&format!("--input must be key=value, got: {kv}")),
            )]);
            return Err((output, 1));
        }
    }

    // Layer 3: Legacy flags (highest precedence for backwards compat)
    let legacy_flag_used = !goal.is_empty() || task.is_some() || base_branch.is_some();
    if !goal.is_empty() {
        merged.insert("goal".to_string(), serde_json::Value::String(goal));
    }
    if let Some(tk) = task {
        merged.insert(
            "task_key".to_string(),
            serde_json::Value::String(tk.clone()),
        );
    }
    if let Some(bb) = base_branch {
        merged.insert(
            "base_branch".to_string(),
            serde_json::Value::String(bb.clone()),
        );
    }

    if legacy_flag_used {
        eprintln!("warning: --goal, --task, and --base-branch are deprecated. Use --input key=value instead.");
    }

    Ok(merged)
}

fn run_axi_loop(conn: &rusqlite::Connection, action: AxiLoopAction, cwd: &str) -> i32 {
    let (output, code) = match action {
        AxiLoopAction::Create {
            goal,
            strategy,
            recipe,
            max_rounds,
            task,
            project,
            start,
            base_branch,
            providers,
            arbiter_provider,
            input_kv,
            inputs_json,
        } => {
            let mut merged =
                match merge_cli_inputs(&inputs_json, &input_kv, goal, &task, &base_branch) {
                    Ok(m) => m,
                    Err((output, code)) => {
                        return {
                            print!("{output}");
                            code
                        }
                    }
                };

            // Inject --providers as inputs.providers (comma-separated)
            if !providers.is_empty() && !merged.contains_key("providers") {
                merged.insert(
                    "providers".to_string(),
                    serde_json::Value::String(providers.join(",")),
                );
            }

            // Inject --arbiter-provider as inputs.arbiter_provider
            if let Some(ref ap) = arbiter_provider {
                if !merged.contains_key("arbiter_provider") {
                    merged.insert(
                        "arbiter_provider".to_string(),
                        serde_json::Value::String(ap.clone()),
                    );
                }
            }

            // Extract goal from merged inputs (required)
            let final_goal = match merged.get("goal").and_then(|v| v.as_str()) {
                Some(g) if !g.is_empty() => g.to_string(),
                _ => {
                    return emit_axi_error("goal is required: use --goal or --input goal=\"...\"");
                }
            };

            // Extract task_key from merged inputs
            let final_task = merged
                .get("task_key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // Pass the full merged inputs map to loop_create
            let inputs_value = if merged.is_empty() {
                None
            } else {
                Some(merged)
            };

            planeai::axi::loop_create(
                conn,
                cwd,
                project.as_deref(),
                final_task.as_deref(),
                &strategy,
                recipe.as_deref(),
                &final_goal,
                max_rounds,
                start,
                inputs_value,
            )
        }
        AxiLoopAction::Observe { id, limit } => planeai::axi::loop_observe(conn, &id, limit),
        AxiLoopAction::Tick { id } => planeai::axi::loop_tick(conn, &id),
        AxiLoopAction::Stop { id } => planeai::axi::loop_stop(conn, &id),
        AxiLoopAction::Tree { id } => planeai::axi::loop_tree(conn, &id),
        AxiLoopAction::Recipe { action } => match action {
            AxiLoopRecipeAction::List => planeai::axi::recipe_ls(cwd),
            AxiLoopRecipeAction::Show { id } => planeai::axi::recipe_show(&id, cwd),
            AxiLoopRecipeAction::Validate { id } => planeai::axi::recipe_validate(&id, cwd),
        },
        AxiLoopAction::Handoff { action } => match action {
            AxiLoopHandoffAction::Path { loop_id, session } => {
                planeai::axi::loop_handoff_path(conn, &loop_id, &session, cwd)
            }
            AxiLoopHandoffAction::Record {
                loop_id,
                session,
                path,
            } => planeai::axi::loop_handoff_record(conn, &loop_id, &session, &path, cwd),
        },
        AxiLoopAction::Verify {
            loop_id,
            session,
            name,
            command,
            timeout_ms,
            max_output_bytes,
        } => planeai::axi::loop_verify(
            conn,
            &loop_id,
            &session,
            &name,
            &command,
            timeout_ms,
            max_output_bytes,
        ),
    };
    print!("{output}");
    code
}

fn code_of(result: &(String, i32)) -> i32 {
    result.1
}

fn extract_key(toon_output: &str) -> Option<String> {
    for line in toon_output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("key: ") {
            return Some(rest.to_string());
        }
    }
    None
}

fn executable_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| std::env::args().next().unwrap_or_default())
}

fn collapse_home(path: &str) -> String {
    if let Some(home) = dirs_home() {
        if path.starts_with(&home) {
            return format!("~{}", &path[home.len()..]);
        }
    }
    path.to_string()
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
}

fn notify_session_changed(session_id: &str) {
    use planeai::ipc::{self, Channel};
    let app_dir = planeai_paths::app_data_dir();
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

    let app_dir = planeai_paths::app_data_dir();
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

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn plugin_test_scenario_help_describes_supported_jsonl_requests() {
        let mut command = Cli::command();
        let plugin = command.find_subcommand_mut("plugin").unwrap();
        let test = plugin.find_subcommand_mut("test").unwrap();
        let help = test.render_long_help().to_string();
        assert!(help.contains("JSONL scenario of plugin RPC requests to run after handshake"));
        assert!(!help.contains("Reserved for future scenario JSONL support"));
    }
}
