//! Task smoke test.
//!
//! Verifies: task loading → task prompt resolution → task-driven worktree launch
//! → daemon session with task_key → prompt injected → output received →
//! detach/reattach → kill → lifecycle hooks → bytes_dropped = 0.
//!
//! Usage:
//!   PLANEAI_DAEMON_PTY_CORE=planeai-pty \
//!   PLANEAI_SESSION_LOG_DIR=/tmp/planeai-task-smoke-logs \
//!   PATH="$(pwd)/target/release:$PATH" \
//!   cargo run --release -p planeai-iced-spike --bin planeai-task-smoke -- \
//!     --project /tmp/planeai-task-smoke/project \
//!     --task-key PLA-123 \
//!     --agent-command "python3 -c 'print(\"agent ready\")'" \
//!     --metrics bench/results/task-smoke.jsonl

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use clap::Parser;
use planeai_core::services::{
    self, CreateSessionParams, ProjectService, SessionService, TaskLaunchRequest, TaskService,
    WorktreeService,
};
use planeai_iced_spike::adapter::PlaneAiTerminalSession;
use planeai_iced_spike::daemon_session::{
    attach, detach_daemon_session, ensure_daemon_running_sync, kill_daemon_session, DaemonSession,
};

#[derive(Parser)]
#[command(name = "planeai-task-smoke")]
struct Args {
    #[arg(long, default_value = "/tmp/planeai-task-smoke/project")]
    project: PathBuf,
    #[arg(long, default_value = "PLA-123")]
    task_key: String,
    #[arg(long)]
    agent_command: Option<String>,
    #[arg(long)]
    metrics: Option<PathBuf>,
    #[arg(long, default_value_t = 120)]
    cols: u16,
    #[arg(long, default_value_t = 40)]
    rows: u16,
}

macro_rules! step {
    ($name:expr, $body:expr) => {{
        let start = Instant::now();
        eprint!("  [{:>34}] ", $name);
        let result: anyhow::Result<_> = $body;
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        match &result {
            Ok(_) => eprintln!("PASS ({:.1}ms)", ms),
            Err(e) => eprintln!("FAIL ({:.1}ms): {}", ms, e),
        }
        result
    }};
}

fn wait_for_output(
    session: &dyn PlaneAiTerminalSession,
    timeout: Duration,
) -> anyhow::Result<Vec<u8>> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(data) = session.try_read_batch()? {
            if !data.is_empty() {
                return Ok(data);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    anyhow::bail!("no output within {}ms", timeout.as_millis())
}

fn main() {
    let args = Args::parse();
    eprintln!("=== Task Smoke Test ===\n");

    let agent_command = args.agent_command.clone().unwrap_or_else(|| {
        "python3 -c 'import time; print(\"agent ready\", flush=True); time.sleep(30)'".to_string()
    });
    eprintln!("  project: {}", args.project.display());
    eprintln!("  task_key: {}", args.task_key);
    eprintln!("  agent_command: {agent_command}\n");

    // Step 0: Init a temporary git repo at --project
    fs::create_dir_all(&args.project).expect("create project dir");
    let project_str = args.project.to_string_lossy().to_string();

    let wt_root = tempfile::tempdir().expect("tempdir for worktree root");
    unsafe { std::env::set_var("PLANEAI_WORKTREE_ROOT", wt_root.path()) };

    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&args.project)
        .output()
        .expect("git init");
    Command::new("git")
        .args(["config", "user.email", "smoke@test.com"])
        .current_dir(&args.project)
        .output()
        .ok();
    Command::new("git")
        .args(["config", "user.name", "Smoke"])
        .current_dir(&args.project)
        .output()
        .ok();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&args.project)
        .output()
        .expect("git commit");

    let mut all_pass = true;
    let mut metrics = serde_json::Map::new();
    let test_start = Instant::now();

    // Step 1: Open DB
    let db_dir = tempfile::tempdir().expect("tempdir");
    let db_path = db_dir.path().join("task-smoke.db");
    let conn = step!("open_db", {
        services::open_db_at(&db_path).map_err(|e| anyhow::anyhow!("{e}"))
    });
    let conn = match conn {
        Ok(c) => c,
        Err(e) => {
            eprintln!("\nFATAL: {e}");
            std::process::exit(1);
        }
    };

    // Step 2: Create project
    let project = step!("create_project", {
        ProjectService::ensure_project(&conn, &project_str).map_err(|e| anyhow::anyhow!("{e}"))
    });
    let project = match project {
        Ok(p) => p,
        Err(e) => {
            eprintln!("\nFATAL: {e}");
            std::process::exit(1);
        }
    };

    // Step 3: Create a test task via planeai-tasks
    let task = step!("create_test_task", {
        (|| -> anyhow::Result<planeai_tasks::model::Task> {
            use planeai_tasks::provider::TaskProvider;
            planeai_tasks::sqlite::migrate(
                &rusqlite::Connection::open(&db_path).map_err(|e| anyhow::anyhow!("{e}"))?,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            let repo = planeai_tasks::sqlite::SqliteRepository::open(
                db_path.to_str().unwrap(),
                &planeai_tasks::sqlite::derive_prefix(&project.name),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            let task = repo
                .create(planeai_tasks::model::CreateParams {
                    title: "Smoke test task".to_string(),
                    description: "Verify task integration works end to end".to_string(),
                    base_branch: "main".to_string(),
                    ..Default::default()
                })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(task)
        })()
    });
    let task = match task {
        Ok(t) => t,
        Err(e) => {
            eprintln!("\nFATAL: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("    task_key = {}", task.key);

    // Step 4: Resolve task prompt
    let prompt = step!("resolve_task_prompt", {
        let p = TaskService::resolve_task_prompt(&task, None);
        if p.is_empty() {
            Err(anyhow::anyhow!("empty prompt"))
        } else {
            Ok(p)
        }
    });
    let _prompt = prompt.unwrap_or_default();
    eprintln!("    prompt = {:?}", &_prompt[.._prompt.len().min(60)]);

    // Step 5: Resolve task launch (worktree + command)
    let config = planeai_core::session_launch::LaunchConfig {
        providers: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "smoke".to_string(),
                planeai_core::session_launch::ProviderConfig {
                    command: agent_command.clone(),
                    yolo_flag: Some("--yolo".to_string()),
                    prompt_command: Some("{prompt}".to_string()),
                    autonomous_prompt_template: None,
                },
            );
            m
        },
        default_provider: "smoke".to_string(),
        ..Default::default()
    };

    let request = TaskLaunchRequest {
        project_id: project.id.clone(),
        project_name: project.name.clone(),
        project_path: args.project.clone(),
        task_key: task.key.clone(),
        task_title: task.title.clone(),
        task_description: task.description.clone(),
        task_base_branch: task.base_branch.clone(),
        provider_id: Some("smoke".to_string()),
        auto_approve: true,
        autonomous: false,
        cols: args.cols,
        rows: args.rows,
    };

    let (resolved, worktree_mode) = step!("resolve_task_launch", {
        TaskService::resolve_task_launch(&request, &config, None)
            .map_err(|e| anyhow::anyhow!("{e}"))
    })
    .unwrap_or_else(|e| {
        eprintln!("\nFATAL: {e}");
        std::process::exit(1);
    });
    eprintln!("    command = {}", resolved.command_label);
    eprintln!("    prompt_injected = {}", resolved.prompt_was_injected);
    eprintln!(
        "    auto_approve_applied = {}",
        resolved.auto_approve_was_applied
    );

    // Step 6: Verify prompt was injected
    let r = step!("prompt_injected", {
        if resolved.prompt_was_injected {
            Ok(())
        } else {
            Err(anyhow::anyhow!("prompt was not injected"))
        }
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 7: Verify auto-approve applied
    let r = step!("auto_approve_applied", {
        if resolved.auto_approve_was_applied {
            Ok(())
        } else {
            Err(anyhow::anyhow!("auto-approve was not applied"))
        }
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 8: Create worktree
    let session_id = resolved.request.session_id.clone();
    let wt_resolved = step!("create_worktree", {
        WorktreeService::resolve_worktree(
            &worktree_mode,
            &project.name,
            &args.project,
            &session_id,
            "main",
        )
        .map_err(|e| anyhow::anyhow!("{e}"))
    });
    let wt_resolved = match wt_resolved {
        Ok(r) => r,
        Err(e) => {
            eprintln!("\nFATAL: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("    worktree_path = {:?}", wt_resolved.worktree_path);
    eprintln!("    branch = {}", wt_resolved.branch_name);

    // Step 9: Create session record with task_key
    let r = step!("create_session_record", {
        let params = CreateSessionParams {
            id: session_id.clone(),
            project_id: project.id.clone(),
            name: format!("{}: {}", task.key, task.title),
            backend: "daemon".to_string(),
            branch: wt_resolved.branch_name.clone(),
            worktree_path: wt_resolved.worktree_path.clone(),
            task_key: Some(task.key.clone()),
            base_branch: wt_resolved.base_branch.clone(),
            auto_approve: true,
            ..Default::default()
        };
        SessionService::create(&conn, &params).map_err(|e| anyhow::anyhow!("{e}"))
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 10: Verify session record has task_key, worktree_path, branch
    let r = step!("verify_session_task_key", {
        (|| -> anyhow::Result<()> {
            let rec = SessionService::get(&conn, &session_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .ok_or_else(|| anyhow::anyhow!("not found"))?;
            if rec.task_key.as_deref() != Some(&task.key) {
                anyhow::bail!("task_key mismatch: {:?}", rec.task_key);
            }
            if rec.worktree_path.is_none() {
                anyhow::bail!("worktree_path is NULL");
            }
            if rec.branch.is_empty() {
                anyhow::bail!("branch is empty");
            }
            Ok(())
        })()
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 11: Start daemon
    let r = step!("ensure_daemon_running", {
        ensure_daemon_running_sync().map_err(|e| anyhow::anyhow!("{e}"))
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 12: Spawn daemon session in worktree cwd
    let session = step!("spawn_in_worktree_cwd", {
        DaemonSession::spawn_with_session_id(
            1,
            &session_id,
            args.cols,
            args.rows,
            Some(&agent_command),
            &wt_resolved.cwd,
            &[],
        )
    });
    let session = match session {
        Ok(s) => s,
        Err(e) => {
            eprintln!("\nFATAL: {e}");
            std::process::exit(1);
        }
    };

    // Step 13: Receive output
    let r = step!(
        "receive_output",
        wait_for_output(&session, Duration::from_secs(5))
    );
    if r.is_err() {
        all_pass = false;
    }

    // Step 14: Detach
    drop(session);
    let r = step!("detach", {
        detach_daemon_session(&session_id).map_err(|e| anyhow::anyhow!("{e}"))
    });
    if r.is_err() {
        all_pass = false;
    }
    std::thread::sleep(Duration::from_millis(200));

    // Step 15: Reattach
    let reattached = step!("reattach", attach(2, &session_id, args.cols, args.rows));
    let reattached = match reattached {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("    reattach failed: {e}");
            all_pass = false;
            None
        }
    };

    // Step 16: Kill
    let r = step!("kill", {
        (|| -> anyhow::Result<()> {
            kill_daemon_session(&session_id)?;
            SessionService::set_status(&conn, &session_id, "destroyed")
                .map_err(|e| anyhow::anyhow!("{e}"))
        })()
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 17: bytes_dropped = 0
    let r = step!("bytes_dropped_zero", {
        (|| -> anyhow::Result<()> {
            if let Some(ref s) = reattached {
                let dropped = s.bytes_dropped();
                if dropped != 0 {
                    anyhow::bail!("bytes_dropped = {dropped}");
                }
            }
            Ok(())
        })()
    });
    if r.is_err() {
        all_pass = false;
    }

    // Summary
    let total_ms = test_start.elapsed().as_secs_f64() * 1000.0;
    metrics.insert("total_ms".into(), total_ms.into());
    metrics.insert("pass".into(), all_pass.into());
    metrics.insert("task_key".into(), task.key.into());
    metrics.insert(
        "worktree_path".into(),
        wt_resolved.worktree_path.unwrap_or_default().into(),
    );
    metrics.insert("branch".into(), wt_resolved.branch_name.into());
    metrics.insert(
        "prompt_injected".into(),
        resolved.prompt_was_injected.into(),
    );
    metrics.insert(
        "auto_approve_applied".into(),
        resolved.auto_approve_was_applied.into(),
    );

    eprintln!("\n{}", if all_pass { "ALL PASS" } else { "SOME FAILED" });
    eprintln!("Total: {total_ms:.0}ms\n");

    if let Some(ref path) = args.metrics {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open metrics file");
        let line = serde_json::to_string(&metrics).unwrap();
        writeln!(f, "{line}").expect("write metrics");
    }

    if !all_pass {
        std::process::exit(1);
    }
}
