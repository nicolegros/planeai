//! Headless workflow smoke test.
//!
//! Verifies the workflow daemon integration: spawn with explicit cwd → receive
//! output → list → send input → detach → verify listed → reattach → receive
//! output → kill → verify durable log.
//!
//! Usage:
//!   PLANEAI_DAEMON_PTY_CORE=planeai-pty \
//!   PLANEAI_SESSION_LOG_DIR=/tmp/planeai-workflow-smoke-logs \
//!   cargo run --release -p planeai-iced-spike --bin planeai-workflow-smoke -- \
//!     --agent-command "python3 -c 'print(\"agent ready\")'; sleep 30" \
//!     --metrics bench/results/workflow-smoke.jsonl

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::Parser;
use planeai_iced_spike::adapter::PlaneAiTerminalSession;
use planeai_iced_spike::daemon_session::{
    attach, detach_daemon_session, ensure_daemon_running_sync, kill_daemon_session,
    list_daemon_sessions, DaemonSession,
};

#[derive(Parser)]
#[command(name = "planeai-workflow-smoke")]
struct Args {
    #[arg(long, default_value = "/tmp/planeai-smoke-project")]
    cwd: PathBuf,
    #[arg(long, default_value = "python3 -c 'print(\"agent ready\")'; sleep 30")]
    agent_command: String,
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

fn main() {
    let args = Args::parse();
    eprintln!("=== Workflow Smoke Test ===\n");

    // Ensure cwd exists
    fs::create_dir_all(&args.cwd).expect("failed to create cwd directory");

    let mut metrics = serde_json::Map::new();
    let mut all_pass = true;
    let test_start = Instant::now();

    // Step 1: Start daemon
    let r = step!("ensure_daemon_running", Ok(()).and_then(|_| ensure_daemon_running_sync()));
    if r.is_err() { all_pass = false; }

    // Step 2: Spawn session with explicit cwd
    let session = step!("spawn_with_cwd", {
        DaemonSession::spawn_with_cwd(1, args.cols, args.rows, Some(&args.agent_command), &args.cwd, &[])
    });
    let session = match session {
        Ok(s) => s,
        Err(e) => {
            eprintln!("\nFATAL: cannot continue without session: {e}");
            std::process::exit(1);
        }
    };
    let session_id = session.session_id().to_string();
    metrics.insert("spawn_latency_ms".into(), session.spawn_latency_ms().into());
    eprintln!("    session_id = {session_id}");

    // Step 3: Receive output
    let r = step!("receive_output", wait_for_output(&session, Duration::from_secs(5)));
    match &r {
        Ok(data) => { metrics.insert("first_output_bytes".into(), (data.len() as u64).into()); }
        Err(_) => { all_pass = false; }
    }

    // Step 4: Verify session in list
    let r = step!("list_sessions_contains", (|| -> anyhow::Result<usize> {
        let sessions = list_daemon_sessions()?;
        if sessions.iter().any(|s| s.session_id == session_id) {
            Ok(sessions.len())
        } else {
            anyhow::bail!("session {session_id} not in list")
        }
    })());
    if r.is_err() { all_pass = false; }

    // Step 5: Send input
    let r = step!("send_input", session.write(b"echo workflow-ok\n"));
    if r.is_err() { all_pass = false; }
    std::thread::sleep(Duration::from_millis(300));
    let _ = session.try_read_batch();

    // Step 6: Detach
    drop(session);
    let r = step!("detach", detach_daemon_session(&session_id));
    if r.is_err() { all_pass = false; }
    std::thread::sleep(Duration::from_millis(200));

    // Step 7: Verify session still in list after detach
    let r = step!("list_after_detach", (|| -> anyhow::Result<()> {
        let sessions = list_daemon_sessions()?;
        if sessions.iter().any(|s| s.session_id == session_id) {
            Ok(())
        } else {
            anyhow::bail!("session {session_id} not in list after detach")
        }
    })());
    if r.is_err() { all_pass = false; }

    // Step 8: Reattach
    let reattached = step!("reattach", attach(2, &session_id, args.cols, args.rows));
    let reattached = match reattached {
        Ok(s) => s,
        Err(e) => {
            eprintln!("\nFATAL: cannot continue without reattach: {e}");
            std::process::exit(1);
        }
    };
    metrics.insert("reattach_latency_ms".into(), reattached.attach_latency_ms().into());

    // Step 9: Receive output after reattach
    let r = step!("receive_after_reattach", wait_for_output(&reattached, Duration::from_secs(3)));
    if r.is_err() { all_pass = false; }

    // Step 10: Kill session
    let r = step!("kill_session", kill_daemon_session(&session_id));
    if r.is_err() { all_pass = false; }
    std::thread::sleep(Duration::from_millis(500));

    // Step 11: Check durable log exists
    if let Ok(log_dir) = std::env::var("PLANEAI_SESSION_LOG_DIR") {
        let session_log_dir = PathBuf::from(&log_dir).join("sessions").join(&session_id);
        let meta_path = session_log_dir.join("meta.json");
        std::thread::sleep(Duration::from_millis(500));

        let r = step!("durable_log_exists", (|| -> anyhow::Result<()> {
            if meta_path.exists() { Ok(()) } else { anyhow::bail!("meta.json not found at {}", meta_path.display()) }
        })());
        if r.is_err() { all_pass = false; }

        // Step 12: Check meta.json has correct command and cwd
        let r = step!("meta_json_fields", (|| -> anyhow::Result<()> {
            let content = fs::read_to_string(&meta_path)?;
            let meta: serde_json::Value = serde_json::from_str(&content)?;
            let meta_cwd = meta.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
            let meta_command = meta.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if meta_cwd != args.cwd.to_string_lossy().as_ref() {
                anyhow::bail!("cwd mismatch: expected {:?}, got {:?}", args.cwd.display(), meta_cwd);
            }
            if meta_command.is_empty() {
                anyhow::bail!("command field is empty");
            }
            eprintln!("    command={meta_command} cwd={meta_cwd}");
            Ok(())
        })());
        if r.is_err() { all_pass = false; }

        // Step 13: Verify bytes_dropped = 0
        let r = step!("bytes_dropped_zero", (|| -> anyhow::Result<()> {
            let content = fs::read_to_string(&meta_path)?;
            let meta: serde_json::Value = serde_json::from_str(&content)?;
            let bytes_dropped = meta.get("bytes_dropped").and_then(|v| v.as_u64()).unwrap_or(0);
            if bytes_dropped != 0 {
                anyhow::bail!("bytes_dropped = {bytes_dropped}, expected 0");
            }
            Ok(())
        })());
        if r.is_err() { all_pass = false; }
    } else {
        eprintln!("  [{:>34}] SKIP (PLANEAI_SESSION_LOG_DIR not set)", "durable_log_exists");
        eprintln!("  [{:>34}] SKIP (PLANEAI_SESSION_LOG_DIR not set)", "meta_json_fields");
        eprintln!("  [{:>34}] SKIP (PLANEAI_SESSION_LOG_DIR not set)", "bytes_dropped_zero");
    }

    // Step 14: Write metrics
    let total_ms = test_start.elapsed().as_secs_f64() * 1000.0;
    metrics.insert("total_ms".into(), total_ms.into());
    metrics.insert("result".into(), if all_pass { "pass" } else { "fail" }.into());

    if let Some(path) = &args.metrics {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("failed to open metrics file");
        let line = serde_json::to_string(&metrics).unwrap();
        writeln!(f, "{line}").unwrap();
        eprintln!("\nMetrics written to {}", path.display());
    }

    if all_pass {
        eprintln!("\n=== ALL PASS ({:.0}ms) ===", total_ms);
    } else {
        eprintln!("\n=== SOME STEPS FAILED ({:.0}ms) ===", total_ms);
        std::process::exit(1);
    }
}

fn wait_for_output(
    session: &dyn PlaneAiTerminalSession,
    timeout: Duration,
) -> anyhow::Result<Vec<u8>> {
    let start = Instant::now();
    let mut collected = Vec::new();
    while start.elapsed() < timeout {
        if let Some(data) = session.try_read_batch()? {
            collected.extend_from_slice(&data);
            if !collected.is_empty() {
                return Ok(collected);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if collected.is_empty() {
        anyhow::bail!("no output received within {:?}", timeout);
    }
    Ok(collected)
}
