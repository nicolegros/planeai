use planeai_pty::{LocalPtyConfig, PipelineDiagnostics, PtyEvent, PtyEventSink, QueuePolicy};
use std::sync::{Arc, Mutex};

#[test]
fn config_defaults() {
    let cfg = LocalPtyConfig::default();
    assert_eq!(cfg.cols, 80);
    assert_eq!(cfg.rows, 24);
    assert_eq!(cfg.read_buffer_size, 16 * 1024);
    assert_eq!(cfg.coalesce_ms, 4);
    assert_eq!(cfg.queue_policy, QueuePolicy::Block);
    assert_eq!(cfg.queue_capacity_bytes, 512 * 1024);
    assert!(cfg.command.is_none());
    assert!(cfg.shell.is_none());
    assert!(cfg.cwd.is_none());
    assert!(cfg.env.is_empty());
}

#[test]
fn queue_policy_default_is_block() {
    assert_eq!(QueuePolicy::default(), QueuePolicy::Block);
}

#[test]
fn diagnostics_counters_start_at_zero() {
    let diag = PipelineDiagnostics::new();
    let snap = diag.snapshot();
    assert_eq!(snap.reader_bytes, 0);
    assert_eq!(snap.reader_reads, 0);
    assert_eq!(snap.flusher_batches, 0);
    assert_eq!(snap.flusher_bytes, 0);
    assert_eq!(snap.flusher_wakeups, 0);
    assert_eq!(snap.flusher_sleep_ns, 0);
}

#[test]
fn diagnostics_counters_can_be_incremented_and_read() {
    use std::sync::atomic::Ordering;
    let diag = PipelineDiagnostics::new();
    diag.reader_bytes.fetch_add(1024, Ordering::Relaxed);
    diag.reader_reads.fetch_add(1, Ordering::Relaxed);
    diag.flusher_batches.fetch_add(2, Ordering::Relaxed);
    diag.flusher_bytes.fetch_add(2048, Ordering::Relaxed);
    let snap = diag.snapshot();
    assert_eq!(snap.reader_bytes, 1024);
    assert_eq!(snap.reader_reads, 1);
    assert_eq!(snap.flusher_batches, 2);
    assert_eq!(snap.flusher_bytes, 2048);
}

/// Verify PtyEventSink can be implemented without any Tauri/Iced dependency.
struct CollectorSink {
    events: Mutex<Vec<PtyEvent>>,
}

impl PtyEventSink for CollectorSink {
    fn send(&self, event: PtyEvent) -> anyhow::Result<()> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

#[test]
fn sink_receives_output_from_spawned_session() {
    let sink = Arc::new(CollectorSink {
        events: Mutex::new(Vec::new()),
    });
    let config = LocalPtyConfig {
        session_id: 42,
        command: Some("echo hello".to_string()),
        cols: 80,
        rows: 24,
        ..Default::default()
    };
    let session =
        planeai_pty::LocalPtySession::spawn(config, sink.clone()).expect("spawn should succeed");

    // Wait for exit
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !session.has_exited() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(session.has_exited(), "session should have exited");

    let events = sink.events.lock().unwrap();
    let output_bytes: Vec<u8> = events
        .iter()
        .filter_map(|e| match e {
            PtyEvent::Output { bytes, .. } => Some(bytes.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    let output_str = String::from_utf8_lossy(&output_bytes);
    assert!(
        output_str.contains("hello"),
        "output should contain 'hello', got: {output_str}"
    );

    // Should have received an Exit event
    let has_exit = events.iter().any(|e| matches!(e, PtyEvent::Exit { .. }));
    assert!(has_exit, "should receive Exit event");
}

#[test]
fn large_output_does_not_deadlock_sink() {
    let sink = Arc::new(CollectorSink {
        events: Mutex::new(Vec::new()),
    });
    // Generate ~1MB of output
    let config = LocalPtyConfig {
        session_id: 99,
        command: Some("dd if=/dev/zero bs=1024 count=1024 2>/dev/null | cat".to_string()),
        cols: 80,
        rows: 24,
        ..Default::default()
    };
    let session =
        planeai_pty::LocalPtySession::spawn(config, sink.clone()).expect("spawn should succeed");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !session.has_exited() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(session.has_exited(), "session should exit without deadlock");

    let events = sink.events.lock().unwrap();
    let total_bytes: usize = events
        .iter()
        .filter_map(|e| match e {
            PtyEvent::Output { bytes, .. } => Some(bytes.len()),
            _ => None,
        })
        .sum();
    // Should have received substantial output (at least 512KB of the 1MB)
    assert!(
        total_bytes > 512 * 1024,
        "expected >512KB output, got {total_bytes}"
    );
}

/// Regression test: after pause/resume, output must arrive from the PTY.
/// This catches the notify_one vs notify_all bug in FlowControl where only
/// one of reader/flusher threads gets woken on resume.
#[test]
fn pause_resume_delivers_output() {
    // Run multiple iterations to catch the non-deterministic race
    for iteration in 0..20 {
        let sink = Arc::new(CollectorSink {
            events: Mutex::new(Vec::new()),
        });
        let config = LocalPtyConfig {
            session_id: 200 + iteration,
            command: Some("cat".to_string()),
            cols: 80,
            rows: 24,
            ..Default::default()
        };
        let session = planeai_pty::LocalPtySession::spawn(config, sink.clone())
            .expect("spawn should succeed");

        // Let cat start up
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Pause flow — both reader and flusher will block
        session.pause();

        // Give threads time to hit wait_if_paused
        std::thread::sleep(std::time::Duration::from_millis(30));

        // Write input that cat will echo back
        let marker = format!("MARKER_{iteration}\n");
        session
            .write(marker.as_bytes())
            .expect("write should succeed");

        // Data is now in the PTY output buffer but flow is paused
        std::thread::sleep(std::time::Duration::from_millis(30));

        // Resume — both reader AND flusher must wake up
        session.resume();

        // Wait for the echoed output to arrive
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let expected = format!("MARKER_{iteration}");
        loop {
            let events = sink.events.lock().unwrap();
            let output: String = events
                .iter()
                .filter_map(|e| match e {
                    PtyEvent::Output { bytes, .. } => {
                        Some(String::from_utf8_lossy(bytes).to_string())
                    }
                    _ => None,
                })
                .collect();
            if output.contains(&expected) {
                break;
            }
            drop(events);
            if std::time::Instant::now() > deadline {
                panic!(
                    "iteration {iteration}: output never arrived after resume. \
                     Expected '{expected}' in output. This indicates the flusher \
                     thread was not woken by resume (notify_one bug)."
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let _ = session.kill();
    }
}

#[test]
fn no_tauri_iced_dependency() {
    // This is a compile-time guarantee. If planeai-pty depended on tauri or iced,
    // this test crate would fail to compile since those aren't in planeai-pty's deps.
    // Explicit assertion for documentation:
    let cargo_toml = include_str!("../Cargo.toml");
    assert!(
        !cargo_toml.contains("tauri"),
        "planeai-pty must not depend on tauri"
    );
    assert!(
        !cargo_toml.contains("iced"),
        "planeai-pty must not depend on iced"
    );
}

// ── WSL spawn config ──

#[test]
fn config_defaults_wsl_is_none() {
    let cfg = LocalPtyConfig::default();
    assert!(cfg.wsl.is_none());
}

#[test]
fn wsl_spawn_config_can_be_set() {
    use planeai_pty::WslSpawnConfig;
    let cfg = LocalPtyConfig {
        wsl: Some(WslSpawnConfig {
            distro: "Ubuntu".to_string(),
            cwd: Some("/home/user/project".to_string()),
        }),
        ..Default::default()
    };
    assert_eq!(cfg.wsl.as_ref().unwrap().distro, "Ubuntu");
    assert_eq!(
        cfg.wsl.as_ref().unwrap().cwd.as_deref(),
        Some("/home/user/project")
    );
}

#[test]
fn wsl_spawn_config_cwd_optional() {
    use planeai_pty::WslSpawnConfig;
    let wsl = WslSpawnConfig {
        distro: "Debian".to_string(),
        cwd: None,
    };
    assert!(wsl.cwd.is_none());
}
