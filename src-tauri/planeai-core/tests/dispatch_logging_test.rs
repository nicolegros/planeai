//! Alone in its binary: the subscriber it installs is the process-wide one, so no parallel
//! test caches the dispatch log callsites as unwanted before it is set.

use planeai_core::session::{Backend, DispatchConfig, NewSession, SessionDispatcher};
use planeai_core::task::{Task, TaskSource};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

struct NoTasks;

impl TaskSource for NoTasks {
    fn list_tasks(&self) -> Result<Vec<Task>, String> {
        Ok(Vec::new())
    }
    fn get_task(&self, key: &str) -> Result<Task, String> {
        Err(format!("no task {key}"))
    }
    fn move_task(&self, _key: &str, _status: &str) -> Result<(), String> {
        Ok(())
    }
    fn is_terminal(&self, _status: &str) -> bool {
        false
    }
}

struct AcceptingBackend;

impl Backend for AcceptingBackend {
    fn create_worktree(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn create_tmux_session(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn create_daemon_session(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn create_rmux_session(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn insert_session(&self, _: &NewSession) -> Result<(), String> {
        Ok(())
    }
    fn notify_gui(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn kill_session(&self, _: &NewSession) -> Result<(), String> {
        Ok(())
    }
    fn list_active_sessions(&self) -> Result<Vec<NewSession>, String> {
        Ok(Vec::new())
    }
    fn list_claimed_task_keys(&self) -> Result<HashSet<String>, String> {
        Ok(HashSet::new())
    }
    fn fetch_base(&self, _: &str, base: &str) -> Result<String, String> {
        Ok(base.to_string())
    }
    fn reload_dispatch_config(&self, _: &str) -> Option<DispatchConfig> {
        None
    }
    fn project_folder(&self, _: &str) -> Option<String> {
        Some("/tmp/testproj".to_string())
    }
}

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn dispatch_logs_the_prompt_size_but_never_its_text() {
    let logs = Logs::default();
    let writer = logs.clone();
    tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .init();
    let dispatcher = SessionDispatcher {
        task_source: Arc::new(NoTasks),
        on_start: None,
        dispatch_config: DispatchConfig {
            provider: "claude".to_string(),
            provider_command: "claude".to_string(),
            yolo: false,
            yolo_flag: None,
            worktree_root: "/tmp/wt".to_string(),
            base_branch: "main".to_string(),
            session_backend: "tmux".to_string(),
            prompt_template: Some("Rotate key {title}".to_string()),
            prompt_command: Some("-- {prompt}".to_string()),
            prompt_wrapper: None,
            name_template: None,
        },
        project_id: "p1".to_string(),
        project_name: "proj".to_string(),
        project_path: "/repo".to_string(),
    };
    let task = Task {
        key: "T-1".to_string(),
        title: "sk-live-1234".to_string(),
        status: "todo".to_string(),
        description: String::new(),
        priority: 1,
        parent_key: None,
        blocked_by: vec![],
        subtasks: vec![],
        base_branch: "main".to_string(),
    };

    dispatcher.dispatch(&task, &AcceptingBackend).unwrap();

    let logs = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("dispatch command built"), "{logs}");
    assert!(logs.contains("prompt_chars=Some(23)"), "{logs}");
    assert!(!logs.contains("sk-live-1234"), "{logs}");
}
