use planeai_core::orchestrator::{
    AutoProject, Orchestrator, OrchestratorCommand, OrchestratorConfig,
};
use planeai_core::session::{Backend, DispatchConfig, NewSession, OnStartHook};
use planeai_core::task::{Task, TaskSource};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct MockTaskSource {
    tasks: Mutex<Vec<Task>>,
    terminal_states: Vec<String>,
    moves: Mutex<Vec<(String, String)>>,
}

impl MockTaskSource {
    fn new(tasks: Vec<Task>, terminal_states: Vec<String>) -> Self {
        Self {
            tasks: Mutex::new(tasks),
            terminal_states,
            moves: Mutex::new(vec![]),
        }
    }
}

impl TaskSource for MockTaskSource {
    fn list_tasks(&self) -> Result<Vec<Task>, String> {
        Ok(self.tasks.lock().unwrap().clone())
    }
    fn get_task(&self, key: &str) -> Result<Task, String> {
        self.tasks
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.key == key)
            .cloned()
            .ok_or_else(|| "not found".into())
    }
    fn move_task(&self, key: &str, status: &str) -> Result<(), String> {
        self.moves
            .lock()
            .unwrap()
            .push((key.to_string(), status.to_string()));
        // Also update the task status in-memory for reconciliation tests
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(t) = tasks.iter_mut().find(|t| t.key == key) {
            t.status = status.to_string();
        }
        Ok(())
    }
    fn is_terminal(&self, status: &str) -> bool {
        self.terminal_states
            .iter()
            .any(|s| s.eq_ignore_ascii_case(status))
    }
}

/// Backend that records dispatched sessions.
#[derive(Default)]
struct TestBackend {
    sessions: Mutex<Vec<NewSession>>,
}

impl Backend for TestBackend {
    fn create_worktree(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn create_tmux_session(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn create_daemon_session(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn insert_session(&self, session: &NewSession) -> Result<(), String> {
        self.sessions.lock().unwrap().push(session.clone());
        Ok(())
    }
    fn notify_gui(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn kill_session(&self, _: &NewSession) -> Result<(), String> {
        Ok(())
    }
    fn list_active_sessions(&self) -> Result<Vec<NewSession>, String> {
        Ok(vec![])
    }
    fn fetch_base(&self, _: &str, base: &str) -> Result<String, String> {
        Ok(format!("origin/{base}"))
    }
    fn reload_dispatch_config(&self, _: &str) -> Option<DispatchConfig> {
        None
    }
}

fn default_dispatch_config(worktree_root: &str) -> DispatchConfig {
    DispatchConfig {
        provider: "kiro".to_string(),
        provider_command: "kiro-cli chat".to_string(),
        yolo: true,
        yolo_flag: Some("--trust-all-tools".to_string()),
        worktree_root: worktree_root.to_string(),
        base_branch: "main".to_string(),
        session_backend: "tmux".to_string(),
        prompt_template: None,
        prompt_command: None,
        prompt_wrapper: None,
        name_template: None,
    }
}

#[tokio::test]
async fn orchestrator_polls_dispatches_and_stops_on_channel_command() {
    let source = Arc::new(MockTaskSource::new(
        vec![Task {
            key: "KAN-1".into(),
            title: "Fix bug".into(),
            status: "todo".into(),
            description: "desc".into(),
            priority: 1,
            blocked_by: vec![],
            subtasks: vec![],
            base_branch: "main".to_string(),
        }],
        vec!["done".into(), "cancelled".into()],
    ));

    let config = OrchestratorConfig {
        poll_interval_ms: 50,
        max_concurrent: 2,
        projects: vec![AutoProject {
            project_id: "p1".to_string(),
            project_name: "testproj".to_string(),
            project_path: "/tmp/testproj".to_string(),
            task_source: source.clone(),
            on_start: Some(OnStartHook {
                move_to: "in_progress".to_string(),
            }),
            dispatch_config: default_dispatch_config("/tmp/worktrees"),
        }],
    };

    let backend = Arc::new(TestBackend::default());
    let orchestrator = Orchestrator::new(config, backend.clone());

    let (tx, rx) = mpsc::channel(8);

    let handle = tokio::spawn(async move { orchestrator.run(CancellationToken::new(), rx).await });

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    tx.send(OrchestratorCommand::Stop).await.unwrap();

    let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
        .await
        .expect("orchestrator should stop within 2s")
        .expect("task should not panic");

    assert!(result.is_ok());

    let sessions = backend.sessions.lock().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].task_key, "KAN-1");
    assert!(sessions[0].auto_dispatched);
}

#[tokio::test]
async fn orchestrator_kills_session_when_task_becomes_terminal() {
    // Task starts as "todo", then we'll change it to "done" after dispatch
    let source = Arc::new(MockTaskSource::new(
        vec![Task {
            key: "KAN-1".into(),
            title: "Fix bug".into(),
            status: "todo".into(),
            description: "".into(),
            priority: 1,
            blocked_by: vec![],
            subtasks: vec![],
            base_branch: "main".to_string(),
        }],
        vec!["done".into()],
    ));

    /// Backend that tracks kills and transitions task to done after first insert.
    struct KillTrackingBackend {
        sessions: Mutex<Vec<NewSession>>,
        killed: Mutex<Vec<String>>,
        source: Arc<MockTaskSource>,
    }

    impl Backend for KillTrackingBackend {
        fn create_worktree(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn create_tmux_session(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn create_daemon_session(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn insert_session(&self, session: &NewSession) -> Result<(), String> {
            self.sessions.lock().unwrap().push(session.clone());
            // After dispatching, mark the task as done so reconciliation kills it
            let mut tasks = self.source.tasks.lock().unwrap();
            if let Some(t) = tasks.iter_mut().find(|t| t.key == session.task_key) {
                t.status = "done".to_string();
            }
            Ok(())
        }
        fn notify_gui(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn kill_session(&self, session: &NewSession) -> Result<(), String> {
            self.killed.lock().unwrap().push(session.task_key.clone());
            Ok(())
        }
        fn list_active_sessions(&self) -> Result<Vec<NewSession>, String> {
            Ok(vec![])
        }
        fn fetch_base(&self, _: &str, base: &str) -> Result<String, String> {
            Ok(format!("origin/{base}"))
        }
        fn reload_dispatch_config(&self, _: &str) -> Option<DispatchConfig> {
            None
        }
    }

    let backend = Arc::new(KillTrackingBackend {
        sessions: Mutex::new(vec![]),
        killed: Mutex::new(vec![]),
        source: source.clone(),
    });

    let config = OrchestratorConfig {
        poll_interval_ms: 50,
        max_concurrent: 2,
        projects: vec![AutoProject {
            project_id: "p1".to_string(),
            project_name: "testproj".to_string(),
            project_path: "/tmp/testproj".to_string(),
            task_source: source.clone(),
            on_start: None,
            dispatch_config: default_dispatch_config("/tmp/wt"),
        }],
    };

    let orchestrator = Orchestrator::new(config, backend.clone());
    let (tx, rx) = mpsc::channel(8);

    let handle = tokio::spawn(async move { orchestrator.run(CancellationToken::new(), rx).await });

    // Wait for dispatch + reconciliation
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    tx.send(OrchestratorCommand::Stop).await.unwrap();

    let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
        .await
        .unwrap()
        .unwrap();
    assert!(result.is_ok());

    let sessions = backend.sessions.lock().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].task_key, "KAN-1");

    let killed = backend.killed.lock().unwrap();
    assert_eq!(killed.len(), 1);
    assert_eq!(killed[0], "KAN-1");
}

#[tokio::test]
async fn orchestrator_reattaches_active_sessions_on_startup() {
    let source = Arc::new(MockTaskSource::new(
        vec![
            Task {
                key: "KAN-1".into(),
                title: "Task 1".into(),
                status: "todo".into(),
                priority: 1,
                blocked_by: vec![],
                subtasks: vec![],
                ..Default::default()
            },
            Task {
                key: "KAN-2".into(),
                title: "Task 2".into(),
                status: "todo".into(),
                priority: 2,
                blocked_by: vec![],
                subtasks: vec![],
                ..Default::default()
            },
            Task {
                key: "KAN-3".into(),
                title: "Task 3".into(),
                status: "todo".into(),
                priority: 3,
                blocked_by: vec![],
                subtasks: vec![],
                ..Default::default()
            },
        ],
        vec!["done".into()],
    ));

    /// Backend that pre-loads 2 active sessions from "DB".
    #[derive(Default)]
    struct PreloadedBackend {
        dispatched: Mutex<Vec<NewSession>>,
    }

    impl Backend for PreloadedBackend {
        fn create_worktree(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn create_tmux_session(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn create_daemon_session(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn insert_session(&self, session: &NewSession) -> Result<(), String> {
            self.dispatched.lock().unwrap().push(session.clone());
            Ok(())
        }
        fn notify_gui(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn kill_session(&self, _: &NewSession) -> Result<(), String> {
            Ok(())
        }
        fn fetch_base(&self, _: &str, base: &str) -> Result<String, String> {
            Ok(format!("origin/{base}"))
        }
        fn list_active_sessions(&self) -> Result<Vec<NewSession>, String> {
            Ok(vec![
                NewSession {
                    id: "existing-1".to_string(),
                    project_id: "p1".to_string(),
                    project_name: "testproj".to_string(),
                    name: "KAN-1: Task 1".to_string(),
                    tmux_name: Some("planeai-testproj-aaa".to_string()),
                    branch: "kan-1".to_string(),
                    worktree_path: "/tmp/wt/1".to_string(),
                    provider: "kiro".to_string(),
                    backend: "tmux".to_string(),
                    auto_approve: true,
                    task_key: "KAN-1".to_string(),
                    base_branch: "main".to_string(),
                    auto_dispatched: true,
                    command: "kiro-cli chat".to_string(),
                },
                NewSession {
                    id: "existing-2".to_string(),
                    project_id: "p1".to_string(),
                    project_name: "testproj".to_string(),
                    name: "KAN-2: Task 2".to_string(),
                    tmux_name: Some("planeai-testproj-bbb".to_string()),
                    branch: "kan-2".to_string(),
                    worktree_path: "/tmp/wt/2".to_string(),
                    provider: "kiro".to_string(),
                    backend: "tmux".to_string(),
                    auto_approve: true,
                    task_key: "KAN-2".to_string(),
                    base_branch: "main".to_string(),
                    auto_dispatched: true,
                    command: "kiro-cli chat".to_string(),
                },
            ])
        }
        fn reload_dispatch_config(&self, _: &str) -> Option<DispatchConfig> {
            None
        }
    }

    let backend = Arc::new(PreloadedBackend::default());

    let config = OrchestratorConfig {
        poll_interval_ms: 50,
        max_concurrent: 3,
        projects: vec![AutoProject {
            project_id: "p1".to_string(),
            project_name: "testproj".to_string(),
            project_path: "/tmp/testproj".to_string(),
            task_source: source,
            on_start: None,
            dispatch_config: default_dispatch_config("/tmp/wt"),
        }],
    };

    let orchestrator = Orchestrator::new(config, backend.clone());
    let (tx, rx) = mpsc::channel(8);

    let handle = tokio::spawn(async move { orchestrator.run(CancellationToken::new(), rx).await });

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    tx.send(OrchestratorCommand::Stop).await.unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
        .await
        .unwrap()
        .unwrap();
    assert!(result.is_ok());

    // With max_concurrent=3 and 2 pre-existing sessions (KAN-1, KAN-2),
    // only 1 new session should be dispatched (KAN-3)
    let dispatched = backend.dispatched.lock().unwrap();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(dispatched[0].task_key, "KAN-3");
}
