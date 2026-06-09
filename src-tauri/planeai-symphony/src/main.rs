use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use planeai_core::orchestrator::{AutoProject, Orchestrator, OrchestratorConfig};
use planeai_core::session::{Backend, NewSession};
use planeai_core::task::TaskManagerConfig;
use planeai_core::template;

use std::collections::HashMap;

/// Real backend that shells out to git/tmux and writes to the notify socket.
struct RealBackend;

impl Backend for RealBackend {
    fn create_worktree(&self, repo: &str, path: &str, branch: &str, base: &str) -> Result<(), String> {
        let output = Command::new("git")
            .args(["worktree", "add", "-b", branch, path, base])
            .current_dir(repo)
            .output()
            .map_err(|e| format!("git worktree add: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
    }

    fn create_tmux_session(&self, name: &str, cwd: &str, cmd: &str, _session_id: &str) -> Result<(), String> {
        let output = Command::new("tmux")
            .args(["new-session", "-d", "-s", name, "-c", cwd, cmd])
            .output()
            .map_err(|e| format!("tmux new-session: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
    }

    fn insert_session(&self, _session: &NewSession) -> Result<(), String> {
        // TODO: open DB and insert session with auto_dispatched=true
        Ok(())
    }

    fn run_move_task(&self, config: &TaskManagerConfig, key: &str, status: &str, cwd: &Path) -> Result<(), String> {
        let mut vars = HashMap::new();
        vars.insert("key", key);
        vars.insert("status", status);
        let cmd_str = template::render(&config.move_task, &vars);
        let parts: Vec<&str> = cmd_str.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }
        let output = Command::new(parts[0])
            .args(&parts[1..])
            .current_dir(cwd)
            .output()
            .map_err(|e| format!("move_task: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
    }

    fn notify_gui(&self, session_id: &str) -> Result<(), String> {
        use std::io::Write;
        use std::os::unix::net::UnixStream;

        let socket_path = planeai_core::notify_socket_path();
        if !socket_path.exists() {
            return Ok(()); // GUI not running, skip
        }
        let mut stream = UnixStream::connect(&socket_path).map_err(|e| e.to_string())?;
        let msg = format!("{{\"event\":\"session_created\",\"session_id\":\"{session_id}\"}}\n");
        stream.write_all(msg.as_bytes()).map_err(|e| e.to_string())
    }
}

#[tokio::main]
async fn main() {
    // TODO: read config.json, find auto_mode projects, build OrchestratorConfig
    eprintln!("planeai-symphony: not yet fully wired to config. Use planeai-core directly.");
    std::process::exit(1);
}
