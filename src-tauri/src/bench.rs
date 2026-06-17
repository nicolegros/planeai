use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use tauri::ipc::{Channel, Response};

// ─── Tauri Commands ──────────────────────────────────────────────────────────

/// Returns bench replay config from env vars, or null if not in bench mode.
#[tauri::command]
pub fn bench_get_config() -> Option<serde_json::Value> {
    let fixture = std::env::var("PLANEAI_BENCH_REPLAY").ok()?;
    let cols: u32 = std::env::var("PLANEAI_BENCH_COLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);
    let rows: u32 = std::env::var("PLANEAI_BENCH_ROWS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let chunk_size: u32 = std::env::var("PLANEAI_BENCH_CHUNK_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16384);
    let chunk_interval_ms: u32 = std::env::var("PLANEAI_BENCH_CHUNK_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let metrics = std::env::var("PLANEAI_BENCH_METRICS")
        .ok()
        .unwrap_or_else(|| "bench/results/metrics.jsonl".to_string());
    let snapshot = std::env::var("PLANEAI_BENCH_SNAPSHOT").ok();
    let exit_when_done = std::env::var("PLANEAI_BENCH_EXIT")
        .ok()
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);

    Some(serde_json::json!({
        "fixturePath": fixture,
        "cols": cols,
        "rows": rows,
        "chunkSize": chunk_size,
        "chunkIntervalMs": chunk_interval_ms,
        "metricsPath": metrics,
        "snapshotPath": snapshot,
        "exitWhenDone": exit_when_done
    }))
}

#[tauri::command]
pub async fn bench_replay_file(
    fixture_path: String,
    chunk_size: usize,
    chunk_interval_ms: u64,
    on_data: Channel<Response>,
) -> Result<(), String> {
    let data = fs::read(&fixture_path)
        .map_err(|e| format!("failed to read fixture {}: {}", fixture_path, e))?;
    let interval = std::time::Duration::from_millis(chunk_interval_ms);

    let mut offset = 0;
    while offset < data.len() {
        let end = (offset + chunk_size).min(data.len());
        let chunk = data[offset..end].to_vec();
        if on_data.send(Response::new(chunk)).is_err() {
            break;
        }
        offset = end;
        if chunk_interval_ms > 0 && offset < data.len() {
            tokio::time::sleep(interval).await;
        }
    }

    Ok(())
}

#[tauri::command]
pub fn bench_fixture_info(fixture_path: String) -> Result<serde_json::Value, String> {
    let meta = fs::metadata(&fixture_path)
        .map_err(|e| format!("failed to stat {}: {}", fixture_path, e))?;
    let name = PathBuf::from(&fixture_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(serde_json::json!({
        "bytes_total": meta.len(),
        "filename": name
    }))
}

#[tauri::command]
pub fn bench_write_metrics(metrics_path: String, lines: Vec<String>) -> Result<(), String> {
    let path = PathBuf::from(&metrics_path);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("failed to open metrics file: {}", e))?;
    for line in &lines {
        writeln!(file, "{}", line).map_err(|e| format!("write failed: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub fn bench_write_snapshot(snapshot_path: String, content: String) -> Result<(), String> {
    let path = PathBuf::from(&snapshot_path);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, content).map_err(|e| format!("failed to write snapshot: {}", e))
}
