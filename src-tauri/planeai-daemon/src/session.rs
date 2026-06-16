use crate::buffer::RingBuffer;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub struct DaemonSession {
    session_id: String,
    buffer: Arc<Mutex<RingBuffer>>,
    alive: Arc<AtomicBool>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    tx: broadcast::Sender<Vec<u8>>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
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
            writer: Arc::new(Mutex::new(writer)),
            master: Arc::new(Mutex::new(pair.master)),
            tx,
            child: Arc::new(Mutex::new(child)),
        })
    }

    pub fn write(&self, data: &[u8]) -> anyhow::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data)?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        let master = self.master.lock().unwrap();
        master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn kill(&self) -> anyhow::Result<()> {
        self.child.lock().unwrap().kill()?;
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
}
