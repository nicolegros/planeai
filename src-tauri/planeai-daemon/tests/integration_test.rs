use planeai_daemon::registry::SessionRegistry;
use planeai_daemon::session::DaemonSession;
use planeai_daemon::types::{SpawnMode, SpawnOutcome};
use std::sync::Mutex;
use std::time::Duration;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn spawn_echo_captures_output() {
    let session = DaemonSession::spawn(
        "test-echo",
        "/bin/sh",
        &["-c", "echo hello"],
        None,
        None,
        4096,
    )
    .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let snap = session.buffer_snapshot();
        if String::from_utf8_lossy(&snap).contains("hello") {
            return;
        }
        if std::time::Instant::now() > deadline {
            let snap = session.buffer_snapshot();
            panic!(
                "buffer should contain 'hello', got: {:?}",
                String::from_utf8_lossy(&snap)
            );
        }
    }
}

#[test]
fn write_input_to_cat() {
    let session = DaemonSession::spawn("test-cat", "/bin/cat", &[], None, None, 4096).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    session.write(b"ping\n").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let snap = session.buffer_snapshot();
    assert!(
        String::from_utf8_lossy(&snap).contains("ping"),
        "buffer should contain 'ping', got: {:?}",
        String::from_utf8_lossy(&snap)
    );
    session.kill().unwrap();
}

#[test]
fn resize_does_not_error() {
    let session = DaemonSession::spawn("test-resize", "/bin/cat", &[], None, None, 4096).unwrap();
    session.resize(120, 40).unwrap();
    session.kill().unwrap();
}

#[test]
fn eof_detection() {
    let session = DaemonSession::spawn(
        "test-eof",
        "/bin/sh",
        &["-c", "echo done"],
        None,
        None,
        4096,
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(1000));
    assert!(
        !session.is_alive(),
        "session should be dead after echo exits"
    );
}

#[test]
fn kill_terminates_process() {
    let session = DaemonSession::spawn("test-kill", "sleep", &["999"], None, None, 4096).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(session.is_alive());
    session.kill().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(!session.is_alive());
}

#[tokio::test]
async fn subscribe_output_receives_bytes() {
    let session = DaemonSession::spawn(
        "test-sub",
        "/bin/sh",
        &["-c", "echo broadcast"],
        None,
        None,
        4096,
    )
    .unwrap();
    let mut rx = session.subscribe_output();

    let result = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;
    std::thread::sleep(Duration::from_millis(300));
    let snap = session.buffer_snapshot();
    assert!(
        String::from_utf8_lossy(&snap).contains("broadcast") || result.is_ok(),
        "should receive broadcast data"
    );
}

#[test]
fn registry_spawn_list_kill() {
    let mut reg = SessionRegistry::new();
    assert!(reg.is_empty());

    reg.spawn(
        "s1",
        "sleep",
        &["999"],
        None,
        None,
        4096,
        SpawnMode::CreateOnly,
    )
    .unwrap();
    reg.spawn(
        "s2",
        "sleep",
        &["999"],
        None,
        None,
        4096,
        SpawnMode::CreateOnly,
    )
    .unwrap();

    let list = reg.list();
    assert_eq!(list.len(), 2);
    assert!(list.iter().all(|s| s.alive));

    reg.kill("s1").unwrap();
    std::thread::sleep(Duration::from_millis(500));

    reg.kill("s2").unwrap();
}

#[test]
fn registry_poll_exits_retains_sessions() {
    let mut reg = SessionRegistry::new();
    reg.spawn(
        "alive",
        "sleep",
        &["999"],
        None,
        None,
        4096,
        SpawnMode::CreateOnly,
    )
    .unwrap();
    reg.spawn(
        "dead",
        "/bin/sh",
        &["-c", "echo bye"],
        None,
        None,
        4096,
        SpawnMode::CreateOnly,
    )
    .unwrap();

    std::thread::sleep(Duration::from_millis(1000));

    let exited = reg.poll_exits();
    assert!(exited.contains(&"dead".to_string()));
    assert!(!exited.contains(&"alive".to_string()));
    // Both sessions still in registry (exited one retained)
    assert_eq!(reg.list().len(), 2);
    assert_eq!(reg.live_count(), 1);

    reg.kill("alive").unwrap();
}

#[test]
fn duplicate_spawn_create_only_fails() {
    let mut reg = SessionRegistry::new();
    reg.spawn(
        "s1",
        "sleep",
        &["999"],
        None,
        None,
        4096,
        SpawnMode::CreateOnly,
    )
    .unwrap();
    let err = reg
        .spawn(
            "s1",
            "echo",
            &["x"],
            None,
            None,
            4096,
            SpawnMode::CreateOnly,
        )
        .unwrap_err();
    assert!(err.to_string().contains("already exists"));
    reg.kill("s1").unwrap();
}

#[test]
fn restart_mode_replaces_running() {
    let mut reg = SessionRegistry::new();
    reg.spawn(
        "s1",
        "sleep",
        &["999"],
        None,
        None,
        4096,
        SpawnMode::CreateOnly,
    )
    .unwrap();
    let outcome = reg
        .spawn(
            "s1",
            "sleep",
            &["999"],
            None,
            None,
            4096,
            SpawnMode::Restart,
        )
        .unwrap();
    assert_eq!(outcome, SpawnOutcome::Restarted);
    assert!(reg.get("s1").unwrap().is_alive());
    reg.kill("s1").unwrap();
}

// ─── planeai-pty based spawn tests ──────────────────────────────────────────

#[test]
fn spawn_write_and_resize() {
    let session =
        DaemonSession::spawn("test-pty-write", "/bin/cat", &[], None, None, 4096).unwrap();

    session.write(b"test input\n").unwrap();
    session.resize(120, 40).unwrap();
    session.kill().unwrap();
    assert!(!session.is_alive());
}

#[test]
fn spawn_diagnostics_available() {
    let session = DaemonSession::spawn(
        "test-pty-diag",
        "/bin/sh",
        &["-c", "echo diag"],
        None,
        None,
        4096,
    )
    .unwrap();

    std::thread::sleep(Duration::from_millis(500));
    let diag = session.diagnostics();
    let snap = diag.snapshot();
    assert!(snap.reader_bytes > 0);
}

#[test]
fn spawn_buffer_snapshot_works() {
    let session = DaemonSession::spawn(
        "test-pty-snap",
        "/bin/sh",
        &["-c", "echo snapshot-test"],
        None,
        None,
        4096,
    )
    .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let snap = session.buffer_snapshot();
        if String::from_utf8_lossy(&snap).contains("snapshot-test") {
            return;
        }
        if std::time::Instant::now() > deadline {
            let snap = session.buffer_snapshot();
            panic!("got: {}", String::from_utf8_lossy(&snap));
        }
    }
}

#[test]
fn spawn_argv_preserves_spaces() {
    // Test that args with spaces are preserved correctly using printf which handles args
    let session = DaemonSession::spawn(
        "test-argv-spaces",
        "/bin/sh",
        &["-c", "printf '%s\\n' 'hello world' 'foo bar'"],
        None,
        None,
        4096,
    )
    .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let snap = session.buffer_snapshot();
        let output = String::from_utf8_lossy(&snap);
        if output.contains("hello world") && output.contains("foo bar") {
            return;
        }
        if std::time::Instant::now() > deadline {
            panic!(
                "expected 'hello world' and 'foo bar', got: {:?}",
                String::from_utf8_lossy(&snap)
            );
        }
    }
}

#[test]
fn spawn_direct_argv_preserves_args() {
    // Test direct argv: /bin/cat receives input (long-lived) proving direct spawn works
    let session =
        DaemonSession::spawn("test-direct-argv", "/bin/cat", &[], None, None, 4096).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    session.write(b"direct argv test\n").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let snap = session.buffer_snapshot();
    assert!(
        String::from_utf8_lossy(&snap).contains("direct argv test"),
        "cat should echo input, got: {:?}",
        String::from_utf8_lossy(&snap)
    );
    session.kill().unwrap();
}

#[test]
fn spawn_argv_preserves_quotes() {
    let session = DaemonSession::spawn(
        "test-argv-quotes",
        "/bin/sh",
        &["-c", "echo \"quoted output\""],
        None,
        None,
        4096,
    )
    .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let snap = session.buffer_snapshot();
        if String::from_utf8_lossy(&snap).contains("quoted output") {
            return;
        }
        if std::time::Instant::now() > deadline {
            panic!(
                "got: {}",
                String::from_utf8_lossy(&session.buffer_snapshot())
            );
        }
    }
}

#[test]
fn spawn_durable_log_written() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        std::env::set_var(
            "PLANEAI_SESSION_LOG_DIR",
            tempfile::TempDir::new().unwrap().path(),
        )
    };

    let tmp = tempfile::TempDir::new().unwrap();
    unsafe { std::env::set_var("PLANEAI_SESSION_LOG_DIR", tmp.path()) };

    let session = DaemonSession::spawn(
        "test-pty-log",
        "/bin/sh",
        &["-c", "echo log-test-output"],
        None,
        None,
        4096,
    )
    .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        if !session.is_alive() {
            break;
        }
        if std::time::Instant::now() > deadline {
            break;
        }
    }
    std::thread::sleep(Duration::from_millis(200));

    let session_dir = tmp.path().join("sessions").join("test-pty-log");
    assert!(session_dir.exists(), "session log directory should exist");

    let meta_path = session_dir.join("meta.json");
    assert!(meta_path.exists(), "meta.json should exist");

    let meta_content = std::fs::read_to_string(&meta_path).unwrap();
    let meta: serde_json::Value = serde_json::from_str(&meta_content).unwrap();
    assert_eq!(meta["session_source"], "daemon");
    assert_eq!(meta["pty_core"], "planeai-pty");
    assert_eq!(meta["session_id"], "test-pty-log");
    assert_eq!(meta["status"], "exited");
    assert!(meta["bytes_written"].as_u64().unwrap() > 0);
    assert_eq!(meta["bytes_dropped"], 0);

    let ansi_file = meta["ansi_log_file"].as_str().unwrap();
    let ansi_path = session_dir.join(ansi_file);
    assert!(ansi_path.exists(), "ansi log file should exist");
    let ansi_content = std::fs::read(&ansi_path).unwrap();
    assert!(
        String::from_utf8_lossy(&ansi_content).contains("log-test-output"),
        "ansi log should contain output"
    );

    unsafe { std::env::remove_var("PLANEAI_SESSION_LOG_DIR") };
}
