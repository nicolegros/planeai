//! Daemon client — connects to planeai-daemon over Unix socket.
//! Used by both the GUI backend and the CLI.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::paths;

const CONNECT_RETRIES: u32 = 10;
const RETRY_DELAY: Duration = Duration::from_millis(100);

pub fn daemon_socket_path() -> PathBuf {
    paths::app_data_dir().join("daemon.sock")
}

fn daemon_pid_path() -> PathBuf {
    paths::app_data_dir().join("daemon.pid")
}

/// Ensure the daemon is running; start it if not.
pub fn ensure_daemon() -> Result<(), String> {
    if daemon_socket_path().exists() {
        // Try a ping to verify it's alive
        if let Ok(mut conn) = UnixStream::connect(daemon_socket_path()) {
            conn.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let _ = conn.write_all(b"{\"cmd\":\"ping\"}\n");
            let mut reader = BufReader::new(&conn);
            let mut line = String::new();
            if reader.read_line(&mut line).is_ok() && line.contains("pong") {
                return Ok(());
            }
        }
        // Stale socket — remove it
        let _ = std::fs::remove_file(daemon_socket_path());
    }

    start_daemon()?;

    // Wait for socket to appear
    for _ in 0..CONNECT_RETRIES {
        std::thread::sleep(RETRY_DELAY);
        if daemon_socket_path().exists() {
            return Ok(());
        }
    }
    Err("daemon did not start in time".to_string())
}

fn start_daemon() -> Result<(), String> {
    // Look for the daemon binary next to ourselves, or on PATH
    let daemon_bin = find_daemon_binary()?;
    Command::new(&daemon_bin)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn daemon: {e}"))?;
    Ok(())
}

fn find_daemon_binary() -> Result<PathBuf, String> {
    // 1. Next to current executable (sidecar pattern)
    if let Ok(exe) = std::env::current_exe() {
        let dir = exe.parent().unwrap_or(std::path::Path::new("."));
        let candidate = dir.join("planeai-daemon");
        if candidate.exists() {
            return Ok(candidate);
        }
        // Tauri sidecar naming pattern: planeai-daemon-{target_triple}
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("planeai-daemon") {
                return Ok(entry.path());
            }
        }
    }
    // 2. On PATH
    Ok(PathBuf::from("planeai-daemon"))
}

/// A connection to the daemon control socket.
pub struct DaemonConn {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl DaemonConn {
    pub fn connect() -> Result<Self, String> {
        let stream =
            UnixStream::connect(daemon_socket_path()).map_err(|e| format!("connect: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        Ok(Self { stream, reader })
    }

    pub fn send(&mut self, json: &str) -> Result<String, String> {
        self.stream
            .write_all(json.as_bytes())
            .map_err(|e| format!("write: {e}"))?;
        self.stream
            .write_all(b"\n")
            .map_err(|e| format!("write newline: {e}"))?;
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .map_err(|e| format!("read: {e}"))?;
        Ok(line)
    }

    /// Create a session in the daemon.
    pub fn create_session(
        &mut self,
        session_id: &str,
        command: &str,
        args: &[String],
        cwd: &str,
        env: &[(String, String)],
    ) -> Result<(), String> {
        let req = serde_json::json!({
            "cmd": "create_session",
            "session_id": session_id,
            "command": command,
            "args": args,
            "cwd": cwd,
            "env": env,
        });
        let resp = self.send(&req.to_string())?;
        if resp.contains("\"error\"") {
            Err(resp.trim().to_string())
        } else {
            Ok(())
        }
    }

    /// Write input to a daemon session.
    pub fn write_to_session(&mut self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let encoded = base64_encode(data);
        let req = serde_json::json!({
            "cmd": "write",
            "session_id": session_id,
            "data": encoded,
        });
        let resp = self.send(&req.to_string())?;
        if resp.contains("\"error\"") {
            Err(resp.trim().to_string())
        } else {
            Ok(())
        }
    }

    /// Resize a daemon session.
    pub fn resize_session(&mut self, session_id: &str, rows: u16, cols: u16) -> Result<(), String> {
        let req = serde_json::json!({
            "cmd": "resize",
            "session_id": session_id,
            "rows": rows,
            "cols": cols,
        });
        let resp = self.send(&req.to_string())?;
        if resp.contains("\"error\"") {
            Err(resp.trim().to_string())
        } else {
            Ok(())
        }
    }

    /// Kill a daemon session.
    pub fn kill_session(&mut self, session_id: &str) -> Result<(), String> {
        let req = serde_json::json!({
            "cmd": "kill",
            "session_id": session_id,
        });
        let resp = self.send(&req.to_string())?;
        if resp.contains("\"error\"") {
            Err(resp.trim().to_string())
        } else {
            Ok(())
        }
    }

    /// List daemon sessions.
    pub fn list_sessions(&mut self) -> Result<String, String> {
        let req = r#"{"cmd":"list"}"#;
        self.send(req)
    }

    /// Attach to a session's data stream. Returns the raw stream for reading binary frames.
    /// After this call, the connection is in "data mode" — reads are binary frames.
    pub fn attach(mut self, session_id: &str) -> Result<DataStream, String> {
        let req = serde_json::json!({
            "cmd": "attach",
            "session_id": session_id,
        });
        self.stream
            .write_all(format!("{}\n", req).as_bytes())
            .map_err(|e| format!("write: {e}"))?;
        // After attach, the daemon sends binary data frames (4-byte len + data)
        // and eventually a JSON line for session_exited
        self.stream.set_read_timeout(None).ok();
        Ok(DataStream {
            stream: self.stream,
        })
    }
}

/// A data stream from an attached daemon session.
/// Reads length-prefixed binary frames.
pub struct DataStream {
    stream: UnixStream,
}

impl DataStream {
    /// Read next frame. Returns None if connection closed or session exited.
    pub fn read_frame(&mut self) -> Option<Vec<u8>> {
        let mut len_buf = [0u8; 4];
        if self.stream.read_exact(&mut len_buf).is_err() {
            return None;
        }
        let len = u32::from_be_bytes(len_buf) as usize;
        if len == 0 {
            return None;
        }
        let mut data = vec![0u8; len];
        if self.stream.read_exact(&mut data).is_err() {
            return None;
        }
        Some(data)
    }

    pub fn try_clone(&self) -> Result<Self, String> {
        Ok(Self {
            stream: self.stream.try_clone().map_err(|e| e.to_string())?,
        })
    }
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}
