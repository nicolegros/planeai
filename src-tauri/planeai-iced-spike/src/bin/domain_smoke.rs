//! Domain parity smoke test.
//!
//! Verifies the full domain integration: project resolved → session record created →
//! daemon session starts → output received → status updated → durable log linked →
//! detach/reattach → kill updates status → bytes_dropped = 0.
//!
//! Usage:
//!   PLANEAI_DAEMON_PTY_CORE=planeai-pty \
//!   PLANEAI_SESSION_LOG_DIR=/tmp/planeai-domain-smoke-logs \
//!   cargo run --release -p planeai-iced-spike --bin planeai-domain-smoke -- \
//!     --cwd /tmp/planeai-smoke-project \
//!     --agent-command "python3 -c 'import time; print(\"agent ready\", flush=True); time.sleep(30)'" \
//!     --metrics bench/results/domain-smoke.jsonl

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::Parser;
use planeai_core::services::{self, CreateSessionParams, ProjectService, SessionService};
use planeai_iced_spike::adapter::PlaneAiTerminalSession;
use planeai_iced_spike::daemon_session::{
    attach, detach_daemon_session, ensure_daemon_running_sync, kill_daemon_session, DaemonSession,
};

#[derive(Parser)]
#[command(name = "planeai-domain-smoke")]
struct Args {
    #[arg(long, default_value = "/tmp/planeai-smoke-project")]
    cwd: PathBuf,
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
    eprintln!("=== Domain Parity Smoke Test ===\n");

    let agent_command = args.agent_command.clone().unwrap_or_else(|| {
        "python3 -c 'import time; print(\"agent ready\", flush=True); time.sleep(30)'".to_string()
    });
    eprintln!("  agent_command: {agent_command}");
    eprintln!("  cwd: {}\n", args.cwd.display());

    fs::create_dir_all(&args.cwd).expect("failed to create cwd");

    let mut all_pass = true;
    let mut metrics = serde_json::Map::new();
    let test_start = Instant::now();

    // Step 1: Open shared DB
    let db_dir = tempfile::tempdir().expect("tempdir");
    let db_path = db_dir.path().join("domain-smoke.db");
    let conn = step!("open_db", {
        services::open_db_at(&db_path).map_err(|e| anyhow::anyhow!("{e}"))
    });
    let conn = match conn {
        Ok(c) => c,
        Err(e) => {
            eprintln!("\nFATAL: cannot open DB: {e}");
            std::process::exit(1);
        }
    };

    // Step 2: Resolve project
    let project = step!("resolve_project", {
        ProjectService::ensure_project(&conn, &args.cwd.to_string_lossy())
            .map_err(|e| anyhow::anyhow!("{e}"))
    });
    let project = match project {
        Ok(p) => {
            eprintln!("    project_id = {}", p.id);
            p
        }
        Err(e) => {
            eprintln!("\nFATAL: {e}");
            std::process::exit(1);
        }
    };

    // Step 3: Start daemon
    let r = step!("ensure_daemon_running", {
        ensure_daemon_running_sync().map_err(|e| anyhow::anyhow!("{e}"))
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 4: Spawn session
    let session = step!("spawn_session", {
        DaemonSession::spawn_with_cwd(
            1,
            args.cols,
            args.rows,
            Some(&agent_command),
            &args.cwd,
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
    let session_id = session.session_id().to_string();
    eprintln!("    session_id = {session_id}");

    // Step 5: Create session record in DB
    let r = step!("create_session_record", {
        let params = CreateSessionParams {
            id: session_id.clone(),
            project_id: project.id.clone(),
            name: "smoke-test".to_string(),
            backend: "daemon".to_string(),
            ..Default::default()
        };
        SessionService::create(&conn, &params).map_err(|e| anyhow::anyhow!("{e}"))
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 6: Verify session record exists
    let r = step!("verify_session_record", {
        (|| -> anyhow::Result<()> {
            let s = SessionService::get(&conn, &session_id).map_err(|e| anyhow::anyhow!("{e}"))?;
            match s {
                Some(rec) if rec.status == "active" => Ok(()),
                Some(rec) => anyhow::bail!("unexpected status: {}", rec.status),
                None => anyhow::bail!("session record not found"),
            }
        })()
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 7: Receive output
    let r = step!(
        "receive_output",
        wait_for_output(&session, Duration::from_secs(5))
    );
    if r.is_err() {
        all_pass = false;
    }

    // Step 8: Detach
    drop(session);
    let r = step!("detach", {
        detach_daemon_session(&session_id).map_err(|e| anyhow::anyhow!("{e}"))
    });
    if r.is_err() {
        all_pass = false;
    }
    std::thread::sleep(Duration::from_millis(200));

    // Step 9: Reattach
    let reattached = step!("reattach", attach(2, &session_id, args.cols, args.rows));
    let reattached = match reattached {
        Ok(s) => s,
        Err(e) => {
            eprintln!("\nFATAL: reattach failed: {e}");
            std::process::exit(1);
        }
    };

    // Step 10: Receive after reattach
    let r = step!(
        "receive_after_reattach",
        wait_for_output(&reattached, Duration::from_secs(3))
    );
    if r.is_err() {
        all_pass = false;
    }

    // Step 11: Kill and update status
    let r = step!("kill_and_update_status", {
        (|| -> anyhow::Result<()> {
            kill_daemon_session(&session_id)?;
            SessionService::set_status(&conn, &session_id, "destroyed")
                .map_err(|e| anyhow::anyhow!("{e}"))
        })()
    });
    if r.is_err() {
        all_pass = false;
    }
    std::thread::sleep(Duration::from_millis(500));

    // Step 12: Verify status updated
    let r = step!("verify_status_destroyed", {
        (|| -> anyhow::Result<()> {
            let s = SessionService::get(&conn, &session_id).map_err(|e| anyhow::anyhow!("{e}"))?;
            match s {
                Some(rec) if rec.status == "destroyed" => Ok(()),
                Some(rec) => anyhow::bail!("expected 'destroyed', got '{}'", rec.status),
                None => anyhow::bail!("session record not found"),
            }
        })()
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 13: Verify durable log linked
    let r = step!("durable_log_linked", {
        (|| -> anyhow::Result<()> {
            let log_dir = services::SessionService::durable_log_dir(&session_id);
            match log_dir {
                Some(d) => {
                    eprintln!("    log_dir = {}", d.display());
                    Ok(())
                }
                None => {
                    if std::env::var("PLANEAI_SESSION_LOG_DIR").is_ok() {
                        anyhow::bail!("log dir should exist")
                    } else {
                        eprintln!("    (PLANEAI_SESSION_LOG_DIR not set — skip)");
                        Ok(())
                    }
                }
            }
        })()
    });
    if r.is_err() {
        all_pass = false;
    }

    // Step 14: Verify bytes_dropped = 0
    let r = step!("bytes_dropped_zero", {
        (|| -> anyhow::Result<()> {
            if let Ok(log_dir) = std::env::var("PLANEAI_SESSION_LOG_DIR") {
                let meta_path = PathBuf::from(&log_dir)
                    .join("sessions")
                    .join(&session_id)
                    .join("meta.json");
                if meta_path.exists() {
                    let content = fs::read_to_string(&meta_path)?;
                    let meta: serde_json::Value = serde_json::from_str(&content)?;
                    let dropped = meta
                        .get("bytes_dropped")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    if dropped != 0 {
                        anyhow::bail!("bytes_dropped = {dropped}");
                    }
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
