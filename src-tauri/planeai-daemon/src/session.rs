use crate::buffer::RingBuffer;
use planeai_pty::{LocalPtyConfig, LocalPtySession, PipelineDiagnostics, PtyEvent, PtyEventSink};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Internal PTY implementation — either legacy portable-pty or planeai-pty.
enum PtyInner {
    Legacy {
        writer: Arc<Mutex<Box<dyn Write + Send>>>,
        master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
        child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    },
    PlaneaiPty {
        session: LocalPtySession,
    },
}

pub struct DaemonSession {
    session_id: String,
    buffer: Arc<Mutex<RingBuffer>>,
    alive: Arc<AtomicBool>,
    inner: PtyInner,
    tx: broadcast::Sender<Vec<u8>>,
}

impl DaemonSession {
    pub fn spawn(
        session_id: impl Into<String>,
        command: &str,
        args: &[&str],
        cwd: Option<&str>,
        env: Option<&HashMap<String, String>>,
        buffer_capacity: usize,
    ) -> anyhow::Result<Self> {
        let session_id = session_id.into();
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(command);
        cmd.args(args);
        if let Some(dir) = cwd {
            cmd.cwd(dir);
        }
        if let Some(env_map) = env {
            for (k, v) in env_map {
                cmd.env(k, v);
            }
        }

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let buffer = Arc::new(Mutex::new(RingBuffer::new(buffer_capacity)));
        let alive = Arc::new(AtomicBool::new(true));
        let (tx, _) = broadcast::channel(64);

        let writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;
        let buf_clone = Arc::clone(&buffer);
        let alive_clone = Arc::clone(&alive);
        let tx_clone = tx.clone();

        std::thread::spawn(move || {
            let mut chunk = [0u8; 16384];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => {
                        alive_clone.store(false, Ordering::SeqCst);
                        break;
                    }
                    Ok(n) => {
                        let data = &chunk[..n];
                        buf_clone.lock().unwrap().write(data);
                        let _ = tx_clone.send(data.to_vec());
                    }
                }
            }
        });

        Ok(Self {
            session_id,
            buffer,
            alive,
            inner: PtyInner::Legacy {
                writer: Arc::new(Mutex::new(writer)),
                master: Arc::new(Mutex::new(pair.master)),
                child: Arc::new(Mutex::new(child)),
            },
            tx,
        })
    }

    /// Spawn using planeai-pty crate. Output flows through the same buffer/broadcast.
    pub fn spawn_planeai_pty(
        session_id: impl Into<String>,
        command: &str,
        args: &[&str],
        cwd: Option<&str>,
        env: Option<&HashMap<String, String>>,
        buffer_capacity: usize,
    ) -> anyhow::Result<Self> {
        let session_id = session_id.into();
        let buffer = Arc::new(Mutex::new(RingBuffer::new(buffer_capacity)));
        let alive = Arc::new(AtomicBool::new(true));
        let (tx, _) = broadcast::channel(64);

        let sink: Arc<dyn PtyEventSink> = {
            let primary = Arc::new(DaemonPtySink {
                buffer: Arc::clone(&buffer),
                tx: tx.clone(),
                alive: Arc::clone(&alive),
            });
            if let Some(log_sink) = DurableLogSink::open(&session_id, command, cwd) {
                Arc::new(TeeEventSink {
                    primary,
                    log: log_sink,
                })
            } else {
                primary
            }
        };

        let full_cmd = if args.is_empty() {
            command.to_string()
        } else {
            format!("{} {}", command, args.join(" "))
        };

        let mut env_vec: Vec<(String, String)> =
            vec![("TERM".to_string(), "xterm-256color".to_string())];
        if let Some(env_map) = env {
            for (k, v) in env_map {
                env_vec.push((k.clone(), v.clone()));
            }
        }

        let config = LocalPtyConfig {
            session_id: 0,
            command: Some(full_cmd),
            cwd: cwd.map(PathBuf::from),
            env: env_vec,
            cols: 80,
            rows: 24,
            ..Default::default()
        };

        tracing::info!(
            session_id = %session_id,
            pty_core = "planeai-pty",
            command,
            cwd = cwd.unwrap_or("(none)"),
            cols = 80,
            rows = 24,
            "daemon session spawn (planeai-pty)"
        );

        let session = LocalPtySession::spawn(config, sink)?;

        Ok(Self {
            session_id,
            buffer,
            alive,
            inner: PtyInner::PlaneaiPty { session },
            tx,
        })
    }

    pub fn write(&self, data: &[u8]) -> anyhow::Result<()> {
        match &self.inner {
            PtyInner::Legacy { writer, .. } => {
                writer.lock().unwrap().write_all(data)?;
            }
            PtyInner::PlaneaiPty { session } => {
                session.write(data)?;
            }
        }
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        match &self.inner {
            PtyInner::Legacy { master, .. } => {
                master.lock().unwrap().resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })?;
            }
            PtyInner::PlaneaiPty { session } => {
                session.resize(cols, rows)?;
            }
        }
        Ok(())
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn kill(&self) -> anyhow::Result<()> {
        match &self.inner {
            PtyInner::Legacy { child, .. } => {
                child.lock().unwrap().kill()?;
            }
            PtyInner::PlaneaiPty { session } => {
                session.kill()?;
            }
        }
        self.alive.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn buffer_snapshot(&self) -> Vec<u8> {
        self.buffer.lock().unwrap().snapshot()
    }

    pub fn subscribe_output(&self) -> broadcast::Receiver<Vec<u8>> {
        self.tx.subscribe()
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn diagnostics(&self) -> Option<&Arc<PipelineDiagnostics>> {
        match &self.inner {
            PtyInner::PlaneaiPty { session } => Some(session.diagnostics()),
            PtyInner::Legacy { .. } => None,
        }
    }
}

// ─── DaemonPtySink ───────────────────────────────────────────────────────────

/// Bridges planeai-pty output events to the daemon's buffer + broadcast mechanism.
struct DaemonPtySink {
    buffer: Arc<Mutex<RingBuffer>>,
    tx: broadcast::Sender<Vec<u8>>,
    alive: Arc<AtomicBool>,
}

impl PtyEventSink for DaemonPtySink {
    fn send(&self, event: PtyEvent) -> anyhow::Result<()> {
        match event {
            PtyEvent::Output { bytes, .. } => {
                self.buffer.lock().unwrap().write(&bytes);
                let _ = self.tx.send(bytes);
            }
            PtyEvent::Exit { .. } => {
                self.alive.store(false, Ordering::SeqCst);
            }
            PtyEvent::Error { message, .. } => {
                tracing::error!("planeai-pty error: {message}");
            }
        }
        Ok(())
    }
}

// ─── TeeEventSink ────────────────────────────────────────────────────────────

/// Forwards events to primary sink and durable log sink.
struct TeeEventSink {
    primary: Arc<dyn PtyEventSink>,
    log: DurableLogSink,
}

impl PtyEventSink for TeeEventSink {
    fn send(&self, event: PtyEvent) -> anyhow::Result<()> {
        let _ = self.log.on_event(&event);
        self.primary.send(event)
    }
}

// ─── DurableLogSink ──────────────────────────────────────────────────────────

/// Writes raw output to .ansi file and maintains meta.json for daemon sessions.
struct DurableLogSink {
    file: Mutex<File>,
    bytes_written: AtomicU64,
    bytes_dropped: AtomicU64,
    meta_path: PathBuf,
    meta: Mutex<DaemonSessionMeta>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct DaemonSessionMeta {
    schema_version: u32,
    session_id: String,
    pty_core: String,
    session_source: String,
    started_at: String,
    ended_at: Option<String>,
    command: String,
    cwd: String,
    cols: u16,
    rows: u16,
    ansi_log_file: String,
    bytes_written: u64,
    bytes_dropped: u64,
    exit_status: Option<i32>,
    status: String,
}

impl DurableLogSink {
    fn open(session_id: &str, command: &str, cwd: Option<&str>) -> Option<Self> {
        let log_dir = std::env::var("PLANEAI_SESSION_LOG_DIR").ok()?;
        let session_dir = PathBuf::from(&log_dir).join("sessions").join(session_id);
        fs::create_dir_all(&session_dir).ok()?;

        let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let log_filename = format!("{ts}_output.ansi");
        let log_path = session_dir.join(&log_filename);
        let meta_path = session_dir.join("meta.json");

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .ok()?;

        let meta = DaemonSessionMeta {
            schema_version: 1,
            session_id: session_id.to_string(),
            pty_core: "planeai-pty".to_string(),
            session_source: "daemon".to_string(),
            started_at: chrono::Utc::now().to_rfc3339(),
            ended_at: None,
            command: command.to_string(),
            cwd: cwd.unwrap_or("").to_string(),
            cols: 80,
            rows: 24,
            ansi_log_file: log_filename,
            bytes_written: 0,
            bytes_dropped: 0,
            exit_status: None,
            status: "running".to_string(),
        };
        let _ = fs::write(
            &meta_path,
            serde_json::to_string_pretty(&meta).unwrap_or_default(),
        );

        tracing::info!(session_id, path = %log_path.display(), "daemon durable log enabled");

        Some(Self {
            file: Mutex::new(file),
            bytes_written: AtomicU64::new(0),
            bytes_dropped: AtomicU64::new(0),
            meta_path,
            meta: Mutex::new(meta),
        })
    }

    fn on_event(&self, event: &PtyEvent) -> anyhow::Result<()> {
        match event {
            PtyEvent::Output { bytes, .. } => {
                if let Ok(mut f) = self.file.lock() {
                    if f.write_all(bytes).is_ok() {
                        self.bytes_written
                            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                    } else {
                        self.bytes_dropped
                            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                    }
                }
            }
            PtyEvent::Exit { status, .. } => {
                if let Ok(mut meta) = self.meta.lock() {
                    meta.ended_at = Some(chrono::Utc::now().to_rfc3339());
                    meta.exit_status = *status;
                    meta.status = "exited".to_string();
                    meta.bytes_written = self.bytes_written.load(Ordering::Relaxed);
                    meta.bytes_dropped = self.bytes_dropped.load(Ordering::Relaxed);
                    let _ = fs::write(
                        &self.meta_path,
                        serde_json::to_string_pretty(&*meta).unwrap_or_default(),
                    );
                }
            }
            PtyEvent::Error { .. } => {}
        }
        Ok(())
    }
}

// ─── PTY Core Selection ──────────────────────────────────────────────────────

/// Returns true if PLANEAI_DAEMON_PTY_CORE=planeai-pty.
pub fn use_planeai_pty_core() -> bool {
    match std::env::var("PLANEAI_DAEMON_PTY_CORE") {
        Ok(val) if val == "planeai-pty" => true,
        Ok(val) if val == "legacy" || val.is_empty() => false,
        Ok(val) => {
            tracing::warn!("invalid PLANEAI_DAEMON_PTY_CORE={val:?}, falling back to legacy");
            false
        }
        Err(_) => false,
    }
}
