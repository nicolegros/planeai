use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Instant;

pub const MAX_BUFFER: usize = 512 * 1024; // 512KB bounded buffer

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QueuePolicy {
    /// Lossless: PTY reader blocks when buffer is full until UI drains.
    Block,
    /// Lossy: drop oldest bytes when buffer is full (stress testing only).
    DropOldest,
}

impl QueuePolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::DropOldest => "drop_oldest",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "drop_oldest" => Self::DropOldest,
            _ => Self::Block,
        }
    }
}

pub struct Shell {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    reader_buf: Arc<Mutex<Vec<u8>>>,
    buf_not_full: Arc<Condvar>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    pub max_pending_bytes: Arc<Mutex<usize>>,
    pub bytes_dropped: Arc<Mutex<u64>>,
    pub producer_block_count: Arc<Mutex<u64>>,
    pub producer_block_duration_ms: Arc<Mutex<f64>>,
    pub exited: Arc<Mutex<bool>>,
    pub policy: QueuePolicy,
}

impl Shell {
    pub fn spawn(cols: u16, rows: u16) -> Self {
        Self::spawn_with_policy(cols, rows, None, QueuePolicy::Block)
    }

    pub fn spawn_command(cols: u16, rows: u16, command: Option<&str>) -> Self {
        Self::spawn_with_policy(cols, rows, command, QueuePolicy::Block)
    }

    pub fn spawn_with_policy(cols: u16, rows: u16, command: Option<&str>, policy: QueuePolicy) -> Self {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .expect("Failed to open PTY");

        let cmd = if let Some(cmd_str) = command {
            let mut cmd = CommandBuilder::new("bash");
            cmd.args(["-c", cmd_str]);
            cmd.env("TERM", "xterm-256color");
            cmd
        } else {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            let mut cmd = CommandBuilder::new(&shell);
            cmd.env("TERM", "xterm-256color");
            cmd
        };

        pair.slave.spawn_command(cmd).expect("Failed to spawn shell");

        let writer: Box<dyn Write + Send> = pair.master.take_writer().unwrap();
        let writer = Arc::new(Mutex::new(writer));

        let mut reader = pair.master.try_clone_reader().unwrap();
        let reader_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let buf_not_full: Arc<Condvar> = Arc::new(Condvar::new());
        let buf_clone = Arc::clone(&reader_buf);
        let condvar_clone = Arc::clone(&buf_not_full);
        let max_pending_bytes = Arc::new(Mutex::new(0usize));
        let max_pending_clone = Arc::clone(&max_pending_bytes);
        let bytes_dropped = Arc::new(Mutex::new(0u64));
        let bytes_dropped_clone = Arc::clone(&bytes_dropped);
        let producer_block_count = Arc::new(Mutex::new(0u64));
        let block_count_clone = Arc::clone(&producer_block_count);
        let producer_block_duration_ms = Arc::new(Mutex::new(0.0f64));
        let block_duration_clone = Arc::clone(&producer_block_duration_ms);
        let exited = Arc::new(Mutex::new(false));
        let exited_clone = Arc::clone(&exited);

        thread::spawn(move || {
            let mut tmp = [0u8; 16384];
            loop {
                match reader.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut buf = buf_clone.lock().unwrap();
                        match policy {
                            QueuePolicy::Block => {
                                // Block until there's room
                                if buf.len() + n > MAX_BUFFER {
                                    *block_count_clone.lock().unwrap() += 1;
                                    let block_start = Instant::now();
                                    buf = condvar_clone.wait_while(buf, |b| {
                                        b.len() + n > MAX_BUFFER
                                    }).unwrap();
                                    let elapsed = block_start.elapsed().as_secs_f64() * 1000.0;
                                    *block_duration_clone.lock().unwrap() += elapsed;
                                }
                            }
                            QueuePolicy::DropOldest => {
                                if buf.len() + n > MAX_BUFFER {
                                    let drain = (buf.len() + n).saturating_sub(MAX_BUFFER);
                                    *bytes_dropped_clone.lock().unwrap() += drain as u64;
                                    buf.drain(..drain);
                                }
                            }
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        let len = buf.len();
                        let mut max = max_pending_clone.lock().unwrap();
                        if len > *max {
                            *max = len;
                        }
                    }
                    Err(_) => break,
                }
            }
            *exited_clone.lock().unwrap() = true;
        });

        let master: Box<dyn MasterPty + Send> = pair.master;
        Shell {
            writer,
            reader_buf,
            buf_not_full,
            master: Arc::new(Mutex::new(master)),
            max_pending_bytes,
            bytes_dropped,
            producer_block_count,
            producer_block_duration_ms,
            exited,
            policy,
        }
    }

    pub fn drain(&self) -> Vec<u8> {
        let mut buf = self.reader_buf.lock().unwrap();
        let data = std::mem::take(&mut *buf);
        // Notify the producer that buffer space is available
        self.buf_not_full.notify_one();
        data
    }

    pub fn has_exited(&self) -> bool {
        *self.exited.lock().unwrap()
    }

    pub fn pending_len(&self) -> usize {
        self.reader_buf.lock().unwrap().len()
    }

    pub fn write(&self, data: &[u8]) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(data);
            let _ = w.flush();
        }
    }

    pub fn bytes_dropped(&self) -> u64 {
        *self.bytes_dropped.lock().unwrap()
    }

    pub fn producer_block_count(&self) -> u64 {
        *self.producer_block_count.lock().unwrap()
    }

    pub fn producer_block_duration_ms(&self) -> f64 {
        *self.producer_block_duration_ms.lock().unwrap()
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        if let Ok(m) = self.master.lock() {
            let _ = m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }
}
