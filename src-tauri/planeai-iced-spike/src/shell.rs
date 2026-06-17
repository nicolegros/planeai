use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;

pub struct Shell {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    reader_buf: Arc<Mutex<Vec<u8>>>,
}

impl Shell {
    pub fn spawn(cols: u16, rows: u16) -> Self {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("Failed to open PTY");

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.env("TERM", "xterm-256color");

        pair.slave
            .spawn_command(cmd)
            .expect("Failed to spawn shell");

        let writer: Box<dyn Write + Send> = pair.master.take_writer().unwrap();
        let writer = Arc::new(Mutex::new(writer));

        let mut reader = pair.master.try_clone_reader().unwrap();
        let reader_buf = Arc::new(Mutex::new(Vec::new()));
        let buf_clone = Arc::clone(&reader_buf);

        // Background thread reads PTY output
        thread::spawn(move || {
            let mut tmp = [0u8; 8192];
            loop {
                match reader.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut buf = buf_clone.lock().unwrap();
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    Err(_) => break,
                }
            }
        });

        Shell { writer, reader_buf }
    }

    /// Drain any pending output from the PTY
    pub fn drain(&self) -> Vec<u8> {
        let mut buf = self.reader_buf.lock().unwrap();
        let data = buf.clone();
        buf.clear();
        data
    }

    /// Write bytes to the PTY (keyboard input)
    pub fn write(&self, data: &[u8]) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(data);
            let _ = w.flush();
        }
    }

    /// Resize the PTY
    pub fn resize(&self, _cols: u16, _rows: u16) {
        // portable-pty doesn't expose resize on the writer directly;
        // the MasterPty would need to be kept. For this spike, resize
        // is a no-op. A full implementation would keep the MasterPty handle.
    }
}
