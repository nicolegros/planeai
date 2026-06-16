use std::collections::HashMap;
use std::sync::Arc;

use crate::task::{Task, TaskSource};
use crate::template;

/// Operations that interact with git, tmux, and DB.
/// Injected for testability.
pub trait Backend: Send + Sync {
    fn create_worktree(
        &self,
        repo: &str,
        path: &str,
        branch: &str,
        base: &str,
    ) -> Result<(), String>;
    fn create_tmux_session(
        &self,
        name: &str,
        cwd: &str,
        cmd: &str,
        session_id: &str,
    ) -> Result<(), String>;
    fn insert_session(&self, session: &NewSession) -> Result<(), String>;
    fn notify_gui(&self, session_id: &str) -> Result<(), String>;
    fn kill_session(&self, session: &NewSession) -> Result<(), String>;
    fn list_active_sessions(&self) -> Result<Vec<NewSession>, String>;
    fn fetch_base(&self, repo: &str, base: &str) -> Result<String, String>;
    /// Reload the dispatch config for a provider. Called before each dispatch to pick up config changes.
    fn reload_dispatch_config(&self, provider: &str) -> Option<DispatchConfig>;
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
    pub prompt_command: Option<String>,
    pub prompt_wrapper: Option<String>,
    pub name_template: Option<String>,
}

/// Lifecycle hook config for on_start.
#[derive(Debug, Clone)]
pub struct OnStartHook {
    pub move_to: String,
}

pub struct SessionDispatcher {
    pub task_source: Arc<dyn TaskSource>,
    pub on_start: Option<OnStartHook>,
    pub dispatch_config: DispatchConfig,
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
}

impl SessionDispatcher {
    pub fn dispatch(&self, task: &Task, backend: &dyn Backend) -> Result<NewSession, String> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let short_id = &session_id.replace('-', "")[..8];

        tracing::info!(task_key = %task.key, session_id = %session_id, project = %self.project_name, "dispatching session");

        // Build branch name from task key + short session id to avoid ref conflicts
        let branch = format!("{}/{}", task.key.to_lowercase().replace(' ', "-"), short_id);

        // Fetch base branch to ensure we have the latest remote ref
        let resolved_base = backend.fetch_base(&self.project_path, &task.base_branch)?;

        // Create worktree
        let wt_path = format!(
            "{}/{}/{}",
            self.dispatch_config.worktree_root, self.project_name, short_id
        );
        backend.create_worktree(&self.project_path, &wt_path, &branch, &resolved_base)?;

        // Build agent launch command
        let mut cmd = self.dispatch_config.provider_command.clone();
        if self.dispatch_config.yolo {
            if let Some(flag) = &self.dispatch_config.yolo_flag {
                cmd = format!("{cmd} {flag}");
            }
        }

        // Render prompt template and inject via prompt_command if both are configured
        if let (Some(tpl), Some(prompt_cmd)) = (
            &self.dispatch_config.prompt_template,
            &self.dispatch_config.prompt_command,
        ) {
            let mut vars = HashMap::new();
            vars.insert("key", task.key.as_str());
            vars.insert("title", task.title.as_str());
            vars.insert("description", task.description.as_str());
            let rendered = template::render(tpl, &vars);

            // Apply prompt_wrapper if set (wraps content before CLI delivery)
            let final_prompt = if let Some(wrapper) = &self.dispatch_config.prompt_wrapper {
                let mut wrap_vars = HashMap::new();
                wrap_vars.insert("prompt", rendered.as_str());
                template::render(wrapper, &wrap_vars)
            } else {
                rendered
            };

            template::append_prompt(&mut cmd, prompt_cmd, &final_prompt);
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
            base_branch: resolved_base,
            auto_dispatched: true,
            command: cmd,
        };

        backend.insert_session(&new_session)?;

        // Fire on_start hook (move task to in_progress)
        if let Some(hook) = &self.on_start {
            tracing::info!(task_key = %task.key, move_to = %hook.move_to, "firing on_start hook");
            let _ = self.task_source.move_task(&task.key, &hook.move_to);
        }

        // Notify GUI
        let _ = backend.notify_gui(&session_id);

        Ok(new_session)
    }
}
