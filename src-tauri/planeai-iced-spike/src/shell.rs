use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;

pub const MAX_BUFFER: usize = 512 * 1024; // 512KB bounded buffer
pub const QUEUE_POLICY: &str = "drop_oldest";

pub struct Shell {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    reader_buf: Arc<Mutex<Vec<u8>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    pub max_pending_bytes: Arc<Mutex<usize>>,
    pub bytes_dropped: Arc<Mutex<u64>>,
    pub exited: Arc<Mutex<bool>>,
}

impl Shell {
    pub fn spawn(cols: u16, rows: u16) -> Self {
        Self::spawn_command(cols, rows, None)
    }

    pub fn spawn_command(cols: u16, rows: u16, command: Option<&str>) -> Self {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
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
        let buf_clone = Arc::clone(&reader_buf);
        let max_pending_bytes = Arc::new(Mutex::new(0usize));
        let max_pending_clone = Arc::clone(&max_pending_bytes);
        let bytes_dropped = Arc::new(Mutex::new(0u64));
        let bytes_dropped_clone = Arc::clone(&bytes_dropped);
        let exited = Arc::new(Mutex::new(false));
        let exited_clone = Arc::clone(&exited);

        thread::spawn(move || {
            let mut tmp = [0u8; 16384];
            loop {
                match reader.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut buf = buf_clone.lock().unwrap();
                        // Bounded: drop oldest if over limit
                        if buf.len() + n > MAX_BUFFER {
                            let drain = (buf.len() + n).saturating_sub(MAX_BUFFER);
                            *bytes_dropped_clone.lock().unwrap() += drain as u64;
                            buf.drain(..drain);
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
            master: Arc::new(Mutex::new(master)),
            max_pending_bytes,
            bytes_dropped,
            exited,
        }
    }

    pub fn drain(&self) -> Vec<u8> {
        let mut buf = self.reader_buf.lock().unwrap();
        let data = std::mem::take(&mut *buf);
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

    pub fn resize(&self, cols: u16, rows: u16) {
        if let Ok(m) = self.master.lock() {
            let _ = m.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }
}
