//! Worktree smoke test.
//!
//! Verifies: git repo init → worktree creation via shared logic → daemon session
//! spawned in worktree cwd → output received → session record with worktree_path
//! and branch_name → detach/reattach → kill → bytes_dropped = 0.
//!
//! Usage:
//!   PLANEAI_DAEMON_PTY_CORE=planeai-pty \
//!   PLANEAI_SESSION_LOG_DIR=/tmp/planeai-wt-smoke-logs \
//!   PATH="$(pwd)/target/release:$PATH" \
//!   cargo run --release -p planeai-iced-spike --bin planeai-worktree-smoke -- \
//!     --project /tmp/planeai-worktree-smoke/project \
//!     --branch planeai-smoke-branch \
//!     --agent-command "python3 -c 'print(\"agent ready\")'" \
//!     --metrics bench/results/worktree-smoke.jsonl

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use clap::Parser;
use planeai_core::services::{
    self, CreateSessionParams, ProjectService, SessionService, WorktreeMode, WorktreeService,
};
use planeai_iced_spike::adapter::PlaneAiTerminalSession;
use planeai_iced_spike::daemon_session::{
    attach, detach_daemon_session, ensure_daemon_running_sync, kill_daemon_session, DaemonSession,
};

#[derive(Parser)]
#[command(name = "planeai-worktree-smoke")]
struct Args {
    #[arg(long, default_value = "/tmp/planeai-worktree-smoke/project")]
    project: PathBuf,
    #[arg(long, default_value = "planeai-smoke-branch")]
    branch: String,
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
    eprintln!("=== Worktree Smoke Test ===\n");

    let agent_command = args.agent_command.clone().unwrap_or_else(|| {
        "python3 -c 'import time; print(\"agent ready\", flush=True); time.sleep(30)'".to_string()
    });
    eprintln!("  project: {}", args.project.display());
    eprintln!("  branch: {}", args.branch);
    eprintln!("  agent_command: {agent_command}\n");

    // Step 0: Init a temporary git repo at --project
    fs::create_dir_all(&args.project).expect("create project dir");
    let project_str = args.project.to_string_lossy().to_string();
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
    let db_path = db_dir.path().join("worktree-smoke.db");
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

    // Step 2: Resolve project
    let project = step!("resolve_project", {
        ProjectService::ensure_project(&conn, &project_str).map_err(|e| anyhow::anyhow!("{e}"))
    });
    let project = match project {
        Ok(p) => p,
        Err(e) => {
            eprintln!("\nFATAL: {e}");
            std::process::exit(1);
        }
    };

    // Step 3: Create worktree via shared logic
    let session_id = uuid::Uuid::new_v4().to_string();
    let mode = WorktreeMode::Create {
        base_project_path: args.project.clone(),
        branch_name: args.branch.clone(),
        task_key: None,
    };
    let resolved = step!("create_worktree", {
        WorktreeService::resolve_worktree(&mode, &project.name, &args.project, &session_id, "main")
            .map_err(|e| anyhow::anyhow!("{e}"))
    });
    let resolved = match resolved {
        Ok(r) => r,
        Err(e) => {
            eprintln!("\nFATAL: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("    worktree_path = {:?}", resolved.worktree_path);
    eprintln!("    branch = {}", resolved.branch_name);
    eprintln!("    cwd = {}", resolved.cwd.display());

    // Step 4: Verify worktree directory exists
    let r = step!("verify_worktree_dir", {
        (|| -> anyhow::Result<()> {
            if resolved.cwd.is_dir() {
                Ok(())
            } else {
                anyhow::bail!("worktree cwd does not exist: {}", resolved.cwd.display())
            }
        })()
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 5: Start daemon
    let r = step!("ensure_daemon_running", {
        ensure_daemon_running_sync().map_err(|e| anyhow::anyhow!("{e}"))
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 6: Create session record with worktree fields
    let r = step!("create_session_record", {
        let params = CreateSessionParams {
            id: session_id.clone(),
            project_id: project.id.clone(),
            name: "worktree-smoke".to_string(),
            backend: "daemon".to_string(),
            branch: resolved.branch_name.clone(),
            worktree_path: resolved.worktree_path.clone(),
            base_branch: resolved.base_branch.clone(),
            ..Default::default()
        };
        SessionService::create(&conn, &params).map_err(|e| anyhow::anyhow!("{e}"))
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 7: Verify session record has worktree_path and branch
    let r = step!("verify_session_record_fields", {
        (|| -> anyhow::Result<()> {
            let rec = SessionService::get(&conn, &session_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .ok_or_else(|| anyhow::anyhow!("not found"))?;
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

    // Step 8: Spawn daemon session in worktree cwd
    let session = step!("spawn_in_worktree_cwd", {
        DaemonSession::spawn_with_session_id(
            1,
            &session_id,
            args.cols,
            args.rows,
            Some(&agent_command),
            &resolved.cwd,
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

    // Step 9: Receive output
    let r = step!(
        "receive_output",
        wait_for_output(&session, Duration::from_secs(5))
    );
    if r.is_err() {
        all_pass = false;
    }

    // Step 10: Detach
    drop(session);
    let r = step!("detach", {
        detach_daemon_session(&session_id).map_err(|e| anyhow::anyhow!("{e}"))
    });
    if r.is_err() {
        all_pass = false;
    }
    std::thread::sleep(Duration::from_millis(200));

    // Step 11: Reattach
    let reattached = step!("reattach", attach(2, &session_id, args.cols, args.rows));
    let reattached = match reattached {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("    reattach failed: {e}");
            all_pass = false;
            None
        }
    };

    // Step 12: Kill
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

    // Step 13: bytes_dropped = 0
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
    metrics.insert(
        "worktree_path".into(),
        resolved.worktree_path.unwrap_or_default().into(),
    );
    metrics.insert("branch".into(), resolved.branch_name.into());

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
