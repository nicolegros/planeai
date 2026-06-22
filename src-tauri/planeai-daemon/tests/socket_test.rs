#[cfg(unix)]
mod pipe_busy_regression {
    use planeai_daemon::server::DaemonServer;
    use planeai_daemon::transport::{DaemonListener, DaemonStream};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::AsyncWriteExt;

    /// Regression test for PLA-139: rapid sequential connects must not fail.
    /// On Windows this triggers ERROR_PIPE_BUSY (os error 231) if the client
    /// doesn't retry. On Unix this validates the same code path works under load.
    #[tokio::test]
    async fn rapid_sequential_connects_succeed() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");

        let listener = DaemonListener::bind(&sock).unwrap();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());
        let server = Arc::new(DaemonServer::new(4096));
        tokio::spawn(async move { server.run(listener, shutdown_rx).await });
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Rapidly open 10 connections using AsyncIpcStream::connect (the fixed path)
        for i in 0..10 {
            let mut stream = DaemonStream::connect(&sock).await.unwrap_or_else(|e| {
                panic!("connect #{i} failed: {e}");
            });
            // Send control byte to prove it's a working connection
            stream.write_all(&[0x00]).await.unwrap();
            drop(stream);
        }

        drop(shutdown_tx);
    }
}

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
        assert_eq!(sessions.len(), 0);
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
}
