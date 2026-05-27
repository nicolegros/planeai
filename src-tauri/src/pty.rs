use portable_pty::{native_pty_system, CommandBuilder, PtySize, MasterPty, Child};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{AppHandle, Emitter};

use crate::notify;
use crate::tmux;

/// Describes what command to run inside the PTY.
#[allow(dead_code)]
pub enum PtyTarget {
    /// Attach to an existing tmux session.
    TmuxAttach { tmux_name: String },
    /// Spawn a command directly (no tmux).
    Direct { command: String, args: Vec<String>, cwd: String },
}

struct PtyHandle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    _child: Box<dyn Child + Send + Sync>,
}

pub struct PtyManager {
    ptys: Mutex<HashMap<String, Arc<Mutex<PtyHandle>>>>,
    notify_state: Mutex<Option<notify::SharedNotifyState>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            ptys: Mutex::new(HashMap::new()),
            notify_state: Mutex::new(None),
        }
    }

    pub fn set_notify_state(&self, state: notify::SharedNotifyState) {
        *self.notify_state.lock().unwrap() = Some(state);
    }

    /// Attach a PTY to a session. The command run inside depends on the PtyTarget variant.
    pub fn attach(&self, session_id: &str, target: PtyTarget, app: AppHandle) -> Result<(), String> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("failed to open pty: {e}"))?;

        let cmd = match &target {
            PtyTarget::TmuxAttach { tmux_name } => {
                let mut c = CommandBuilder::new(tmux::tmux_bin());
                c.args(["attach-session", "-t", tmux_name]);
                c
            }
            PtyTarget::Direct { command, args, cwd } => {
                let mut c = CommandBuilder::new(command);
                c.args(args);
                c.cwd(cwd);
                c
            }
        };

        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("failed to spawn: {e}"))?;
        drop(pair.slave);

        let writer = pair.master.take_writer().map_err(|e| format!("failed to get writer: {e}"))?;
        let mut reader = pair.master.try_clone_reader().map_err(|e| format!("failed to get reader: {e}"))?;

        let handle = Arc::new(Mutex::new(PtyHandle {
            master: pair.master,
            writer,
            _child: child,
        }));

        {
            let mut ptys = self.ptys.lock().map_err(|e| e.to_string())?;
            ptys.insert(session_id.to_string(), handle);
        }

        // Spawn reader thread that emits output to frontend
        let event_name = format!("pty-output-{session_id}");
        let exit_event_name = format!("pty-exited-{session_id}");
        let notify_clone = self.notify_state.lock().unwrap().clone();
        let sid = session_id.to_string();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut was_busy = false;
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = &buf[..n];
                        let encoded = base64_encode(data);
                        let _ = app.emit(&event_name, encoded);
                        if let Some(ref ns) = notify_clone {
                            let mut s = ns.lock().unwrap();
                            let was_idle = s.get_state(&sid) != Some(crate::notify::AgentState::Busy);
                            s.notify_output(&sid);
                            if was_idle && !was_busy {
                                drop(s);
                                let _ = app.emit("agent-state-change", serde_json::json!({
                                    "session_id": &sid,
                                    "state": "Busy"
                                }));
                            }
                            was_busy = true;
                        }
                    }
                    Err(_) => break,
                }
            }
            // EOF — process exited
            let _ = app.emit(&exit_event_name, ());
        });

        Ok(())
    }

    /// Write input bytes to a session's PTY.
    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let ptys = self.ptys.lock().map_err(|e| e.to_string())?;
        let handle = ptys.get(session_id).ok_or("session not attached")?;
        let mut h = handle.lock().map_err(|e| e.to_string())?;
        h.writer.write_all(data).map_err(|e| format!("write failed: {e}"))
    }

    /// Resize a session's PTY.
    pub fn resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<(), String> {
        let ptys = self.ptys.lock().map_err(|e| e.to_string())?;
        let handle = ptys.get(session_id).ok_or("session not attached")?;
        let h = handle.lock().map_err(|e| e.to_string())?;
        h.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("resize failed: {e}"))
    }

    /// Detach a session's PTY (cleanup).
    #[allow(dead_code)]
    pub fn detach(&self, session_id: &str) {
        let mut ptys = self.ptys.lock().unwrap_or_else(|e| e.into_inner());
        ptys.remove(session_id);
    }
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}
