//! Session log catalog backend.
//!
//! Provides Tauri commands to discover, read metadata, and stream chunks of
//! durable session logs stored under `PLANEAI_SESSION_LOG_DIR`.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::pty_planeai_core_adapter::{session_log_dir, SessionMeta};

/// Catalog entry returned to the frontend.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SessionLogEntry {
    pub session_id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub pty_core: String,
    pub ansi_log_path: String,
    pub meta_path: String,
    pub bytes_written: u64,
    pub bytes_dropped: u64,
    pub command: String,
    pub cwd: String,
}

/// Validate that `path` is strictly under `base`. Prevents path traversal.
fn is_under(base: &Path, path: &Path) -> bool {
    match (fs::canonicalize(base), fs::canonicalize(path)) {
        (Ok(b), Ok(p)) => p.starts_with(b),
        // If base doesn't exist yet, deny
        _ => false,
    }
}

fn log_base_dir() -> Option<PathBuf> {
    session_log_dir().map(|d| d.join("sessions"))
}

#[tauri::command]
pub fn get_session_log_dir() -> Result<String, String> {
    session_log_dir()
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| "PLANEAI_SESSION_LOG_DIR not set".to_string())
}

#[tauri::command]
pub fn is_dogfood_log_viewer_enabled() -> bool {
    std::env::var("PLANEAI_DOGFOOD_LOG_VIEWER")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false)
}

#[tauri::command]
pub fn list_session_logs() -> Result<Vec<SessionLogEntry>, String> {
    let base = log_base_dir().ok_or("PLANEAI_SESSION_LOG_DIR not set")?;
    if !base.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    let read_dir = fs::read_dir(&base).map_err(|e| format!("cannot read log dir: {e}"))?;
    for dir_entry in read_dir.flatten() {
        if !dir_entry.path().is_dir() {
            continue;
        }
        let session_dir = dir_entry.path();
        let meta_path = session_dir.join("meta.json");
        if let Some(entry) = read_catalog_entry(&session_dir, &meta_path) {
            entries.push(entry);
        }
    }
    // Most recent first
    entries.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(entries)
}

fn read_catalog_entry(session_dir: &Path, meta_path: &Path) -> Option<SessionLogEntry> {
    let content = fs::read_to_string(meta_path).ok()?;
    let meta: SessionMeta = serde_json::from_str(&content).ok()?;
    let ansi_log_path = session_dir.join(&meta.ansi_log_file);
    Some(SessionLogEntry {
        session_id: meta.session_id,
        started_at: meta.started_at,
        ended_at: meta.ended_at,
        status: meta.status,
        pty_core: meta.pty_core,
        ansi_log_path: ansi_log_path.to_string_lossy().to_string(),
        meta_path: meta_path.to_string_lossy().to_string(),
        bytes_written: meta.bytes_written,
        bytes_dropped: meta.bytes_dropped,
        command: meta.command,
        cwd: meta.cwd,
    })
}

#[tauri::command]
pub fn get_session_log_metadata(session_id: String) -> Result<SessionLogEntry, String> {
    let base = log_base_dir().ok_or("PLANEAI_SESSION_LOG_DIR not set")?;
    let session_dir = base.join(&session_id);
    if !is_under(&base, &session_dir) {
        return Err("invalid session id".to_string());
    }
    let meta_path = session_dir.join("meta.json");
    read_catalog_entry(&session_dir, &meta_path)
        .ok_or_else(|| "metadata not found or corrupt".to_string())
}

#[tauri::command]
pub fn read_session_log_chunk(
    path: String,
    offset: u64,
    length: u32,
) -> Result<Vec<u8>, String> {
    let base = log_base_dir().ok_or("PLANEAI_SESSION_LOG_DIR not set")?;
    let file_path = PathBuf::from(&path);
    if !is_under(&base, &file_path) {
        return Err("path traversal denied".to_string());
    }
    let mut file = fs::File::open(&file_path).map_err(|e| format!("open failed: {e}"))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("seek failed: {e}"))?;
    // Cap at 256 KiB per chunk
    let len = (length as usize).min(256 * 1024);
    let mut buf = vec![0u8; len];
    let n = file.read(&mut buf).map_err(|e| format!("read failed: {e}"))?;
    buf.truncate(n);
    Ok(buf)
}

#[tauri::command]
pub fn open_session_log_folder(path: String) -> Result<(), String> {
    let base = log_base_dir().ok_or("PLANEAI_SESSION_LOG_DIR not set")?;
    let target = PathBuf::from(&path);
    if !is_under(&base, &target) {
        return Err("path traversal denied".to_string());
    }
    let dir = if target.is_file() {
        target.parent().unwrap_or(&target).to_path_buf()
    } else {
        target
    };
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(&dir).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(&dir).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(&dir).spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_log_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::env::set_var("PLANEAI_SESSION_LOG_DIR", dir.path());
        let sessions_dir = dir.path().join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        dir
    }

    #[test]
    fn list_handles_empty_dir() {
        let dir = setup_log_dir();
        let result = list_session_logs().unwrap();
        assert!(result.is_empty());
        drop(dir);
    }

    #[test]
    fn list_handles_missing_sessions_subdir() {
        let dir = TempDir::new().unwrap();
        std::env::set_var("PLANEAI_SESSION_LOG_DIR", dir.path());
        // Don't create sessions/ subdir
        let result = list_session_logs().unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn list_parses_valid_meta() {
        let dir = setup_log_dir();
        let session_dir = dir.path().join("sessions").join("test-session-1");
        fs::create_dir_all(&session_dir).unwrap();
        let meta = SessionMeta {
            schema_version: 1,
            session_id: "test-session-1".to_string(),
            pty_core: "planeai-pty".to_string(),
            started_at: "2026-06-17T19:00:00+00:00".to_string(),
            ended_at: Some("2026-06-17T19:05:00+00:00".to_string()),
            command: "echo hi".to_string(),
            cwd: "/tmp".to_string(),
            cols: 80,
            rows: 24,
            ansi_log_file: "20260617T190000Z_output.ansi".to_string(),
            bytes_written: 100,
            bytes_dropped: 0,
            exit_status: Some(0),
            status: "exited".to_string(),
        };
        fs::write(
            session_dir.join("meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();
        fs::write(session_dir.join("20260617T190000Z_output.ansi"), b"hello").unwrap();

        let result = list_session_logs().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].session_id, "test-session-1");
        assert_eq!(result[0].status, "exited");
        assert_eq!(result[0].bytes_written, 100);
        drop(dir);
    }

    #[test]
    fn list_ignores_corrupt_meta() {
        let dir = setup_log_dir();
        let session_dir = dir.path().join("sessions").join("corrupt");
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(session_dir.join("meta.json"), "not valid json{{{").unwrap();
        let result = list_session_logs().unwrap();
        assert!(result.is_empty());
        drop(dir);
    }

    #[test]
    fn chunk_read_returns_expected_bytes() {
        let dir = setup_log_dir();
        let session_dir = dir.path().join("sessions").join("chunk-test");
        fs::create_dir_all(&session_dir).unwrap();
        let ansi_path = session_dir.join("output.ansi");
        let data: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
        fs::write(&ansi_path, &data).unwrap();

        let chunk =
            read_session_log_chunk(ansi_path.to_string_lossy().to_string(), 100, 50).unwrap();
        assert_eq!(chunk.len(), 50);
        assert_eq!(chunk, &data[100..150]);
        drop(dir);
    }

    #[test]
    fn chunk_read_rejects_path_traversal() {
        let dir = setup_log_dir();
        let result = read_session_log_chunk("/etc/passwd".to_string(), 0, 100);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("path traversal denied"));
        drop(dir);
    }

    #[test]
    fn metadata_get_rejects_traversal() {
        let dir = setup_log_dir();
        let result = get_session_log_metadata("../../etc".to_string());
        assert!(result.is_err());
        drop(dir);
    }

    #[test]
    fn chunk_read_caps_at_256kib() {
        let dir = setup_log_dir();
        let session_dir = dir.path().join("sessions").join("big-chunk");
        fs::create_dir_all(&session_dir).unwrap();
        let ansi_path = session_dir.join("output.ansi");
        let data = vec![0xABu8; 512 * 1024];
        fs::write(&ansi_path, &data).unwrap();

        let chunk = read_session_log_chunk(
            ansi_path.to_string_lossy().to_string(),
            0,
            512 * 1024,
        )
        .unwrap();
        assert_eq!(chunk.len(), 256 * 1024);
        drop(dir);
    }
}
