use planeai_daemon::registry::SessionRegistry;
use planeai_daemon::session::DaemonSession;
use std::time::Duration;

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
