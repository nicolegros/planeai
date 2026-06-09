use planeai_core::orchestrator::{AutoProject, Orchestrator, OrchestratorConfig};
use planeai_core::session::{Backend, NewSession};
use planeai_core::task::{LifecycleHook, TaskManagerConfig};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

/// Backend that records dispatched sessions.
#[derive(Default)]
struct TestBackend {
    worktrees: Mutex<Vec<String>>,
    sessions: Mutex<Vec<NewSession>>,
}

impl Backend for TestBackend {
    fn create_worktree(&self, _repo: &str, path: &str, _branch: &str, _base: &str) -> Result<(), String> {
        self.worktrees.lock().unwrap().push(path.to_string());
        Ok(())
    }
    fn create_tmux_session(&self, _name: &str, _cwd: &str, _cmd: &str, _sid: &str) -> Result<(), String> {
        Ok(())
    }
    fn insert_session(&self, session: &NewSession) -> Result<(), String> {
        self.sessions.lock().unwrap().push(session.clone());
        Ok(())
    }
    fn run_move_task(&self, _cfg: &TaskManagerConfig, _key: &str, _status: &str, _cwd: &Path) -> Result<(), String> {
        Ok(())
    }
    fn notify_gui(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }
}

fn write_script(dir: &Path, name: &str, output: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\nprintf '%s' '{output}'")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

#[tokio::test]
async fn orchestrator_polls_dispatches_and_stops_on_socket_command() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("symphony.sock");

    let list_json = r#"[{"key":"KAN-1","title":"Fix bug","status":"todo","description":"desc","priority":1,"blocked_by":[]}]"#;
    let list_script = write_script(dir.path(), "list.sh", list_json);

    let config = OrchestratorConfig {
        poll_interval_ms: 50,
        max_concurrent: 2,
        socket_path: socket_path.clone(),
        projects: vec![AutoProject {
            project_id: "p1".to_string(),
            project_name: "testproj".to_string(),
            project_path: dir.path().to_string_lossy().to_string(),
            task_manager_config: TaskManagerConfig {
                list_tasks: format!("{list_script} --project {{project}}"),
                get_task: String::new(),
                move_task: String::new(),
                terminal_states: vec!["done".to_string()],
                on_start: Some(LifecycleHook { move_to: "in_progress".to_string() }),
            },
            dispatch_config: planeai_core::session::DispatchConfig {
                provider: "kiro".to_string(),
                provider_command: "kiro-cli chat".to_string(),
                yolo: true,
                yolo_flag: Some("--trust-all-tools".to_string()),
                worktree_root: dir.path().join("worktrees").to_string_lossy().to_string(),
                base_branch: "main".to_string(),
                session_backend: "tmux".to_string(),
                prompt_template: None,
            },
        }],
    };

    let backend = Arc::new(TestBackend::default());
    let orchestrator = Orchestrator::new(config, backend.clone());

    // Spawn orchestrator in background
    let handle = tokio::spawn(async move {
        orchestrator.run().await
    });

    // Wait for at least one poll tick to fire
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Send stop command via socket
    let mut stream = tokio::net::UnixStream::connect(&socket_path)
        .await
        .expect("should connect to symphony.sock");
    tokio::io::AsyncWriteExt::write_all(&mut stream, b"stop\n")
        .await
        .expect("should send stop");

    // Orchestrator should exit cleanly
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
        .await
        .expect("orchestrator should stop within 2s")
        .expect("task should not panic");

    assert!(result.is_ok());

    // Verify a session was dispatched
    let sessions = backend.sessions.lock().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].task_key, "KAN-1");
    assert!(sessions[0].auto_dispatched);
}
