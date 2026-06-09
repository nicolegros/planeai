use std::collections::HashMap;
use std::path::Path;

use crate::task::{Task, TaskManagerConfig};
use crate::template;

/// Operations that interact with git, tmux, DB, and task manager CLI.
/// Injected for testability.
pub trait Backend: Send + Sync {
    fn create_worktree(&self, repo: &str, path: &str, branch: &str, base: &str) -> Result<(), String>;
    fn create_tmux_session(&self, name: &str, cwd: &str, cmd: &str, session_id: &str) -> Result<(), String>;
    fn insert_session(&self, session: &NewSession) -> Result<(), String>;
    fn run_move_task(&self, config: &TaskManagerConfig, key: &str, status: &str, cwd: &Path) -> Result<(), String>;
    fn notify_gui(&self, session_id: &str) -> Result<(), String>;
    fn kill_session(&self, session: &NewSession) -> Result<(), String>;
    fn list_active_sessions(&self) -> Result<Vec<NewSession>, String>;
}

/// Data needed to insert a session into the DB.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub name: String,
    pub tmux_name: Option<String>,
    pub branch: String,
    pub worktree_path: String,
    pub provider: String,
    pub backend: String,
    pub auto_approve: bool,
    pub task_key: String,
    pub base_branch: String,
    pub auto_dispatched: bool,
    pub command: String,
}

/// Configuration for how the orchestrator dispatches sessions.
#[derive(Debug, Clone)]
pub struct DispatchConfig {
    pub provider: String,
    pub provider_command: String,
    pub yolo: bool,
    pub yolo_flag: Option<String>,
    pub worktree_root: String,
    pub base_branch: String,
    pub session_backend: String,
    pub prompt_template: Option<String>,
    pub name_template: Option<String>,
}

pub struct SessionDispatcher {
    pub task_manager_config: TaskManagerConfig,
    pub dispatch_config: DispatchConfig,
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
}

impl SessionDispatcher {
    pub fn dispatch(
        &self,
        task: &Task,
        backend: &dyn Backend,
    ) -> Result<NewSession, String> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let short_id = &session_id.replace('-', "")[..8];

        // Build branch name from task key
        let branch = task.key.to_lowercase().replace(' ', "-");

        // Create worktree
        let wt_path = format!(
            "{}/{}/{}",
            self.dispatch_config.worktree_root, self.project_name, short_id
        );
        backend.create_worktree(
            &self.project_path,
            &wt_path,
            &branch,
            &self.dispatch_config.base_branch,
        )?;

        // Build agent launch command
        let mut cmd = self.dispatch_config.provider_command.clone();
        if self.dispatch_config.yolo {
            if let Some(flag) = &self.dispatch_config.yolo_flag {
                cmd = format!("{cmd} {flag}");
            }
        }

        // Render prompt template if configured
        if let Some(tpl) = &self.dispatch_config.prompt_template {
            let mut vars = HashMap::new();
            vars.insert("key", task.key.as_str());
            vars.insert("title", task.title.as_str());
            vars.insert("description", task.description.as_str());
            let rendered = template::render(tpl, &vars);
            let escaped = format!("'{}'", rendered.replace('\'', "'\\''"));
            cmd = format!("{cmd} {escaped}");
        }

        // Create tmux session
        let tmux_name = format!("planeai-{}-{}", self.project_name, short_id);
        if self.dispatch_config.session_backend == "tmux" {
            backend.create_tmux_session(&tmux_name, &wt_path, &cmd, &session_id)?;
        }

        let session_name = if let Some(tpl) = &self.dispatch_config.name_template {
            let mut vars = HashMap::new();
            vars.insert("key", task.key.as_str());
            vars.insert("title", task.title.as_str());
            vars.insert("description", task.description.as_str());
            vars.insert("status", task.status.as_str());
            template::render(tpl, &vars)
        } else {
            format!("{}: {}", task.key, task.title)
        };

        let new_session = NewSession {
            id: session_id.clone(),
            project_id: self.project_id.clone(),
            project_name: self.project_name.clone(),
            name: session_name,
            tmux_name: Some(tmux_name),
            branch,
            worktree_path: wt_path,
            provider: self.dispatch_config.provider.clone(),
            backend: self.dispatch_config.session_backend.clone(),
            auto_approve: self.dispatch_config.yolo,
            task_key: task.key.clone(),
            base_branch: self.dispatch_config.base_branch.clone(),
            auto_dispatched: true,
            command: cmd,
        };

        backend.insert_session(&new_session)?;

        // Fire on_start hook (move task to in_progress)
        if let Some(hook) = &self.task_manager_config.on_start {
            let _ = backend.run_move_task(
                &self.task_manager_config,
                &task.key,
                &hook.move_to,
                Path::new(&self.project_path),
            );
        }

        // Notify GUI
        let _ = backend.notify_gui(&session_id);

        Ok(new_session)
    }
}
