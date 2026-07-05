#[cfg(unix)]
mod data_tests {
    use planeai_daemon::protocol::{
        read_frame, write_frame, CONN_CONTROL, CONN_DATA, FRAME_INPUT, FRAME_OUTPUT, FRAME_RESIZE,
    };
    use planeai_daemon::server::DaemonServer;
    use planeai_daemon::transport::DaemonListener;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    async fn start_server(socket_path: &std::path::Path) -> tokio::sync::watch::Sender<()> {
        let listener = DaemonListener::bind(socket_path).unwrap();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());
        let server = Arc::new(DaemonServer::new(4096));
        tokio::spawn(async move { server.run(listener, shutdown_rx).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        shutdown_tx
    }

    async fn connect_control(sock: &std::path::Path) -> BufReader<UnixStream> {
        let mut conn = UnixStream::connect(sock).await.unwrap();
        conn.write_all(&[CONN_CONTROL]).await.unwrap();
        BufReader::new(conn)
    }

    async fn connect_data(sock: &std::path::Path, session_id: &str) -> UnixStream {
        let mut conn = UnixStream::connect(sock).await.unwrap();
        conn.write_all(&[CONN_DATA]).await.unwrap();
        // Send handshake frame: FRAME_OUTPUT with session_id
        write_frame(&mut conn, FRAME_OUTPUT, session_id.as_bytes())
            .await
            .unwrap();
        conn
    }

    async fn spawn_session(reader: &mut BufReader<UnixStream>, id: &str, cmd: &str, args: &str) {
        let msg =
            format!(r#"{{"cmd":"spawn","session_id":"{id}","command":"{cmd}","args":[{args}]}}"#);
        reader.get_mut().write_all(msg.as_bytes()).await.unwrap();
        reader.get_mut().write_all(b"\n").await.unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
    }

    async fn kill_session(reader: &mut BufReader<UnixStream>, id: &str) {
        let msg = format!(r#"{{"cmd":"kill","session_id":"{id}"}}"#);
        reader.get_mut().write_all(msg.as_bytes()).await.unwrap();
        reader.get_mut().write_all(b"\n").await.unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
    }

    // --- Frame protocol unit tests ---

    #[tokio::test]
    async fn frame_write_read_roundtrip() {
        let (mut client, mut server) = tokio::io::duplex(1024);
        let payload = b"hello world";

        write_frame(&mut client, FRAME_OUTPUT, payload)
            .await
            .unwrap();
        let (ft, data) = read_frame(&mut server).await.unwrap();

        assert_eq!(ft, FRAME_OUTPUT);
        assert_eq!(data, payload);
    }

    #[tokio::test]
    async fn frame_write_read_input_type() {
        let (mut client, mut server) = tokio::io::duplex(1024);
        let payload = b"keystroke";

        write_frame(&mut client, FRAME_INPUT, payload)
            .await
            .unwrap();
        let (ft, data) = read_frame(&mut server).await.unwrap();

        assert_eq!(ft, FRAME_INPUT);
        assert_eq!(data, payload);
    }

    #[tokio::test]
    async fn frame_empty_payload() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        write_frame(&mut client, FRAME_OUTPUT, b"").await.unwrap();
        let (ft, data) = read_frame(&mut server).await.unwrap();

        assert_eq!(ft, FRAME_OUTPUT);
        assert!(data.is_empty());
    }

    #[tokio::test]
    async fn frame_large_payload() {
        let (mut client, mut server) = tokio::io::duplex(256 * 1024);
        let payload = vec![0xAB; 100_000];

        write_frame(&mut client, FRAME_OUTPUT, &payload)
            .await
            .unwrap();
        let (ft, data) = read_frame(&mut server).await.unwrap();

        assert_eq!(ft, FRAME_OUTPUT);
        assert_eq!(data.len(), 100_000);
        assert_eq!(data, payload);
    }

    #[tokio::test]
    async fn frame_partial_reads() {
        // Simulate TCP-style partial delivery using a tiny duplex buffer
        let (mut client, mut server) = tokio::io::duplex(8); // very small buffer forces fragmentation
        let payload = b"this is a longer payload for partial read test";

        let write_handle = tokio::spawn(async move {
            write_frame(&mut client, FRAME_INPUT, payload)
                .await
                .unwrap();
        });

        let (ft, data) = read_frame(&mut server).await.unwrap();
        write_handle.await.unwrap();

        assert_eq!(ft, FRAME_INPUT);
        assert_eq!(data, payload);
    }

    #[tokio::test]
    async fn frame_multiple_frames_sequential() {
        let (mut client, mut server) = tokio::io::duplex(4096);

        write_frame(&mut client, FRAME_OUTPUT, b"first")
            .await
            .unwrap();
        write_frame(&mut client, FRAME_INPUT, b"second")
            .await
            .unwrap();
        write_frame(&mut client, FRAME_OUTPUT, b"third")
            .await
            .unwrap();

        let (t1, d1) = read_frame(&mut server).await.unwrap();
        let (t2, d2) = read_frame(&mut server).await.unwrap();
        let (t3, d3) = read_frame(&mut server).await.unwrap();

        assert_eq!((t1, &d1[..]), (FRAME_OUTPUT, &b"first"[..]));
        assert_eq!((t2, &d2[..]), (FRAME_INPUT, &b"second"[..]));
        assert_eq!((t3, &d3[..]), (FRAME_OUTPUT, &b"third"[..]));
    }

    // --- Integration tests ---

    #[tokio::test]
    async fn data_connection_receives_output() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut ctrl = connect_control(&sock).await;
        spawn_session(&mut ctrl, "out1", "/bin/sh", r#""-c","echo hello""#).await;

        // Wait for output to be produced
        tokio::time::sleep(Duration::from_millis(300)).await;

        let mut data = connect_data(&sock, "out1").await;

        // Read frames until we find "hello"
        let mut collected = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(500), read_frame(&mut data)).await {
                Ok(Ok((ft, payload))) => {
                    assert_eq!(ft, FRAME_OUTPUT);
                    collected.extend_from_slice(&payload);
                    if String::from_utf8_lossy(&collected).contains("hello") {
                        break;
                    }
                }
                _ => break,
            }
        }
        assert!(
            String::from_utf8_lossy(&collected).contains("hello"),
            "expected 'hello' in output, got: {:?}",
            String::from_utf8_lossy(&collected)
        );

        kill_session(&mut ctrl, "out1").await;
    }

    #[tokio::test]
    async fn scrollback_replay() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut ctrl = connect_control(&sock).await;
        // Use a session that outputs then stays alive
        spawn_session(
            &mut ctrl,
            "replay1",
            "/bin/sh",
            r#""-c","echo replay_marker && sleep 30""#,
        )
        .await;

        // Wait for output to land in buffer
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Connect data — should get replay containing "replay_marker"
        let mut data = connect_data(&sock, "replay1").await;

        let mut collected = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(500), read_frame(&mut data)).await {
                Ok(Ok((_, payload))) => {
                    collected.extend_from_slice(&payload);
                    if String::from_utf8_lossy(&collected).contains("replay_marker") {
                        break;
                    }
                }
                _ => break,
            }
        }
        assert!(
            String::from_utf8_lossy(&collected).contains("replay_marker"),
            "replay should contain 'replay_marker', got: {:?}",
            String::from_utf8_lossy(&collected)
        );

        kill_session(&mut ctrl, "replay1").await;
    }

    #[tokio::test]
    async fn input_delivery() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut ctrl = connect_control(&sock).await;
        spawn_session(&mut ctrl, "cat1", "cat", "").await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let mut data = connect_data(&sock, "cat1").await;

        // Send input
        write_frame(&mut data, FRAME_INPUT, b"hello\n")
            .await
            .unwrap();

        // Read output - cat should echo it back
        let mut collected = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(500), read_frame(&mut data)).await {
                Ok(Ok((_, payload))) => {
                    collected.extend_from_slice(&payload);
                    if String::from_utf8_lossy(&collected).contains("hello") {
                        break;
                    }
                }
                _ => break,
            }
        }
        assert!(
            String::from_utf8_lossy(&collected).contains("hello"),
            "cat should echo 'hello', got: {:?}",
            String::from_utf8_lossy(&collected)
        );

        kill_session(&mut ctrl, "cat1").await;
    }

    #[tokio::test]
    async fn multiple_clients_receive_output() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut ctrl = connect_control(&sock).await;
        spawn_session(&mut ctrl, "multi1", "cat", "").await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let mut data1 = connect_data(&sock, "multi1").await;
        let mut data2 = connect_data(&sock, "multi1").await;

        // Send input via client 1
        write_frame(&mut data1, FRAME_INPUT, b"multi_test\n")
            .await
            .unwrap();

        // Both should receive the output
        let mut c1 = Vec::new();
        let mut c2 = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            tokio::select! {
                result = read_frame(&mut data1) => {
                    if let Ok((_, payload)) = result {
                        c1.extend_from_slice(&payload);
                    }
                }
                result = read_frame(&mut data2) => {
                    if let Ok((_, payload)) = result {
                        c2.extend_from_slice(&payload);
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(500)) => {}
            }
            if String::from_utf8_lossy(&c1).contains("multi_test")
                && String::from_utf8_lossy(&c2).contains("multi_test")
            {
                break;
            }
        }
        assert!(
            String::from_utf8_lossy(&c1).contains("multi_test"),
            "client 1 should see output"
        );
        assert!(
            String::from_utf8_lossy(&c2).contains("multi_test"),
            "client 2 should see output"
        );

        kill_session(&mut ctrl, "multi1").await;
    }

    #[tokio::test]
    async fn client_disconnect_others_survive() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut ctrl = connect_control(&sock).await;
        spawn_session(&mut ctrl, "disc1", "cat", "").await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let data1 = connect_data(&sock, "disc1").await;
        let mut data2 = connect_data(&sock, "disc1").await;

        // Drop client 1
        drop(data1);
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Client 2 should still work
        write_frame(&mut data2, FRAME_INPUT, b"still_alive\n")
            .await
            .unwrap();

        let mut collected = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(500), read_frame(&mut data2)).await {
                Ok(Ok((_, payload))) => {
                    collected.extend_from_slice(&payload);
                    if String::from_utf8_lossy(&collected).contains("still_alive") {
                        break;
                    }
                }
                _ => break,
            }
        }
        assert!(
            String::from_utf8_lossy(&collected).contains("still_alive"),
            "client 2 should still receive output after client 1 disconnects"
        );

        kill_session(&mut ctrl, "disc1").await;
    }

    #[tokio::test]
    async fn session_exit_closes_data_connection() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut ctrl = connect_control(&sock).await;
        spawn_session(&mut ctrl, "exit1", "/bin/sh", r#""-c","echo bye""#).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let mut data = connect_data(&sock, "exit1").await;

        // Read frames until EOF (session already exited)
        let mut got_eof = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(500), read_frame(&mut data)).await {
                Ok(Ok(_)) => continue, // consume replay frames
                Ok(Err(_)) => {
                    got_eof = true;
                    break;
                }
                Err(_) => break, // timeout
            }
        }
        assert!(got_eof, "data connection should close after session exits");
    }

    #[tokio::test]
    async fn connection_type_routing() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        // Control connection works
        let mut ctrl = connect_control(&sock).await;
        ctrl.get_mut()
            .write_all(b"{\"cmd\":\"list\"}\n")
            .await
            .unwrap();
        let mut line = String::new();
        ctrl.read_line(&mut line).await.unwrap();
        let val: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert!(val.get("sessions").is_some());

        // Data connection to nonexistent session gets closed
        let mut conn = UnixStream::connect(&sock).await.unwrap();
        conn.write_all(&[CONN_DATA]).await.unwrap();
        // Send handshake with nonexistent session
        write_frame(&mut conn, FRAME_OUTPUT, b"nonexistent")
            .await
            .unwrap();
        // Should get EOF quickly
        let result = tokio::time::timeout(Duration::from_secs(1), read_frame(&mut conn)).await;
        match result {
            Ok(Err(_)) => {} // expected: connection closed
            Err(_) => {}     // timeout is also acceptable (server closed silently)
            Ok(Ok(_)) => panic!("should not receive frames for nonexistent session"),
        }
    }

    #[tokio::test]
    async fn rapid_connect_disconnect_no_panic() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut ctrl = connect_control(&sock).await;
        spawn_session(&mut ctrl, "rapid1", "cat", "").await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Rapidly connect and disconnect 20 data connections
        for _ in 0..20 {
            let conn = connect_data(&sock, "rapid1").await;
            drop(conn);
        }

        // Server should still work
        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut data = connect_data(&sock, "rapid1").await;
        write_frame(&mut data, FRAME_INPUT, b"ok\n").await.unwrap();

        let mut collected = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(500), read_frame(&mut data)).await {
                Ok(Ok((_, payload))) => {
                    collected.extend_from_slice(&payload);
                    if String::from_utf8_lossy(&collected).contains("ok") {
                        break;
                    }
                }
                _ => break,
            }
        }
        assert!(
            String::from_utf8_lossy(&collected).contains("ok"),
            "server still functional after rapid connect/disconnect"
        );

        kill_session(&mut ctrl, "rapid1").await;
    }

    /// Regression test: resize via FRAME_RESIZE on the data connection.
    /// Previously, resize opened a separate control connection per event,
    /// causing FD exhaustion (os error 24) under rapid resize.
    #[tokio::test]
    async fn resize_via_data_connection_frame() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let _shutdown = start_server(&sock).await;

        let mut ctrl = connect_control(&sock).await;
        spawn_session(&mut ctrl, "resize_data1", "cat", "").await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let mut data = connect_data(&sock, "resize_data1").await;

        // Send 50 rapid resize frames through the data connection (simulates window drag)
        for i in 0..50u16 {
            let cols: u16 = 80 + i;
            let rows: u16 = 24;
            let mut payload = [0u8; 4];
            payload[0..2].copy_from_slice(&cols.to_be_bytes());
            payload[2..4].copy_from_slice(&rows.to_be_bytes());
            write_frame(&mut data, FRAME_RESIZE, &payload)
                .await
                .unwrap();
        }

        // Verify the session is still alive and responsive after rapid resizes
        write_frame(&mut data, FRAME_INPUT, b"alive\n")
            .await
            .unwrap();

        let mut collected = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(500), read_frame(&mut data)).await {
                Ok(Ok((_, payload))) => {
                    collected.extend_from_slice(&payload);
                    if String::from_utf8_lossy(&collected).contains("alive") {
                        break;
                    }
                }
                _ => break,
            }
        }
        assert!(
            String::from_utf8_lossy(&collected).contains("alive"),
            "session should still work after rapid FRAME_RESIZE, got: {:?}",
            String::from_utf8_lossy(&collected)
        );

        kill_session(&mut ctrl, "resize_data1").await;
    }
}
