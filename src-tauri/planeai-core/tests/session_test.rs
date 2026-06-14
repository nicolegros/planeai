use planeai_core::session::{Backend, DispatchConfig, NewSession, OnStartHook, SessionDispatcher};
use planeai_core::task::{Task, TaskSource};
use std::path::Path;
use std::sync::{Arc, Mutex};

struct MockTaskSource {
    moves: Mutex<Vec<(String, String)>>,
}

impl MockTaskSource {
    fn new() -> Self {
        Self {
            moves: Mutex::new(vec![]),
        }
    }
}

impl TaskSource for MockTaskSource {
    fn list_tasks(&self) -> Result<Vec<Task>, String> {
        Ok(vec![])
    }
    fn get_task(&self, _key: &str) -> Result<Task, String> {
        Err("not found".into())
    }
    fn move_task(&self, key: &str, status: &str) -> Result<(), String> {
        self.moves
            .lock()
            .unwrap()
            .push((key.to_string(), status.to_string()));
        Ok(())
    }
    fn is_terminal(&self, _status: &str) -> bool {
        false
    }
}

/// Records all calls made to the backend for assertions.
#[derive(Default)]
struct RecordingBackend {
    worktrees_created: Mutex<Vec<(String, String, String, String)>>,
    tmux_sessions: Mutex<Vec<(String, String, String, String)>>,
    sessions_inserted: Mutex<Vec<NewSession>>,
    gui_notified: Mutex<Vec<String>>,
    fetches: Mutex<Vec<(String, String)>>,
}

impl Backend for RecordingBackend {
    fn create_worktree(
        &self,
        repo: &str,
        path: &str,
        branch: &str,
        base: &str,
    ) -> Result<(), String> {
        self.worktrees_created.lock().unwrap().push((
            repo.to_string(),
            path.to_string(),
            branch.to_string(),
            base.to_string(),
        ));
        Ok(())
    }
    fn create_tmux_session(
        &self,
        name: &str,
        cwd: &str,
        cmd: &str,
        session_id: &str,
    ) -> Result<(), String> {
        self.tmux_sessions.lock().unwrap().push((
            name.to_string(),
            cwd.to_string(),
            cmd.to_string(),
            session_id.to_string(),
        ));
        Ok(())
    }
    fn insert_session(&self, session: &NewSession) -> Result<(), String> {
        self.sessions_inserted.lock().unwrap().push(session.clone());
        Ok(())
    }
    fn notify_gui(&self, session_id: &str) -> Result<(), String> {
        self.gui_notified
            .lock()
            .unwrap()
            .push(session_id.to_string());
        Ok(())
    }
    fn kill_session(&self, _session: &NewSession) -> Result<(), String> {
        Ok(())
    }
    fn list_active_sessions(&self) -> Result<Vec<NewSession>, String> {
        Ok(vec![])
    }
    fn fetch_base(&self, repo: &str, base: &str) -> Result<String, String> {
        self.fetches
            .lock()
            .unwrap()
            .push((repo.to_string(), base.to_string()));
        Ok(format!("origin/{base}"))
    }
    fn reload_dispatch_config(&self, _provider: &str) -> Option<DispatchConfig> {
        None
    }
}

#[test]
fn dispatch_creates_worktree_session_and_fires_on_start() {
    let backend = RecordingBackend::default();
    let task_source = Arc::new(MockTaskSource::new());

    let dispatcher = SessionDispatcher {
        task_source: task_source.clone(),
        on_start: Some(OnStartHook {
            move_to: "in_progress".to_string(),
        }),
        dispatch_config: DispatchConfig {
            provider: "kiro".to_string(),
            provider_command: "kiro-cli chat".to_string(),
            yolo: true,
            yolo_flag: Some("--trust-all-tools".to_string()),
            worktree_root: "/tmp/worktrees".to_string(),
            base_branch: "main".to_string(),
            session_backend: "tmux".to_string(),
            prompt_template: Some("Implement {key}: {title}\n\n{description}".to_string()),
            prompt_command: Some("{prompt}".to_string()),
            prompt_wrapper: None,
            name_template: None,
        },
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
        subtasks: vec![],
        base_branch: None,
    };

    let session = dispatcher.dispatch(&task, &backend).unwrap();

    // Worktree was created
    let wts = backend.worktrees_created.lock().unwrap();
    assert_eq!(wts.len(), 1);
    assert_eq!(wts[0].0, "/home/user/myapp");
    assert!(wts[0].1.starts_with("/tmp/worktrees/myapp/"));
    assert!(wts[0].2.starts_with("kan-3/")); // branch = key/short_id
    assert_eq!(wts[0].3, "origin/main");

    // Tmux session was created
    let tmux = backend.tmux_sessions.lock().unwrap();
    assert_eq!(tmux.len(), 1);
    assert!(tmux[0].0.starts_with("planeai-myapp-"));
    assert!(tmux[0].2.contains("kiro-cli chat"));
    assert!(tmux[0].2.contains("--trust-all-tools"));
    assert!(tmux[0].2.contains("Implement KAN-3: Add dark mode"));

    // Session was inserted into DB
    let sessions = backend.sessions_inserted.lock().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].task_key, "KAN-3");
    assert_eq!(sessions[0].project_id, "proj-1");
    assert!(sessions[0].auto_dispatched);
    assert_eq!(sessions[0].backend, "tmux");

    // on_start hook fired (task moved to in_progress via TaskSource)
    let moves = task_source.moves.lock().unwrap();
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0], ("KAN-3".to_string(), "in_progress".to_string()));

    // GUI was notified
    let notified = backend.gui_notified.lock().unwrap();
    assert_eq!(notified.len(), 1);
    assert_eq!(notified[0], session.id);
}

#[test]
fn dispatch_uses_task_base_branch_when_present() {
    let backend = RecordingBackend::default();

    let dispatcher = SessionDispatcher {
        task_source: Arc::new(MockTaskSource::new()),
        on_start: None,
        dispatch_config: DispatchConfig {
            provider: "kiro".to_string(),
            provider_command: "kiro-cli chat".to_string(),
            yolo: false,
            yolo_flag: None,
            worktree_root: "/tmp/wt".to_string(),
            base_branch: "main".to_string(),
            session_backend: "tmux".to_string(),
            prompt_template: None,
            prompt_command: None,
            prompt_wrapper: None,
            name_template: None,
        },
        project_id: "p1".to_string(),
        project_name: "proj".to_string(),
        project_path: "/repo".to_string(),
    };

    let task = Task {
        key: "T-1".to_string(),
        title: "Fix".to_string(),
        status: "todo".to_string(),
        description: String::new(),
        priority: 0,
        blocked_by: vec![],
        subtasks: vec![],
        base_branch: Some("develop".to_string()),
    };

    let session = dispatcher.dispatch(&task, &backend).unwrap();

    let wts = backend.worktrees_created.lock().unwrap();
    assert_eq!(wts[0].3, "origin/develop");
    assert_eq!(session.base_branch, "origin/develop");
}

#[test]
fn dispatch_fetches_base_before_worktree_creation() {
    let backend = RecordingBackend::default();

    let dispatcher = SessionDispatcher {
        task_source: Arc::new(MockTaskSource::new()),
        on_start: None,
        dispatch_config: DispatchConfig {
            provider: "kiro".to_string(),
            provider_command: "kiro-cli chat".to_string(),
            yolo: false,
            yolo_flag: None,
            worktree_root: "/tmp/wt".to_string(),
            base_branch: "main".to_string(),
            session_backend: "tmux".to_string(),
            prompt_template: None,
            prompt_command: None,
            prompt_wrapper: None,
            name_template: None,
        },
        project_id: "p1".to_string(),
        project_name: "proj".to_string(),
        project_path: "/repo".to_string(),
    };

    let task = Task {
        key: "T-2".to_string(),
        title: "Fix".to_string(),
        status: "todo".to_string(),
        description: String::new(),
        priority: 0,
        blocked_by: vec![],
        subtasks: vec![],
        base_branch: Some("develop".to_string()),
    };

    dispatcher.dispatch(&task, &backend).unwrap();

    let fetches = backend.fetches.lock().unwrap();
    assert_eq!(fetches.len(), 1);
    assert_eq!(fetches[0], ("/repo".to_string(), "develop".to_string()));

    let wts = backend.worktrees_created.lock().unwrap();
    assert_eq!(wts[0].3, "origin/develop");
}

#[test]
fn dispatch_uses_prompt_command_to_format_prompt_in_command() {
    let backend = RecordingBackend::default();
    let dispatcher = make_dispatcher("claude", "claude", Some("-p {prompt}"));
    let task = make_task();

    let session = dispatcher.dispatch(&task, &backend).unwrap();

    assert!(
        session.command.contains("-p "),
        "expected -p flag, got: {}",
        session.command
    );
    assert!(
        session.command.contains("Implement T-1: Fix bug"),
        "expected rendered prompt, got: {}",
        session.command
    );
}

#[test]
fn dispatch_skips_prompt_when_prompt_command_is_none() {
    let backend = RecordingBackend::default();
    let dispatcher = make_dispatcher("kiro", "kiro-cli chat", None);
    let task = make_task();

    let session = dispatcher.dispatch(&task, &backend).unwrap();

    assert_eq!(session.command, "kiro-cli chat");
}

fn make_dispatcher(
    provider: &str,
    command: &str,
    prompt_command: Option<&str>,
) -> SessionDispatcher {
    SessionDispatcher {
        task_source: Arc::new(MockTaskSource::new()),
        on_start: None,
        dispatch_config: DispatchConfig {
            provider: provider.to_string(),
            provider_command: command.to_string(),
            yolo: false,
            yolo_flag: None,
            worktree_root: "/tmp/wt".to_string(),
            base_branch: "main".to_string(),
            session_backend: "tmux".to_string(),
            prompt_template: Some("Implement {key}: {title}".to_string()),
            prompt_command: prompt_command.map(|s| s.to_string()),
            prompt_wrapper: None,
            name_template: None,
        },
        project_id: "p1".to_string(),
        project_name: "proj".to_string(),
        project_path: "/repo".to_string(),
    }
}

fn make_task() -> Task {
    Task {
        key: "T-1".to_string(),
        title: "Fix bug".to_string(),
        status: "todo".to_string(),
        description: String::new(),
        priority: 1,
        blocked_by: vec![],
        subtasks: vec![],
        base_branch: None,
    }
}
