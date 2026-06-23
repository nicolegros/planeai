//! Shared sync helpers for spawning sessions in the daemon process.

use std::io::{Read, Write};
use std::path::Path;

/// Ensure the daemon process is running. Spawns it if not reachable.
pub fn ensure_running(
    daemon_bin: &Path,
    socket_path: &Path,
    scrollback_bytes: usize,
) -> Result<(), String> {
    let app_dir = crate::paths::app_data_dir();
    if planeai_ipc::connect(planeai_ipc::Channel::Daemon, &app_dir).is_ok() {
        return Ok(());
    }

    let _ = std::fs::remove_file(socket_path);
    if let Some(parent) = socket_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    std::process::Command::new(daemon_bin)
        .arg("--socket-path")
        .arg(socket_path)
        .arg("--scrollback-bytes")
        .arg(scrollback_bytes.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn daemon: {e}"))?;

    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if planeai_ipc::connect(planeai_ipc::Channel::Daemon, &app_dir).is_ok() {
            return Ok(());
        }
    }
    Err("daemon did not start within 3 seconds".to_string())
}

/// Spawn a session in the daemon. Assumes daemon is already running.
/// Splits the command string into program + args for direct argv execution.
pub fn spawn_session(
    session_id: &str,
    cmd: &str,
    cwd: &str,
    env: Option<&std::collections::HashMap<&str, &str>>,
) -> Result<(), String> {
    let mut parts = cmd.split_whitespace();
    let program = parts.next().unwrap_or(cmd);
    let args: Vec<&str> = parts.collect();
    let app_dir = crate::paths::app_data_dir();

    let mut stream = planeai_ipc::connect(planeai_ipc::Channel::Daemon, &app_dir)
        .map_err(|e| format!("daemon connect failed: {e}"))?;
    stream
        .write_all(&[0x00])
        .map_err(|e| format!("handshake failed: {e}"))?;

    let req = serde_json::json!({
        "cmd": "spawn",
        "session_id": session_id,
        "command": program,
        "args": args,
        "cwd": cwd,
        "env": env,
        "mode": "replace_exited",
    });
    let payload = format!(
        "{}\n",
        serde_json::to_string(&req).map_err(|e| e.to_string())?
    );
    stream
        .write_all(payload.as_bytes())
        .map_err(|e| format!("send failed: {e}"))?;

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    let resp: serde_json::Value =
        serde_json::from_slice(&buf[..n]).map_err(|e| format!("invalid response: {e}"))?;

    if let Some(err) = resp.get("error") {
        return Err(err.as_str().unwrap_or("unknown error").to_string());
    }
    Ok(())
}
