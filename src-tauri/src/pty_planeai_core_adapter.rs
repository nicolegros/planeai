//! Tauri adapter for planeai-pty.
//!
//! Implements `PtyEventSink` to forward PTY output/exit/error events through
//! the existing Tauri `Channel<Response>` and `AppHandle` event paths, so
//! the frontend does not know whether planeai-pty or the legacy backend is active.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use planeai_pty::{LocalPtyConfig, LocalPtySession, PtyEvent, PtyEventSink};
use tauri::ipc::{Channel, Response};
use tauri::{AppHandle, Emitter};

use crate::output_observer::OutputObserver;
use crate::session_backend::SessionBackend;

/// Forwards planeai-pty events to the Tauri frontend via the existing output channel.
pub struct TauriPtySink {
    session_id: String,
    on_data: Channel<Response>,
    app: AppHandle,
    cancelled: Arc<AtomicBool>,
    observer: Arc<dyn OutputObserver>,
}

impl TauriPtySink {
    pub fn new(
        session_id: String,
        on_data: Channel<Response>,
        app: AppHandle,
        cancelled: Arc<AtomicBool>,
        observer: Arc<dyn OutputObserver>,
    ) -> Self {
        Self { session_id, on_data, app, cancelled, observer }
    }
}

impl PtyEventSink for TauriPtySink {
    fn send(&self, event: PtyEvent) -> anyhow::Result<()> {
        match event {
            PtyEvent::Output { bytes, .. } => {
                self.observer.on_output(&self.session_id, bytes.len());
                self.on_data
                    .send(Response::new(bytes))
                    .map_err(|e| anyhow::anyhow!("channel send failed: {e}"))?;
            }
            PtyEvent::Exit { .. } => {
                if !self.cancelled.load(Ordering::Acquire) {
                    let _ = self.app.emit(
                        "pty-exited",
                        serde_json::json!({ "pty_key": self.session_id }),
                    );
                }
            }
            PtyEvent::Error { message, .. } => {
                tracing::error!(session_id = %self.session_id, "pty error: {message}");
            }
        }
        Ok(())
    }
}

/// SessionBackend implementation backed by planeai-pty's LocalPtySession.
pub struct PlaneaiPtyBackend {
    session: LocalPtySession,
}

impl PlaneaiPtyBackend {
    /// Spawn a new local PTY session via planeai-pty and return a SessionBackend.
    pub fn spawn(
        session_id: &str,
        command: &str,
        args: &[String],
        cwd: &str,
        dark_mode: bool,
        app: AppHandle,
        on_data: Channel<Response>,
        cancelled: Arc<AtomicBool>,
        observer: Arc<dyn OutputObserver>,
        socket_path: Option<&str>,
    ) -> Result<Self, String> {
        let full_command = if args.is_empty() {
            command.to_string()
        } else {
            format!("{} {}", command, args.join(" "))
        };

        let tauri_sink: Arc<dyn PtyEventSink> = Arc::new(TauriPtySink::new(
            session_id.to_string(),
            on_data,
            app,
            cancelled,
            observer,
        ));

        let sink: Arc<dyn PtyEventSink> = if let Some(log_dir) = session_log_dir() {
            let session_log_dir = log_dir.join("sessions").join(session_id);
            let ts = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
            let log_filename = format!("{ts}_output.ansi");
            let log_path = session_log_dir.join(&log_filename);
            match LogSink::open(&log_path) {
                Some(log_sink) => {
                    tracing::info!(session_id, path = %log_path.display(), "session log enabled");
                    let meta = SessionMeta {
                        schema_version: 1,
                        session_id: session_id.to_string(),
                        pty_core: "planeai-pty".to_string(),
                        session_source: Some("local".to_string()),
                        started_at: Utc::now().to_rfc3339(),
                        ended_at: None,
                        command: full_command.clone(),
                        cwd: cwd.to_string(),
                        cols: 80,
                        rows: 24,
                        ansi_log_file: log_filename,
                        bytes_written: 0,
                        bytes_dropped: 0,
                        exit_status: None,
                        status: "running".to_string(),
                    };
                    let meta_path = session_log_dir.join("meta.json");
                    if let Err(e) = write_meta(&meta_path, &meta) {
                        tracing::warn!("failed to write session metadata: {e}");
                    }
                    let tracking_sink = TrackingLogSink::new(log_sink, meta_path, meta);
                    Arc::new(TeeSink::new(tauri_sink, vec![Arc::new(tracking_sink)]))
                }
                None => tauri_sink,
            }
        } else {
            tauri_sink
        };

        let mut env = vec![
            ("TERM".to_string(), "xterm-256color".to_string()),
            (
                "COLORFGBG".to_string(),
                if dark_mode { "15;0" } else { "0;15" }.to_string(),
            ),
            ("PLANEAI_SESSION_ID".to_string(), session_id.to_string()),
        ];
        if let Some(sock) = socket_path {
            env.push(("PLANEAI_SOCKET".to_string(), sock.to_string()));
        }

        let config = LocalPtyConfig {
            session_id: 0,
            command: Some(full_command),
            cwd: Some(cwd.into()),
            env,
            cols: 80,
            rows: 24,
            ..Default::default()
        };

        let session =
            LocalPtySession::spawn(config, sink).map_err(|e| format!("planeai-pty spawn: {e}"))?;
        Ok(Self { session })
    }
}

impl SessionBackend for PlaneaiPtyBackend {
    fn write(&self, data: &[u8]) -> Result<(), String> {
        self.session.write(data).map_err(|e| e.to_string())
    }

    fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        self.session.resize(cols, rows).map_err(|e| e.to_string())
    }

    fn pause(&self) -> Result<(), String> {
        self.session.pause();
        Ok(())
    }

    fn resume(&self) -> Result<(), String> {
        self.session.resume();
        Ok(())
    }

    fn detach(&self) {
        let _ = self.session.kill();
    }
}

// ─── TeeSink ─────────────────────────────────────────────────────────────────

/// Multiplexes PtyEvents to multiple sinks. First sink is primary (errors propagate).
/// Secondary sink errors are logged but do not fail the pipeline.
pub struct TeeSink {
    primary: Arc<dyn PtyEventSink>,
    secondary: Vec<Arc<dyn PtyEventSink>>,
}

impl TeeSink {
    pub fn new(primary: Arc<dyn PtyEventSink>, secondary: Vec<Arc<dyn PtyEventSink>>) -> Self {
        Self { primary, secondary }
    }
}

impl PtyEventSink for TeeSink {
    fn send(&self, event: PtyEvent) -> anyhow::Result<()> {
        // Send to all secondary sinks first (best-effort)
        for sink in &self.secondary {
            let _ = sink.send(event.clone());
        }
        // Primary sink errors propagate
        self.primary.send(event)
    }
}

// ─── Session Metadata ────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct SessionMeta {
    pub schema_version: u32,
    pub session_id: String,
    pub pty_core: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_source: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub command: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub ansi_log_file: String,
    pub bytes_written: u64,
    pub bytes_dropped: u64,
    pub exit_status: Option<i32>,
    pub status: String,
}

fn write_meta(path: &PathBuf, meta: &SessionMeta) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(meta).unwrap_or_default();
    fs::write(path, json)
}

// ─── TrackingLogSink (durable ANSI log + metadata updates) ──────────────────

/// Wraps LogSink with byte tracking and metadata finalization on Exit.
pub struct TrackingLogSink {
    log_sink: LogSink,
    bytes_written: AtomicU64,
    bytes_dropped: AtomicU64,
    meta_path: PathBuf,
    meta: Mutex<SessionMeta>,
}

impl TrackingLogSink {
    fn new(log_sink: LogSink, meta_path: PathBuf, meta: SessionMeta) -> Self {
        Self {
            log_sink,
            bytes_written: AtomicU64::new(0),
            bytes_dropped: AtomicU64::new(0),
            meta_path,
            meta: Mutex::new(meta),
        }
    }

    fn finalize(&self, exit_status: Option<i32>) {
        if let Ok(mut meta) = self.meta.lock() {
            meta.ended_at = Some(Utc::now().to_rfc3339());
            meta.exit_status = exit_status;
            meta.status = "exited".to_string();
            meta.bytes_written = self.bytes_written.load(Ordering::Relaxed);
            meta.bytes_dropped = self.bytes_dropped.load(Ordering::Relaxed);
            if let Err(e) = write_meta(&self.meta_path, &meta) {
                tracing::warn!("failed to finalize session metadata: {e}");
            }
        }
    }
}

impl PtyEventSink for TrackingLogSink {
    fn send(&self, event: PtyEvent) -> anyhow::Result<()> {
        match &event {
            PtyEvent::Output { bytes, .. } => {
                let len = bytes.len() as u64;
                if self.log_sink.send(event).is_ok() {
                    self.bytes_written.fetch_add(len, Ordering::Relaxed);
                } else {
                    self.bytes_dropped.fetch_add(len, Ordering::Relaxed);
                }
            }
            PtyEvent::Exit { status, .. } => {
                self.finalize(*status);
            }
            PtyEvent::Error { .. } => {}
        }
        Ok(())
    }
}

// ─── LogSink (durable ANSI log) ─────────────────────────────────────────────

/// Writes raw PTY output bytes to a .ansi log file. Buffered, synchronous writes.
/// Write errors are logged but never crash the app.
pub struct LogSink {
    file: Mutex<File>,
}

impl LogSink {
    /// Create a LogSink writing to the given path. Creates parent directories.
    pub fn open(path: &PathBuf) -> Option<Self> {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(f) => Some(Self { file: Mutex::new(f) }),
            Err(e) => {
                tracing::warn!("failed to open session log {}: {e}", path.display());
                None
            }
        }
    }
}

impl PtyEventSink for LogSink {
    fn send(&self, event: PtyEvent) -> anyhow::Result<()> {
        if let PtyEvent::Output { bytes, .. } = event {
            if let Ok(mut f) = self.file.lock() {
                if let Err(e) = f.write_all(&bytes) {
                    tracing::warn!("session log write error: {e}");
                    return Err(anyhow::anyhow!("write failed: {e}"));
                }
            }
        }
        Ok(())
    }
}

/// Returns the session log directory if PLANEAI_SESSION_LOG_DIR is set.
pub fn session_log_dir() -> Option<PathBuf> {
    std::env::var("PLANEAI_SESSION_LOG_DIR").ok().map(PathBuf::from)
}
