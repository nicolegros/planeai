use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};

use crate::ring_buffer::RingBuffer;

const DEFAULT_SCROLLBACK: usize = 1024 * 1024; // 1MB
const READ_BUF: usize = 16 * 1024;

struct PtyInner {
    #[allow(dead_code)]
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

pub struct Session {
    inner: Mutex<PtyInner>,
    pub scrollback: Mutex<RingBuffer>,
    /// Broadcast channel for PTY output — data connections subscribe here.
    pub output_tx: broadcast::Sender<Vec<u8>>,
    pub alive: RwLock<bool>,
}

impl Session {
    pub fn spawn(
        session_id: String,
        command: &str,
        args: &[String],
        cwd: &str,
        env: &[(String, String)],
    ) -> Result<Arc<Self>, String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("open pty: {e}"))?;

        let mut cmd = CommandBuilder::new(command);
        cmd.args(args);
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("PLANEAI_SESSION_ID", &session_id);
        for (k, v) in env {
            cmd.env(k, v);
        }

        #[cfg(unix)]
        {
            cmd.env("LANG", "en_US.UTF-8");
            cmd.env("LC_CTYPE", "en_US.UTF-8");
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn: {e}"))?;
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("writer: {e}"))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("reader: {e}"))?;

        let (output_tx, _) = broadcast::channel(256);

        let session = Arc::new(Self {
            inner: Mutex::new(PtyInner {
                master: pair.master,
                writer,
                child,
            }),
            scrollback: Mutex::new(RingBuffer::new(DEFAULT_SCROLLBACK)),
            output_tx: output_tx.clone(),
            alive: RwLock::new(true),
        });

        // Spawn reader task
        let sess = session.clone();
        let tx = output_tx;
        std::thread::spawn(move || {
            read_loop(reader, tx, sess);
        });

        Ok(session)
    }

    pub async fn write_input(&self, data: &[u8]) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        inner
            .writer
            .write_all(data)
            .map_err(|e| format!("write: {e}"))
    }

    pub async fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        let inner = self.inner.lock().await;
        inner
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("resize: {e}"))
    }

    pub async fn kill(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        inner.child.kill().map_err(|e| format!("kill: {e}"))?;
        *self.alive.write().await = false;
        Ok(())
    }
}

fn read_loop(
    mut reader: Box<dyn Read + Send>,
    tx: broadcast::Sender<Vec<u8>>,
    session: Arc<Session>,
) {
    let mut buf = [0u8; READ_BUF];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let data = buf[..n].to_vec();
                // Store in scrollback
                {
                    let rt = tokio::runtime::Handle::try_current();
                    if let Ok(handle) = rt {
                        let sess = session.clone();
                        let d = data.clone();
                        handle.spawn(async move {
                            sess.scrollback.lock().await.push(&d);
                        });
                    }
                }
                // Broadcast to attached clients
                let _ = tx.send(data);
            }
        }
    }
    // Mark session as dead
    let rt = tokio::runtime::Handle::try_current();
    if let Ok(handle) = rt {
        let sess = session.clone();
        handle.spawn(async move {
            *sess.alive.write().await = false;
        });
    }
}

/// Manages all daemon sessions.
pub struct SessionManager {
    pub sessions: RwLock<HashMap<String, Arc<Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub async fn create(
        &self,
        session_id: String,
        command: &str,
        args: &[String],
        cwd: &str,
        env: &[(String, String)],
    ) -> Result<(), String> {
        let session = Session::spawn(session_id.clone(), command, args, cwd, env)?;
        self.sessions.write().await.insert(session_id, session);
        Ok(())
    }

    pub async fn get(&self, session_id: &str) -> Option<Arc<Session>> {
        self.sessions.read().await.get(session_id).cloned()
    }

    pub async fn remove(&self, session_id: &str) -> Option<Arc<Session>> {
        self.sessions.write().await.remove(session_id)
    }

    pub async fn list(&self) -> Vec<crate::protocol::SessionInfo> {
        let sessions = self.sessions.read().await;
        let mut out = Vec::with_capacity(sessions.len());
        for (id, sess) in sessions.iter() {
            out.push(crate::protocol::SessionInfo {
                session_id: id.clone(),
                alive: *sess.alive.read().await,
            });
        }
        out
    }

    pub async fn has_live_sessions(&self) -> bool {
        let sessions = self.sessions.read().await;
        for sess in sessions.values() {
            if *sess.alive.read().await {
                return true;
            }
        }
        false
    }
}
