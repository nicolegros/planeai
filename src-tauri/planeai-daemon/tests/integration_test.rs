use planeai_daemon::registry::SessionRegistry;
use planeai_daemon::session::DaemonSession;
use std::time::Duration;
use tempfile;

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
    std::thread::sleep(Duration::from_millis(500));
    let snap = session.buffer_snapshot();
    assert!(
        String::from_utf8_lossy(&snap).contains("hello"),
        "buffer should contain 'hello', got: {:?}",
        String::from_utf8_lossy(&snap)
    );
}

#[test]
fn write_input_to_cat() {
    let session = DaemonSession::spawn("test-cat", "cat", &[], None, None, 4096).unwrap();
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
    let session = DaemonSession::spawn("test-resize", "cat", &[], None, None, 4096).unwrap();
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
    // Either we get data or the channel was lagged (data already sent before subscribe)
    // The buffer should still have captured it
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

    reg.spawn("s1", "sleep", &["999"], None, None, 4096)
        .unwrap();
    reg.spawn("s2", "sleep", &["999"], None, None, 4096)
        .unwrap();

    let list = reg.list();
    assert_eq!(list.len(), 2);
    assert!(list.iter().all(|s| s.alive));

    reg.kill("s1").unwrap();
    std::thread::sleep(Duration::from_millis(500));

    reg.kill("s2").unwrap();
}

#[test]
fn registry_remove_dead() {
    let mut reg = SessionRegistry::new();
    reg.spawn("alive", "sleep", &["999"], None, None, 4096)
        .unwrap();
    reg.spawn("dead", "/bin/sh", &["-c", "echo bye"], None, None, 4096)
        .unwrap();

    std::thread::sleep(Duration::from_millis(1000));

    let removed = reg.remove_dead();
    assert!(removed.contains(&"dead".to_string()));
    assert!(!removed.contains(&"alive".to_string()));
    assert_eq!(reg.list().len(), 1);

    reg.kill("alive").unwrap();
}

// ─── Daemon PTY Core Selection Tests ─────────────────────────────────────────

#[test]
fn daemon_pty_core_unset_returns_legacy() {
    std::env::remove_var("PLANEAI_DAEMON_PTY_CORE");
    assert!(!planeai_daemon::session::use_planeai_pty_core());
}

#[test]
fn daemon_pty_core_legacy_returns_false() {
    std::env::set_var("PLANEAI_DAEMON_PTY_CORE", "legacy");
    assert!(!planeai_daemon::session::use_planeai_pty_core());
    std::env::remove_var("PLANEAI_DAEMON_PTY_CORE");
}

#[test]
fn daemon_pty_core_planeai_pty_returns_true() {
    std::env::set_var("PLANEAI_DAEMON_PTY_CORE", "planeai-pty");
    assert!(planeai_daemon::session::use_planeai_pty_core());
    std::env::remove_var("PLANEAI_DAEMON_PTY_CORE");
}

#[test]
fn daemon_pty_core_invalid_falls_back_to_legacy() {
    std::env::set_var("PLANEAI_DAEMON_PTY_CORE", "banana");
    assert!(!planeai_daemon::session::use_planeai_pty_core());
    std::env::remove_var("PLANEAI_DAEMON_PTY_CORE");
}

#[test]
fn spawn_planeai_pty_echo_captures_output() {
    let session = DaemonSession::spawn_planeai_pty(
        "test-pty-echo",
        "echo",
        &["hello-pty"],
        None,
        None,
        4096,
    )
    .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let snap = session.buffer_snapshot();
        if String::from_utf8_lossy(&snap).contains("hello-pty") {
            return;
        }
        if std::time::Instant::now() > deadline {
            let snap = session.buffer_snapshot();
            panic!("got: {}", String::from_utf8_lossy(&snap));
        }
    }
}

#[test]
fn spawn_planeai_pty_write_and_resize() {
    let session = DaemonSession::spawn_planeai_pty(
        "test-pty-write",
        "/bin/cat",
        &[],
        None,
        None,
        4096,
    )
    .unwrap();

    // Write should not error
    session.write(b"test input\n").unwrap();
    // Resize should not error
    session.resize(120, 40).unwrap();
    // Kill should not error
    session.kill().unwrap();
    assert!(!session.is_alive());
}

#[test]
fn spawn_planeai_pty_diagnostics_available() {
    let session = DaemonSession::spawn_planeai_pty(
        "test-pty-diag",
        "/bin/sh",
        &["-c", "echo diag"],
        None,
        None,
        4096,
    )
    .unwrap();

    std::thread::sleep(Duration::from_millis(200));
    let diag = session.diagnostics();
    assert!(diag.is_some());
    let snap = diag.unwrap().snapshot();
    assert!(snap.reader_bytes > 0);
}

#[test]
fn spawn_planeai_pty_buffer_snapshot_works() {
    let session = DaemonSession::spawn_planeai_pty(
        "test-pty-snap",
        "echo",
        &["snapshot-test"],
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
fn local_pty_core_selection_independent_from_daemon() {
    // Setting PLANEAI_LOCAL_PTY_CORE should not affect daemon selection
    std::env::set_var("PLANEAI_LOCAL_PTY_CORE", "planeai-pty");
    std::env::remove_var("PLANEAI_DAEMON_PTY_CORE");
    assert!(!planeai_daemon::session::use_planeai_pty_core());
    std::env::remove_var("PLANEAI_LOCAL_PTY_CORE");
}

#[test]
fn spawn_planeai_pty_durable_log_written() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::env::set_var("PLANEAI_SESSION_LOG_DIR", tmp.path());

    let session = DaemonSession::spawn_planeai_pty(
        "test-pty-log",
        "echo",
        &["log-test-output"],
        None,
        None,
        4096,
    )
    .unwrap();

    // Wait for output and exit
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
    // Give flusher time to write
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

    // Check .ansi file exists and has content
    let ansi_file = meta["ansi_log_file"].as_str().unwrap();
    let ansi_path = session_dir.join(ansi_file);
    assert!(ansi_path.exists(), "ansi log file should exist");
    let ansi_content = std::fs::read(&ansi_path).unwrap();
    assert!(
        String::from_utf8_lossy(&ansi_content).contains("log-test-output"),
        "ansi log should contain output"
    );

    std::env::remove_var("PLANEAI_SESSION_LOG_DIR");
}
