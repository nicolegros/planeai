//! Regression test for PLA-139: concurrent clients must all connect
//! successfully even when the server creates one pipe instance at a time.
//!
//! On Windows this exercises the ERROR_PIPE_BUSY retry logic in
//! AsyncIpcStream::connect(). On Unix it validates the general contract
//! that N concurrent connects succeed against a sequential accept loop.

use planeai_ipc::r#async::{AsyncIpcListener, AsyncIpcStream};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn test_socket_path() -> PathBuf {
    #[cfg(unix)]
    {
        let dir = tempfile::tempdir().unwrap();
        // Leak the tempdir so it lives until process exit
        let path = dir.path().join("test.sock");
        std::mem::forget(dir);
        path
    }
    #[cfg(windows)]
    {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        PathBuf::from(format!(r"\\.\pipe\planeai-test-retry-{}", id))
    }
}

/// Multiple clients connecting concurrently must all succeed.
/// This reproduces the pipe-busy race on Windows.
#[tokio::test]
async fn concurrent_connects_all_succeed() {
    let path = test_socket_path();
    let listener = AsyncIpcListener::bind(&path).unwrap();

    const NUM_CLIENTS: usize = 5;

    // Server: accept NUM_CLIENTS connections sequentially (with a small delay
    // between accepts to widen the pipe-busy window on Windows).
    let server_path = path.clone();
    let server = tokio::spawn(async move {
        let _ = server_path; // keep path alive
        for _ in 0..NUM_CLIENTS {
            let mut stream = listener.accept().await.unwrap();
            // Echo one byte back to confirm the connection is live
            let mut buf = [0u8; 1];
            stream.read_exact(&mut buf).await.unwrap();
            stream.write_all(&buf).await.unwrap();
            // Small delay to simulate real accept-loop overhead and widen the
            // pipe-busy window on Windows
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    });

    // Clients: connect concurrently (all at once)
    // Small initial delay to let the server bind
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    let mut handles = Vec::new();
    for i in 0..NUM_CLIENTS {
        let p = path.clone();
        handles.push(tokio::spawn(async move {
            let mut stream = AsyncIpcStream::connect(&p).await.unwrap_or_else(|e| {
                panic!("client {i} failed to connect: {e}");
            });
            stream.write_all(&[i as u8]).await.unwrap();
            let mut buf = [0u8; 1];
            stream.read_exact(&mut buf).await.unwrap();
            assert_eq!(buf[0], i as u8);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
    server.await.unwrap();
}
