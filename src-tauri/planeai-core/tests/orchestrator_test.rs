#![cfg(unix)]
use planeai_core::orchestrator::{
    AutoProject, Orchestrator, OrchestratorCommand, OrchestratorConfig,
};
use planeai_core::session::{Backend, NewSession};
use planeai_core::task::{LifecycleHook, TaskManagerConfig};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Backend that records dispatched sessions.
#[derive(Default)]
struct TestBackend {
    worktrees: Mutex<Vec<String>>,
    sessions: Mutex<Vec<NewSession>>,
}

impl Backend for TestBackend {
    fn create_worktree(
        &self,
        _repo: &str,
        path: &str,
        _branch: &str,
        _base: &str,
    ) -> Result<(), String> {
        self.worktrees.lock().unwrap().push(path.to_string());
        Ok(())
    }
    fn create_tmux_session(
        &self,
        _name: &str,
        _cwd: &str,
        _cmd: &str,
        _sid: &str,
    ) -> Result<(), String> {
        Ok(())
    }
    fn insert_session(&self, session: &NewSession) -> Result<(), String> {
        self.sessions.lock().unwrap().push(session.clone());
        Ok(())
    }
    fn run_move_task(
        &self,
        _cfg: &TaskManagerConfig,
        _key: &str,
        _status: &str,
        _cwd: &Path,
    ) -> Result<(), String> {
        Ok(())
    }
    fn notify_gui(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }
    fn kill_session(&self, _session: &NewSession) -> Result<(), String> {
        Ok(())
    }
    fn list_active_sessions(&self) -> Result<Vec<NewSession>, String> {
        Ok(vec![])
    }
    fn fetch_base(&self, _repo: &str, base: &str) -> Result<String, String> {
        Ok(format!("origin/{base}"))
    }
    fn reload_dispatch_config(
        &self,
        _provider: &str,
    ) -> Option<planeai_core::session::DispatchConfig> {
        None
    }
}

fn write_script(dir: &Path, name: &str, output: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\nprintf '%s' '{output}'")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

#[tokio::test]
async fn orchestrator_polls_dispatches_and_stops_on_channel_command() {
    let dir = tempdir().unwrap();

    let list_json = r#"[{"key":"KAN-1","title":"Fix bug","status":"todo","description":"desc","priority":1,"blocked_by":[]}]"#;
    let list_script = write_script(dir.path(), "list.sh", list_json);

    let config = OrchestratorConfig {
        poll_interval_ms: 50,
        max_concurrent: 2,
        projects: vec![AutoProject {
            project_id: "p1".to_string(),
            project_name: "testproj".to_string(),
            project_path: dir.path().to_string_lossy().to_string(),
            task_manager_config: TaskManagerConfig {
                list_tasks: format!("{list_script} --project {{project}}"),
                get_task: String::new(),
                move_task: String::new(),
                terminal_states: vec!["done".to_string()],
                on_start: Some(LifecycleHook {
                    move_to: "in_progress".to_string(),
                }),
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
                prompt_command: None,
                prompt_wrapper: None,
                name_template: None,
            },
        }],
    };

    let backend = Arc::new(TestBackend::default());
    let orchestrator = Orchestrator::new(config, backend.clone());

    let (tx, rx) = mpsc::channel(8);

    // Spawn orchestrator in background
    let handle = tokio::spawn(async move { orchestrator.run(CancellationToken::new(), rx).await });

    // Wait for at least one poll tick to fire
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Send stop command via channel
    tx.send(OrchestratorCommand::Stop).await.unwrap();

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

#[tokio::test]
async fn orchestrator_kills_session_when_task_becomes_terminal() {
    let dir = tempdir().unwrap();

    // The list script returns "todo" on first call, then "done" on subsequent calls.
    let state_file = dir.path().join("call_count");
    fs::write(&state_file, "0").unwrap();

    let script_content = format!(
        r#"#!/bin/sh
count=$(cat {state})
if [ "$count" = "0" ]; then
    echo 1 > {state}
    printf '[{{"key":"KAN-1","title":"Fix bug","status":"todo","description":"","priority":1,"blocked_by":[]}}]'
else
    printf '[{{"key":"KAN-1","title":"Fix bug","status":"done","description":"","priority":1,"blocked_by":[]}}]'
fi
"#,
        state = state_file.display()
    );
    let list_script = dir.path().join("list.sh");
    fs::write(&list_script, &script_content).unwrap();
    fs::set_permissions(&list_script, fs::Permissions::from_mode(0o755)).unwrap();

    // get_task also returns "done" (for reconciliation check)
    let get_json = r#"{"key":"KAN-1","title":"Fix bug","status":"done","description":"","priority":1,"blocked_by":[]}"#;
    let get_script = write_script(dir.path(), "get.sh", get_json);

    let config = OrchestratorConfig {
        poll_interval_ms: 50,
        max_concurrent: 2,
        projects: vec![AutoProject {
            project_id: "p1".to_string(),
            project_name: "testproj".to_string(),
            project_path: dir.path().to_string_lossy().to_string(),
            task_manager_config: TaskManagerConfig {
                list_tasks: format!("{}", list_script.display()),
                get_task: format!("{} {{key}}", get_script),
                move_task: String::new(),
                terminal_states: vec!["done".to_string()],
                on_start: None,
            },
            dispatch_config: planeai_core::session::DispatchConfig {
                provider: "kiro".to_string(),
                provider_command: "kiro-cli chat".to_string(),
                yolo: true,
                yolo_flag: None,
                worktree_root: dir.path().join("wt").to_string_lossy().to_string(),
                base_branch: "main".to_string(),
                session_backend: "tmux".to_string(),
                prompt_template: None,
                prompt_command: None,
                prompt_wrapper: None,
                name_template: None,
            },
        }],
    };

    /// Backend that tracks kills.
    #[derive(Default)]
    struct KillTrackingBackend {
        sessions: Mutex<Vec<NewSession>>,
        killed: Mutex<Vec<String>>,
    }

    impl Backend for KillTrackingBackend {
        fn create_worktree(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn create_tmux_session(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn insert_session(&self, session: &NewSession) -> Result<(), String> {
            self.sessions.lock().unwrap().push(session.clone());
            Ok(())
        }
        fn run_move_task(
            &self,
            _: &TaskManagerConfig,
            _: &str,
            _: &str,
            _: &Path,
        ) -> Result<(), String> {
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
        fn fetch_base(&self, _repo: &str, base: &str) -> Result<String, String> {
            Ok(format!("origin/{base}"))
        }
        fn reload_dispatch_config(
            &self,
            _provider: &str,
        ) -> Option<planeai_core::session::DispatchConfig> {
            None
        }
    }

    let backend = Arc::new(KillTrackingBackend::default());
    let orchestrator = Orchestrator::new(config, backend.clone());

    let (tx, rx) = mpsc::channel(8);

    let handle = tokio::spawn(async move { orchestrator.run(CancellationToken::new(), rx).await });

    // Wait for dispatch + reconciliation (at least 3 ticks)
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    // Stop the orchestrator
    tx.send(OrchestratorCommand::Stop).await.unwrap();

    let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
        .await
        .unwrap()
        .unwrap();
    assert!(result.is_ok());

    // Session was dispatched
    let sessions = backend.sessions.lock().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].task_key, "KAN-1");

    // Session was killed due to terminal state
    let killed = backend.killed.lock().unwrap();
    assert_eq!(killed.len(), 1);
    assert_eq!(killed[0], "KAN-1");
}

#[tokio::test]
async fn orchestrator_reattaches_active_sessions_on_startup() {
    let dir = tempdir().unwrap();

    // list_tasks returns 3 tasks
    let list_json = r#"[
        {"key":"KAN-1","title":"Task 1","status":"todo","description":"","priority":1,"blocked_by":[]},
        {"key":"KAN-2","title":"Task 2","status":"todo","description":"","priority":2,"blocked_by":[]},
        {"key":"KAN-3","title":"Task 3","status":"todo","description":"","priority":3,"blocked_by":[]}
    ]"#;
    let list_script = write_script(dir.path(), "list.sh", list_json);
    let get_json = r#"{"key":"KAN-1","title":"Task 1","status":"todo","description":"","priority":1,"blocked_by":[]}"#;
    let get_script = write_script(dir.path(), "get.sh", get_json);

    let config = OrchestratorConfig {
        poll_interval_ms: 50,
        max_concurrent: 3, // only 3 slots total
        projects: vec![AutoProject {
            project_id: "p1".to_string(),
            project_name: "testproj".to_string(),
            project_path: dir.path().to_string_lossy().to_string(),
            task_manager_config: TaskManagerConfig {
                list_tasks: format!("{list_script} --project {{project}}"),
                get_task: format!("{get_script} {{key}}"),
                move_task: String::new(),
                terminal_states: vec!["done".to_string()],
                on_start: None,
            },
            dispatch_config: planeai_core::session::DispatchConfig {
                provider: "kiro".to_string(),
                provider_command: "kiro-cli chat".to_string(),
                yolo: true,
                yolo_flag: None,
                worktree_root: dir.path().join("wt").to_string_lossy().to_string(),
                base_branch: "main".to_string(),
                session_backend: "tmux".to_string(),
                prompt_template: None,
                prompt_command: None,
                prompt_wrapper: None,
                name_template: None,
            },
        }],
    };

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
        fn insert_session(&self, session: &NewSession) -> Result<(), String> {
            self.dispatched.lock().unwrap().push(session.clone());
            Ok(())
        }
        fn run_move_task(
            &self,
            _: &TaskManagerConfig,
            _: &str,
            _: &str,
            _: &Path,
        ) -> Result<(), String> {
            Ok(())
        }
        fn notify_gui(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn kill_session(&self, _: &NewSession) -> Result<(), String> {
            Ok(())
        }
        fn fetch_base(&self, _repo: &str, base: &str) -> Result<String, String> {
            Ok(format!("origin/{base}"))
        }
        fn list_active_sessions(&self) -> Result<Vec<NewSession>, String> {
            // Simulate 2 sessions already running from a previous daemon lifecycle
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
        fn reload_dispatch_config(
            &self,
            _provider: &str,
        ) -> Option<planeai_core::session::DispatchConfig> {
            None
        }
    }

    let backend = Arc::new(PreloadedBackend::default());
    let orchestrator = Orchestrator::new(config, backend.clone());

    let (tx, rx) = mpsc::channel(8);

    let handle = tokio::spawn(async move { orchestrator.run(CancellationToken::new(), rx).await });

    // Wait for dispatch
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Stop
    tx.send(OrchestratorCommand::Stop).await.unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
        .await
        .unwrap()
        .unwrap();
    assert!(result.is_ok());

    // With max_concurrent=3 and 2 pre-existing sessions (KAN-1, KAN-2),
    // only 1 new session should be dispatched (KAN-3, since KAN-1 and KAN-2 are claimed)
    let dispatched = backend.dispatched.lock().unwrap();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(dispatched[0].task_key, "KAN-3");
}
