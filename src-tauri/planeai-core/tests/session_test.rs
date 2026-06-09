use planeai_core::session::{Backend, DispatchConfig, NewSession, SessionDispatcher};
use planeai_core::task::{LifecycleHook, Task, TaskManagerConfig};
use std::path::Path;
use std::sync::Mutex;

/// Records all calls made to the backend for assertions.
#[derive(Default)]
struct RecordingBackend {
    worktrees_created: Mutex<Vec<(String, String, String, String)>>,
    tmux_sessions: Mutex<Vec<(String, String, String, String)>>,
    sessions_inserted: Mutex<Vec<NewSession>>,
    task_moves: Mutex<Vec<(String, String)>>,
    gui_notified: Mutex<Vec<String>>,
}

impl Backend for RecordingBackend {
    fn create_worktree(&self, repo: &str, path: &str, branch: &str, base: &str) -> Result<(), String> {
        self.worktrees_created.lock().unwrap().push((
            repo.to_string(), path.to_string(), branch.to_string(), base.to_string(),
        ));
        Ok(())
    }
    fn create_tmux_session(&self, name: &str, cwd: &str, cmd: &str, session_id: &str) -> Result<(), String> {
        self.tmux_sessions.lock().unwrap().push((
            name.to_string(), cwd.to_string(), cmd.to_string(), session_id.to_string(),
        ));
        Ok(())
    }
    fn insert_session(&self, session: &NewSession) -> Result<(), String> {
        self.sessions_inserted.lock().unwrap().push(session.clone());
        Ok(())
    }
    fn run_move_task(&self, _config: &TaskManagerConfig, key: &str, status: &str, _cwd: &Path) -> Result<(), String> {
        self.task_moves.lock().unwrap().push((key.to_string(), status.to_string()));
        Ok(())
    }
    fn notify_gui(&self, session_id: &str) -> Result<(), String> {
        self.gui_notified.lock().unwrap().push(session_id.to_string());
        Ok(())
    }
    fn kill_session(&self, _session: &NewSession) -> Result<(), String> {
        Ok(())
    }
    fn list_active_sessions(&self) -> Result<Vec<NewSession>, String> {
        Ok(vec![])
    }
}

#[test]
fn dispatch_creates_worktree_session_and_fires_on_start() {
    let backend = RecordingBackend::default();

    let task_manager_config = TaskManagerConfig {
        list_tasks: String::new(),
        get_task: String::new(),
        move_task: "kanban move {key} {status}".to_string(),
        terminal_states: vec!["done".to_string()],
        on_start: Some(LifecycleHook { move_to: "in_progress".to_string() }),
    };

    let dispatch_config = DispatchConfig {
        provider: "kiro".to_string(),
        provider_command: "kiro-cli chat".to_string(),
        yolo: true,
        yolo_flag: Some("--trust-all-tools".to_string()),
        worktree_root: "/tmp/worktrees".to_string(),
        base_branch: "main".to_string(),
        session_backend: "tmux".to_string(),
        prompt_template: Some("Implement {key}: {title}\n\n{description}".to_string()),
    };

    let dispatcher = SessionDispatcher {
        task_manager_config,
        dispatch_config,
        project_id: "proj-1".to_string(),
        project_name: "myapp".to_string(),
        project_path: "/home/user/myapp".to_string(),
    };

    let task = Task {
        key: "KAN-3".to_string(),
        title: "Add dark mode".to_string(),
        status: "todo".to_string(),
        description: "Full dark mode support".to_string(),
        priority: 1,
        blocked_by: vec![],
    };

    let session = dispatcher.dispatch(&task, &backend).unwrap();

    // Worktree was created
    let wts = backend.worktrees_created.lock().unwrap();
    assert_eq!(wts.len(), 1);
    assert_eq!(wts[0].0, "/home/user/myapp"); // repo
    assert!(wts[0].1.starts_with("/tmp/worktrees/myapp/")); // worktree path
    assert_eq!(wts[0].2, "kan-3"); // branch
    assert_eq!(wts[0].3, "main"); // base

    // Tmux session was created
    let tmux = backend.tmux_sessions.lock().unwrap();
    assert_eq!(tmux.len(), 1);
    assert!(tmux[0].0.starts_with("planeai-myapp-")); // name
    assert!(tmux[0].2.contains("kiro-cli chat")); // command contains provider
    assert!(tmux[0].2.contains("--trust-all-tools")); // yolo flag
    assert!(tmux[0].2.contains("Implement KAN-3: Add dark mode")); // prompt rendered

    // Session was inserted into DB
    let sessions = backend.sessions_inserted.lock().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].task_key, "KAN-3");
    assert_eq!(sessions[0].project_id, "proj-1");
    assert!(sessions[0].auto_dispatched);
    assert_eq!(sessions[0].backend, "tmux");

    // on_start hook fired (task moved to in_progress)
    let moves = backend.task_moves.lock().unwrap();
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0], ("KAN-3".to_string(), "in_progress".to_string()));

    // GUI was notified
    let notified = backend.gui_notified.lock().unwrap();
    assert_eq!(notified.len(), 1);
    assert_eq!(notified[0], session.id);
}
