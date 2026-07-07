#[cfg(unix)]
mod socket_tests {
    use planeai_daemon::server::DaemonServer;
    use planeai_daemon::transport::DaemonListener;
    use serde_json::Value;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    async fn start_server(socket_path: &std::path::Path) -> tokio::sync::watch::Sender<()> {
        let listener = DaemonListener::bind(socket_path).unwrap();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());
        let server = Arc::new(DaemonServer::new(4096));
        tokio::spawn(async move { server.run(listener, shutdown_rx).await });
        // Give server time to start
        tokio::time::sleep(Duration::from_millis(50)).await;
        shutdown_tx
    }

    async fn send_recv(stream: &mut BufReader<UnixStream>, cmd: &str) -> Value {
        stream.get_mut().write_all(cmd.as_bytes()).await.unwrap();
        stream.get_mut().write_all(b"\n").await.unwrap();
        let mut line = String::new();
        stream.read_line(&mut line).await.unwrap();
        serde_json::from_str(&line).unwrap()
    }

    async fn connect_control(sock: &std::path::Path) -> BufReader<UnixStream> {
        let mut conn = UnixStream::connect(sock).await.unwrap();
        // Send connection type discriminator: 0x00 = control
        conn.write_all(&[0x00]).await.unwrap();
        BufReader::new(conn)
    }

    #[tokio::test]
    async fn spawn_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut reader = connect_control(&sock).await;

        // Spawn
        let resp = send_recv(
            &mut reader,
            r#"{"cmd":"spawn","session_id":"s1","command":"/bin/sh","args":["-c","sleep 10"]}"#,
        )
        .await;
        assert_eq!(resp["ok"], true);
        assert_eq!(resp["session_id"], "s1");

        // List
        let resp = send_recv(&mut reader, r#"{"cmd":"list"}"#).await;
        let sessions = resp["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["session_id"], "s1");
        assert_eq!(sessions[0]["alive"], true);

        // Kill
        let resp = send_recv(&mut reader, r#"{"cmd":"kill","session_id":"s1"}"#).await;
        assert_eq!(resp["ok"], true);

        // Wait for exit polling, then drain any event messages before sending list
        tokio::time::sleep(Duration::from_millis(800)).await;

        // Send list and read lines until we get the sessions response (skip events)
        reader
            .get_mut()
            .write_all(b"{\"cmd\":\"list\"}\n")
            .await
            .unwrap();
        let resp = loop {
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let val: Value = serde_json::from_str(&line).unwrap();
            if val.get("sessions").is_some() {
                break val;
            }
        };
        let sessions = resp["sessions"].as_array().unwrap();
        // Session is retained as killed (not immediately removed)
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["alive"], false);
        assert_eq!(sessions[0]["status"], "killed");
    }

    #[tokio::test]
    async fn resize_known_session() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut reader = connect_control(&sock).await;

        send_recv(
            &mut reader,
            r#"{"cmd":"spawn","session_id":"r1","command":"cat","args":[]}"#,
        )
        .await;

        let resp = send_recv(
            &mut reader,
            r#"{"cmd":"resize","session_id":"r1","cols":120,"rows":40}"#,
        )
        .await;
        assert_eq!(resp["ok"], true);

        // Kill cleanup
        send_recv(&mut reader, r#"{"cmd":"kill","session_id":"r1"}"#).await;
    }

    #[tokio::test]
    async fn resize_unknown_session_errors() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut reader = connect_control(&sock).await;

        let resp = send_recv(
            &mut reader,
            r#"{"cmd":"resize","session_id":"nope","cols":80,"rows":24}"#,
        )
        .await;
        assert!(resp["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn exited_event_broadcast() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut reader = connect_control(&sock).await;

        // Spawn a short-lived process
        send_recv(
            &mut reader,
            r#"{"cmd":"spawn","session_id":"short","command":"/bin/sh","args":["-c","echo done"]}"#,
        )
        .await;

        // Wait for exit event (poll_exits runs every 500ms + process exit time)
        let mut got_event = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            let mut line = String::new();
            let result =
                tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line)).await;
            if let Ok(Ok(n)) = result {
                if n > 0 {
                    let val: Value = serde_json::from_str(&line).unwrap();
                    if val.get("event").is_some()
                        && val["event"] == "exited"
                        && val["session_id"] == "short"
                    {
                        got_event = true;
                        break;
                    }
                }
            } else {
                break;
            }
        }
        assert!(
            got_event,
            "should receive exited event for short-lived session"
        );
    }

    #[tokio::test]
    async fn invalid_json_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut reader = connect_control(&sock).await;

        let resp = send_recv(&mut reader, r#"not valid json"#).await;
        assert!(!resp["error"].as_str().unwrap().is_empty());
    }

    /// Regression test: after a control connection drops (simulating broken pipe),
    /// a fresh connection to the same daemon should work. This validates the
    /// reconnect-on-failure pattern used in launch_session.
    #[tokio::test]
    async fn reconnect_after_dropped_connection() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        // First connection: spawn a session, then drop the connection (simulates broken pipe)
        {
            let mut reader = connect_control(&sock).await;
            let resp = send_recv(
                &mut reader,
                r#"{"cmd":"spawn","session_id":"reconn1","command":"cat","args":[]}"#,
            )
            .await;
            assert_eq!(resp["ok"], true);
        }
        // Connection dropped here

        tokio::time::sleep(Duration::from_millis(100)).await;

        // Second connection: fresh client should work and see the session
        let mut reader = connect_control(&sock).await;
        let resp = send_recv(&mut reader, r#"{"cmd":"list"}"#).await;
        let sessions = resp["sessions"].as_array().unwrap();
        let found = sessions
            .iter()
            .any(|s| s["session_id"] == "reconn1" && s["alive"] == true);
        assert!(
            found,
            "session should still exist after reconnect: {:?}",
            sessions
        );

        // Should be able to spawn another session on the fresh connection
        let resp = send_recv(
            &mut reader,
            r#"{"cmd":"spawn","session_id":"reconn2","command":"cat","args":[]}"#,
        )
        .await;
        assert_eq!(resp["ok"], true);

        // Cleanup
        send_recv(&mut reader, r#"{"cmd":"kill","session_id":"reconn1"}"#).await;
        send_recv(&mut reader, r#"{"cmd":"kill","session_id":"reconn2"}"#).await;
    }

    #[tokio::test]
    async fn read_buffer_returns_stripped_text() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut reader = connect_control(&sock).await;

        // Spawn a session that outputs known text
        let resp = send_recv(
            &mut reader,
            r#"{"cmd":"spawn","session_id":"rb1","command":"/bin/sh","args":["-c","echo hello-world"]}"#,
        )
        .await;
        assert_eq!(resp["ok"], true);

        // Wait for output to land in buffer
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Read buffer — drain any event messages (e.g. exited) until we get the response
        reader
            .get_mut()
            .write_all(b"{\"cmd\":\"read_buffer\",\"session_id\":\"rb1\",\"lines\":10}\n")
            .await
            .unwrap();
        let resp = loop {
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let val: Value = serde_json::from_str(&line).unwrap();
            if val.get("event").is_none() {
                break val;
            }
        };

        assert_eq!(resp["ok"], true);
        let text = resp["text"].as_str().unwrap();
        assert!(
            text.contains("hello-world"),
            "read_buffer should contain 'hello-world', got: {text:?}"
        );
        // Should not contain ANSI escape codes
        assert!(
            !text.contains("\x1b["),
            "read_buffer should strip ANSI escapes, got: {text:?}"
        );
    }

    #[tokio::test]
    async fn read_buffer_after_returns_incremental_output() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut reader = connect_control(&sock).await;

        // Spawn a session that outputs known text
        let resp = send_recv(
            &mut reader,
            r#"{"cmd":"spawn","session_id":"rba1","command":"/bin/sh","args":["-c","echo first-line"]}"#,
        )
        .await;
        assert_eq!(resp["ok"], true);

        // Wait for output
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Initial cursor read (after=0 gets everything)
        reader
            .get_mut()
            .write_all(b"{\"cmd\":\"read_buffer_after\",\"session_id\":\"rba1\",\"after\":0,\"max_bytes\":0}\n")
            .await
            .unwrap();
        let resp = loop {
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let val: Value = serde_json::from_str(&line).unwrap();
            if val.get("event").is_none() {
                break val;
            }
        };

        assert_eq!(resp["ok"], true);
        assert!(!resp["truncated"].as_bool().unwrap());
        let cursor = resp["cursor"].as_u64().unwrap();
        assert!(cursor > 0, "cursor should be non-zero after output");
        let text = resp["text"].as_str().unwrap();
        assert!(
            text.contains("first-line"),
            "should contain 'first-line', got: {text:?}"
        );

        // Read again with the cursor — should get empty since no new output
        let cmd = format!(
            "{{\"cmd\":\"read_buffer_after\",\"session_id\":\"rba1\",\"after\":{cursor},\"max_bytes\":0}}\n"
        );
        reader
            .get_mut()
            .write_all(cmd.as_bytes())
            .await
            .unwrap();
        let resp = loop {
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let val: Value = serde_json::from_str(&line).unwrap();
            if val.get("event").is_none() {
                break val;
            }
        };

        assert_eq!(resp["ok"], true);
        assert!(!resp["truncated"].as_bool().unwrap());
        let text2 = resp["text"].as_str().unwrap();
        // No new output, should be empty or very short (no new content)
        assert!(
            !text2.contains("first-line"),
            "second read should NOT re-read 'first-line', got: {text2:?}"
        );
    }

    #[tokio::test]
    async fn read_buffer_after_invalid_session_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut reader = connect_control(&sock).await;

        let resp = send_recv(
            &mut reader,
            r#"{"cmd":"read_buffer_after","session_id":"nonexistent","after":0,"max_bytes":0}"#,
        )
        .await;
        assert!(resp["error"].as_str().unwrap().contains("session not found"));
    }
}
