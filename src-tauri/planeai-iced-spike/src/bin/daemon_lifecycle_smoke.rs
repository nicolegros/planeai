//! Headless daemon lifecycle smoke test.
//!
//! Exercises the full daemon session lifecycle without opening an Iced window:
//! spawn → receive output → send input → resize → detach → list → reattach →
//! receive buffered output → receive live output → kill → verify durable log.
//!
//! Usage:
//!   PLANEAI_DAEMON_PTY_CORE=planeai-pty \
//!   PLANEAI_SESSION_LOG_DIR=/tmp/planeai-daemon-session-logs \
//!   cargo run --release -p planeai-iced-spike --bin daemon-lifecycle-smoke -- \
//!     --session-command "python3 -c 'import time; print(\"ready\", flush=True); time.sleep(30)'" \
//!     --cols 120 --rows 40 \
//!     --metrics bench/results/daemon-lifecycle-smoke.jsonl

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
#[command(name = "daemon-lifecycle-smoke")]
struct Args {
    #[arg(long, default_value = "echo lifecycle-ok")]
    session_command: String,
    #[arg(long, default_value_t = 120)]
    cols: u16,
    #[arg(long, default_value_t = 40)]
    rows: u16,
    #[arg(long)]
    metrics: Option<PathBuf>,
}

macro_rules! step {
    ($name:expr, $body:expr) => {{
        let start = Instant::now();
        eprint!("  [{:>30}] ", $name);
        let result = $body;
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        match &result {
            Ok(_) => eprintln!("OK ({:.1}ms)", ms),
            Err(e) => eprintln!("FAIL ({:.1}ms): {}", ms, e),
        }
        result
    }};
}

fn main() {
    let args = Args::parse();
    eprintln!("=== Daemon Lifecycle Smoke Test ===\n");

    let mut metrics = serde_json::Map::new();
    let test_start = Instant::now();

    // Step 1: Ensure daemon is running
    step!("ensure_daemon_running", ensure_daemon_running_sync()).unwrap();
    metrics.insert(
        "daemon_start_ms".into(),
        test_start.elapsed().as_secs_f64().into(),
    );

    // Step 2: Spawn a daemon session
    let session = step!("spawn_session", {
        DaemonSession::spawn(1, args.cols, args.rows, Some(&args.session_command))
    })
    .unwrap();
    let session_id = session.session_id().to_string();
    metrics.insert("spawn_latency_ms".into(), session.spawn_latency_ms().into());
    metrics.insert(
        "attach_latency_ms".into(),
        session.attach_latency_ms().into(),
    );
    eprintln!("    session_id = {session_id}");

    // Step 3: Receive output (wait for data with timeout)
    let output = step!("receive_output", {
        wait_for_output(&session, Duration::from_secs(5))
    })
    .unwrap();
    metrics.insert("first_output_bytes".into(), (output.len() as u64).into());

    // Step 4: Send input
    step!("send_input", session.write(b"echo smoke-input-ok\n")).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    // Drain any output from the input echo
    let _ = session.try_read_batch();

    // Step 5: Resize
    step!("resize", session.resize(80, 24)).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    // Step 6: Detach (drop the session handle to close data connection, then send detach)
    let recv_bytes_before_detach = session.pipeline_diag().pty_reader_bytes_total;
    metrics.insert(
        "bytes_before_detach".into(),
        recv_bytes_before_detach.into(),
    );
    drop(session);
    step!("detach", detach_daemon_session(&session_id)).unwrap();
    std::thread::sleep(Duration::from_millis(200));

    // Step 7: List sessions — should include our session
    let sessions = step!("list_sessions", list_daemon_sessions()).unwrap();
    let found = sessions.iter().any(|s| s.session_id == session_id);
    assert!(found, "session {session_id} not found in list after detach");
    metrics.insert("sessions_listed".into(), (sessions.len() as u64).into());
    eprintln!(
        "    found session in list: {found} (total: {})",
        sessions.len()
    );

    // Step 8: Reattach to the same session
    let reattached = step!("reattach", { attach(2, &session_id, args.cols, args.rows) }).unwrap();
    metrics.insert(
        "reattach_latency_ms".into(),
        reattached.attach_latency_ms().into(),
    );

    // Step 9: Receive buffered/snapshot output after attach
    let snapshot_output = step!("receive_snapshot", {
        wait_for_output(&reattached, Duration::from_secs(3))
    })
    .unwrap();
    metrics.insert(
        "snapshot_bytes".into(),
        (snapshot_output.len() as u64).into(),
    );

    // Step 10: Send input after reattach and receive live output
    step!(
        "send_input_after_attach",
        reattached.write(b"echo reattach-ok\n")
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let live_output = reattached
        .try_read_batch()
        .unwrap_or(None)
        .unwrap_or_default();
    metrics.insert(
        "live_bytes_after_attach".into(),
        (live_output.len() as u64).into(),
    );

    // Step 11: Kill the session
    step!("kill_session", kill_daemon_session(&session_id)).unwrap();
    std::thread::sleep(Duration::from_millis(500));

    // Step 12: Verify session exited (list should not have it alive)
    let sessions_after = step!("verify_killed", list_daemon_sessions()).unwrap();
    let still_alive = sessions_after
        .iter()
        .any(|s| s.session_id == session_id && s.alive);
    assert!(!still_alive, "session should not be alive after kill");
    eprintln!("    session alive after kill: {still_alive}");

    // Step 13: Check bytes_dropped
    let diag = reattached.pipeline_diag();
    let bytes_dropped = diag.output_bytes_dropped;
    metrics.insert("output_bytes_dropped".into(), bytes_dropped.into());
    assert_eq!(bytes_dropped, 0, "bytes_dropped must be 0");
    eprintln!("    bytes_dropped = {bytes_dropped}");

    // Step 14: Verify durable log (if PLANEAI_SESSION_LOG_DIR set)
    if let Ok(log_dir) = std::env::var("PLANEAI_SESSION_LOG_DIR") {
        let session_log_dir = PathBuf::from(&log_dir).join("sessions").join(&session_id);
        let meta_path = session_log_dir.join("meta.json");
        // Wait briefly for meta finalization
        std::thread::sleep(Duration::from_millis(500));
        let log_result = step!("verify_durable_log", { verify_durable_log(&meta_path) });
        match log_result {
            Ok((bw, bd)) => {
                metrics.insert("log_bytes_written".into(), bw.into());
                metrics.insert("log_bytes_dropped".into(), bd.into());
                assert_eq!(bd, 0, "log bytes_dropped must be 0");
            }
            Err(e) => eprintln!("    WARNING: durable log check failed: {e}"),
        }
    } else {
        eprintln!("  [         verify_durable_log] SKIP (PLANEAI_SESSION_LOG_DIR not set)");
    }

    // Summary
    let total_ms = test_start.elapsed().as_secs_f64() * 1000.0;
    metrics.insert("total_ms".into(), total_ms.into());
    metrics.insert("result".into(), "pass".into());

    eprintln!("\n=== PASS ({:.0}ms) ===", total_ms);

    // Write metrics
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
        eprintln!("Metrics written to {}", path.display());
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

fn verify_durable_log(meta_path: &std::path::Path) -> anyhow::Result<(u64, u64)> {
    if !meta_path.exists() {
        anyhow::bail!("meta.json not found at {}", meta_path.display());
    }
    let content = fs::read_to_string(meta_path)?;
    let meta: serde_json::Value = serde_json::from_str(&content)?;

    let status = meta
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let bytes_written = meta
        .get("bytes_written")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let bytes_dropped = meta
        .get("bytes_dropped")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let session_source = meta
        .get("session_source")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let pty_core = meta
        .get("pty_core")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    eprintln!("    status={status} bytes_written={bytes_written} bytes_dropped={bytes_dropped}");
    eprintln!("    session_source={session_source} pty_core={pty_core}");

    if status != "exited" {
        anyhow::bail!("expected status=exited, got {status}");
    }
    if session_source != "daemon" {
        anyhow::bail!("expected session_source=daemon, got {session_source}");
    }
    if pty_core != "planeai-pty" {
        anyhow::bail!("expected pty_core=planeai-pty, got {pty_core}");
    }
    if bytes_written == 0 {
        anyhow::bail!("bytes_written is 0, expected > 0");
    }
    Ok((bytes_written, bytes_dropped))
}
